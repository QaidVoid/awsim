//! Basic Amazon States Language (ASL) interpreter.
//!
//! Supports: Pass, Succeed, Fail, Wait, Task, Choice, Parallel, Map.
//! InputPath / OutputPath / ResultPath transformations are supported.

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::HistoryEvent;

/// Result of executing an ASL state machine.
pub struct ExecResult {
    pub status: String, // SUCCEEDED or FAILED
    pub output: Option<String>,
    pub error: Option<String>,
    pub cause: Option<String>,
    pub history: Vec<HistoryEvent>,
}

/// Walk through the ASL starting from StartAt.
pub fn execute(definition: &str, input: &str, start_time: &str) -> ExecResult {
    execute_typed(definition, input, start_time, false)
}

/// Same as [`execute`], but flags whether the state machine is EXPRESS
/// so the interpreter can enforce the 5-minute cap on accumulated
/// `Wait` seconds.
pub fn execute_typed(
    definition: &str,
    input: &str,
    start_time: &str,
    is_express: bool,
) -> ExecResult {
    let def: Value = match serde_json::from_str(definition) {
        Ok(v) => v,
        Err(e) => {
            return ExecResult {
                status: "FAILED".to_string(),
                output: None,
                error: Some("InvalidDefinition".to_string()),
                cause: Some(e.to_string()),
                history: Vec::new(),
            };
        }
    };

    let input_val: Value = serde_json::from_str(input).unwrap_or(Value::Null);

    let mut ctx = InterpreterContext {
        states: def["States"].clone(),
        history: Vec::new(),
        event_counter: 0,
        start_time: start_time.to_string(),
        is_express,
        simulated_wait_secs: 0,
    };

    let start_at = match def["StartAt"].as_str() {
        Some(s) => s.to_string(),
        None => {
            return ExecResult {
                status: "FAILED".to_string(),
                output: None,
                error: Some("InvalidDefinition".to_string()),
                cause: Some("Missing StartAt".to_string()),
                history: ctx.history,
            };
        }
    };

    ctx.push_event("ExecutionStarted", json!({ "input": input }));

    match ctx.run_state(&start_at, input_val) {
        Ok(output) => {
            let output_str = output.to_string();
            ctx.push_event("ExecutionSucceeded", json!({ "output": output_str }));
            ExecResult {
                status: "SUCCEEDED".to_string(),
                output: Some(output_str),
                error: None,
                cause: None,
                history: ctx.history,
            }
        }
        Err(failure) => {
            ctx.push_event(
                "ExecutionFailed",
                json!({
                    "error": failure.error,
                    "cause": failure.cause,
                }),
            );
            ExecResult {
                status: "FAILED".to_string(),
                output: None,
                error: Some(failure.error),
                cause: Some(failure.cause),
                history: ctx.history,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct StateFailed {
    error: String,
    cause: String,
}

/// The service integration mode encoded in a Task `Resource` ARN suffix.
/// AWS Step Functions reads four shapes:
/// - request-response (default, no suffix)
/// - `.sync` / `.sync:2` (wait until the called job reaches a terminal state)
/// - `.waitForTaskToken` (pause until SendTaskSuccess/Failure)
/// - `.async` (fire-and-forget; return immediately)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskIntegration {
    RequestResponse,
    Sync,
    WaitForTaskToken,
    Async,
}

impl TaskIntegration {
    fn from_resource(resource: &str) -> Self {
        let suffix = resource.rsplit_once('.').map(|(_, s)| s).unwrap_or("");
        match suffix {
            "waitForTaskToken" => Self::WaitForTaskToken,
            "async" => Self::Async,
            "sync" | "sync:2" => Self::Sync,
            _ => Self::RequestResponse,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::RequestResponse => "request-response",
            Self::Sync => "sync",
            Self::WaitForTaskToken => "waitForTaskToken",
            Self::Async => "async",
        }
    }
}

struct InterpreterContext {
    states: Value,
    history: Vec<HistoryEvent>,
    event_counter: u64,
    start_time: String,
    /// EXPRESS workflows have a hard 5-minute (300 s) cap on the
    /// accumulated simulated wait time. Tracked so the interpreter can
    /// short-circuit with `States.Timeout` when a definition would have
    /// exceeded that bound in real AWS.
    is_express: bool,
    simulated_wait_secs: u64,
}

impl InterpreterContext {
    fn push_event(&mut self, event_type: &str, details: Value) {
        self.event_counter += 1;
        self.history.push(HistoryEvent {
            id: self.event_counter,
            event_type: event_type.to_string(),
            timestamp: self.start_time.clone(),
            details,
        });
    }

    fn run_state(&mut self, state_name: &str, input: Value) -> Result<Value, StateFailed> {
        // Guard against infinite loops
        if self.event_counter > 1000 {
            return Err(StateFailed {
                error: "ExecutionLimitExceeded".to_string(),
                cause: "Too many state transitions".to_string(),
            });
        }

        let state = self.states[state_name].clone();
        if state.is_null() {
            return Err(StateFailed {
                error: "NoSuchState".to_string(),
                cause: format!("State '{state_name}' not found in definition"),
            });
        }

        let state_type = state["Type"].as_str().unwrap_or("Pass");

        self.push_event(
            "StateEntered",
            json!({ "name": state_name, "type": state_type, "input": input }),
        );

        let after_input_path = apply_input_path(&input, state["InputPath"].as_str());
        // Parameters runs after InputPath but before the state body. Each
        // value whose key ends in `.$` is resolved as a JSONPath into the
        // post-InputPath input; everything else is a static literal.
        let effective_input = match state.get("Parameters") {
            Some(p) => apply_parameters(p, &after_input_path),
            None => after_input_path.clone(),
        };

        // Retry: re-run the state body up to MaxAttempts when the failure
        // matches an ErrorEquals entry. We don't sleep IntervalSeconds —
        // tasks already run synchronously here.
        let max_attempts = max_retry_attempts(&state);
        let mut attempt: u32 = 0;
        let result = loop {
            let attempt_result = match state_type {
                "Pass" => self.exec_pass(&state, effective_input.clone()),
                "Succeed" => self.exec_succeed(&state, effective_input.clone()),
                "Fail" => self.exec_fail(&state),
                "Wait" => self.exec_wait(&state, effective_input.clone()),
                "Task" => self.exec_task(state_name, &state, effective_input.clone()),
                "Choice" => self.exec_choice(&state, effective_input.clone()),
                "Parallel" => self.exec_parallel(&state, effective_input.clone()),
                "Map" => self.exec_map(&state, effective_input.clone()),
                other => Err(StateFailed {
                    error: "UnsupportedStateType".to_string(),
                    cause: format!("State type '{other}' is not supported"),
                }),
            };

            match attempt_result {
                Ok(ok) => break Ok(ok),
                Err(err) => {
                    if attempt < max_attempts && retry_matches(&state, &err.error) {
                        attempt += 1;
                        self.push_event(
                            "StateRetrying",
                            json!({
                                "name": state_name,
                                "attempt": attempt,
                                "error": err.error,
                                "cause": err.cause,
                            }),
                        );
                        continue;
                    }
                    break Err(err);
                }
            }
        };

        match result {
            Ok((raw_output, next)) => {
                // ResultSelector → ResultPath → OutputPath is the AWS pipeline.
                // Choice/Wait/Succeed don't carry a "result" so we skip
                // ResultSelector/ResultPath for them and just apply OutputPath
                // to whatever they returned (typically the input).
                let has_result = matches!(state_type, "Pass" | "Task" | "Parallel" | "Map");
                let after_post = if has_result {
                    let after_selector = match state.get("ResultSelector") {
                        Some(rs) => apply_parameters(rs, &raw_output),
                        None => raw_output.clone(),
                    };
                    apply_result_path(
                        &after_input_path,
                        &after_selector,
                        state["ResultPath"].as_str(),
                    )
                } else {
                    raw_output
                };
                let final_output = apply_output_path(&after_post, state["OutputPath"].as_str());

                self.push_event(
                    "StateExited",
                    json!({ "name": state_name, "output": final_output }),
                );

                match next {
                    StateTransition::End => Ok(final_output),
                    StateTransition::Next(next_state) => self.run_state(&next_state, final_output),
                }
            }
            Err(err) => {
                // Catch: route to a fallback state with the error info
                // attached at ResultPath (default `$`). State counts as
                // succeeded for the purpose of execution status.
                if let Some((next_state, result_path)) = catch_target(&state, &err.error) {
                    let error_payload = json!({
                        "Error": err.error,
                        "Cause": err.cause,
                    });
                    let merged =
                        apply_result_path(&effective_input, &error_payload, Some(&result_path));
                    self.push_event(
                        "StateCaught",
                        json!({
                            "name": state_name,
                            "next": next_state,
                            "error": err.error,
                        }),
                    );
                    return self.run_state(&next_state, merged);
                }
                Err(err)
            }
        }
    }

    fn exec_pass(
        &mut self,
        state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        // Raw result; ResultSelector + ResultPath handled by run_state.
        let result = state.get("Result").cloned().unwrap_or(input);
        Ok((result, transition(state)))
    }

    fn exec_succeed(
        &mut self,
        _state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        Ok((input, StateTransition::End))
    }

    fn exec_fail(&mut self, state: &Value) -> Result<(Value, StateTransition), StateFailed> {
        let error = state["Error"]
            .as_str()
            .unwrap_or("States.TaskFailed")
            .to_string();
        let cause = state["Cause"].as_str().unwrap_or("").to_string();
        Err(StateFailed { error, cause })
    }

    fn exec_wait(
        &mut self,
        state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        // The dev emulator skips real sleeping, but for EXPRESS
        // workflows AWS still enforces a 5-minute cap on the total
        // wait time accumulated across the execution. Sum the
        // documented `Seconds` parameter and trip States.Timeout
        // before the next state runs when the cap is exceeded.
        if let Some(secs) = state.get("Seconds").and_then(Value::as_u64) {
            self.simulated_wait_secs = self.simulated_wait_secs.saturating_add(secs);
            if self.is_express && self.simulated_wait_secs > 300 {
                return Err(StateFailed {
                    error: "States.Timeout".to_string(),
                    cause: format!(
                        "EXPRESS state machine exceeded the 5-minute (300 s) cap; \
                         accumulated wait = {} s.",
                        self.simulated_wait_secs
                    ),
                });
            }
        }
        Ok((input, transition(state)))
    }

    fn exec_task(
        &mut self,
        state_name: &str,
        state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        let resource = state["Resource"].as_str().unwrap_or("unknown");
        let integration = TaskIntegration::from_resource(resource);

        self.push_event(
            "TaskStateEntered",
            json!({
                "name": state_name,
                "resource": resource,
                "integration": integration.label(),
                "input": input,
            }),
        );

        // TimeoutSeconds: a Task with a non-positive deadline trips
        // States.Timeout right away. The simulator also honors an opt-in
        // `_simulateTimeout` marker in the input so tests can exercise
        // Retry+Catch behavior without real wall-clock waits. Real AWS
        // raises States.Timeout when the configured deadline lapses; in
        // either case the resulting error feeds the Retry/Catch
        // evaluation in `run_state`.
        let timeout = state.get("TimeoutSeconds").and_then(Value::as_i64);
        let simulate_timeout = input
            .get("_simulateTimeout")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if simulate_timeout || matches!(timeout, Some(s) if s <= 0) {
            return Err(StateFailed {
                error: "States.Timeout".to_string(),
                cause: "Task timed out before producing a result".to_string(),
            });
        }

        // `.async` integrations return immediately with a minimal
        // acknowledgement; the simulator never blocks waiting on the
        // downstream service. Other integration modes echo the input as
        // the mock result so callers can keep authoring real state
        // machine definitions without changing the test surface.
        let mock_output = match integration {
            TaskIntegration::Async => json!({ "Status": "Accepted", "StatusCode": 202 }),
            _ => input.clone(),
        };

        let event = match integration {
            TaskIntegration::Async => "TaskSubmitted",
            _ => "TaskSucceeded",
        };
        self.push_event(
            event,
            json!({
                "name": state_name,
                "resource": resource,
                "output": mock_output,
            }),
        );

        Ok((mock_output, transition(state)))
    }

    fn exec_choice(
        &mut self,
        state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        let choices = match state["Choices"].as_array() {
            Some(c) => c,
            None => {
                return Err(StateFailed {
                    error: "InvalidDefinition".to_string(),
                    cause: "Choice state missing Choices".to_string(),
                });
            }
        };

        for choice in choices {
            if evaluate_condition(choice, &input) {
                let next = choice["Next"]
                    .as_str()
                    .ok_or_else(|| StateFailed {
                        error: "InvalidDefinition".to_string(),
                        cause: "Choice branch missing Next".to_string(),
                    })?
                    .to_string();
                return Ok((input, StateTransition::Next(next)));
            }
        }

        // Default
        if let Some(default) = state["Default"].as_str() {
            return Ok((input, StateTransition::Next(default.to_string())));
        }

        Err(StateFailed {
            error: "States.NoChoiceMatched".to_string(),
            cause: "No condition matched and no Default specified".to_string(),
        })
    }

    fn exec_parallel(
        &mut self,
        state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        let branches = state["Branches"].as_array().cloned().unwrap_or_default();
        let mut outputs: Vec<Value> = Vec::with_capacity(branches.len());
        for branch in &branches {
            let branch_def = branch.to_string();
            let branch_result = execute(&branch_def, &input.to_string(), &self.start_time);
            if branch_result.status == "FAILED" {
                return Err(StateFailed {
                    error: branch_result.error.unwrap_or_default(),
                    cause: branch_result.cause.unwrap_or_default(),
                });
            }
            let output_str = branch_result.output.unwrap_or_else(|| "null".to_string());
            outputs.push(serde_json::from_str(&output_str).unwrap_or(Value::Null));
        }
        // Raw result; run_state handles ResultSelector + ResultPath.
        Ok((Value::Array(outputs), transition(state)))
    }

    fn exec_map(
        &mut self,
        state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        let items_path = state["ItemsPath"].as_str().unwrap_or("$");
        let items = resolve_reference_path(&input, items_path);
        // ItemProcessor (newer ASL) supersedes Iterator (legacy) but the
        // payload shape is identical; honor either.
        let iterator_def = if state.get("ItemProcessor").is_some() {
            state["ItemProcessor"].clone()
        } else {
            state["Iterator"].clone()
        };

        let item_array: Vec<Value> = items
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![items.clone()]);

        // ItemSelector (Map 2.0) reshapes each item into the payload the
        // iterator receives. AWS evaluates ItemSelector keys ending in
        // `.$` against the raw item, mirroring Parameters. Absent
        // selector falls through to the bare item.
        let item_selector = state.get("ItemSelector").cloned();
        let iter_def_str = iterator_def.to_string();
        let mut outputs: Vec<Value> = Vec::with_capacity(item_array.len());
        for item in &item_array {
            let effective = match &item_selector {
                Some(sel) if !sel.is_null() => apply_parameters(sel, item),
                _ => item.clone(),
            };
            let item_result = execute(&iter_def_str, &effective.to_string(), &self.start_time);
            if item_result.status == "FAILED" {
                return Err(StateFailed {
                    error: item_result.error.unwrap_or_default(),
                    cause: item_result.cause.unwrap_or_default(),
                });
            }
            let item_output_str = item_result.output.unwrap_or_else(|| "null".to_string());
            outputs.push(serde_json::from_str(&item_output_str).unwrap_or(Value::Null));
        }
        // Raw result; run_state handles ResultSelector + ResultPath.
        Ok((Value::Array(outputs), transition(state)))
    }
}

// ---------------------------------------------------------------------------
// Parameters / ResultSelector
// ---------------------------------------------------------------------------

/// Recursively transform a `Parameters` (or `ResultSelector`) template
/// against a source object. Keys ending in `.$` carry either a JSONPath
/// reference into `source` (e.g. `$.user.id`) or an intrinsic function
/// invocation (`States.Format(...)`, `States.JsonToString(...)`, etc.)
/// and are renamed to drop the suffix in the output. Object / array
/// values recurse; everything else is a literal.
fn apply_parameters(template: &Value, source: &Value) -> Value {
    match template {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if let Some(stripped_key) = k.strip_suffix(".$") {
                    let resolved = match v.as_str() {
                        Some(s) if s.starts_with("States.") => {
                            evaluate_intrinsic(s, source).unwrap_or_else(|| v.clone())
                        }
                        Some(path) if path.starts_with('$') => resolve_reference_path(source, path),
                        _ => v.clone(),
                    };
                    out.insert(stripped_key.to_string(), resolved);
                } else {
                    out.insert(k.clone(), apply_parameters(v, source));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| apply_parameters(v, source)).collect())
        }
        // Scalars pass through.
        _ => template.clone(),
    }
}

/// Evaluate an ASL intrinsic function call against `source`. Returns
/// `None` when the call isn't a recognized intrinsic (caller falls back
/// to treating it as an opaque literal).
///
/// Implements the documented AWS States intrinsics set. Each function
/// matches the AWS shape — argument count and return type — closely
/// enough to slot into existing Parameters / ResultSelector / Map
/// ItemSelector blocks.
fn evaluate_intrinsic(expr: &str, source: &Value) -> Option<Value> {
    let expr = expr.trim();
    let (name, args_str) = expr
        .strip_prefix("States.")
        .and_then(|rest| rest.split_once('('))
        .and_then(|(name, rest)| rest.strip_suffix(')').map(|inner| (name, inner.trim())))?;
    let args = parse_intrinsic_args(args_str);

    match name {
        "Format" => {
            let raw = args.first()?;
            let template = resolve_intrinsic_arg_str(raw, source)?;
            let mut out = String::with_capacity(template.len());
            let mut chars = template.chars().peekable();
            let mut arg_idx = 1usize;
            while let Some(c) = chars.next() {
                match c {
                    '{' if chars.peek() == Some(&'}') => {
                        chars.next();
                        let arg = args.get(arg_idx)?;
                        arg_idx += 1;
                        let value = resolve_intrinsic_arg(arg, source)?;
                        out.push_str(&intrinsic_arg_to_format_string(&value));
                    }
                    '\\' if chars.peek() == Some(&'{') || chars.peek() == Some(&'}') => {
                        if let Some(escaped) = chars.next() {
                            out.push(escaped);
                        }
                    }
                    _ => out.push(c),
                }
            }
            Some(Value::String(out))
        }
        "JsonToString" => {
            let arg = args.first()?;
            let value = resolve_intrinsic_arg(arg, source)?;
            Some(Value::String(value.to_string()))
        }
        "StringToJson" => {
            let arg = args.first()?;
            let s = resolve_intrinsic_arg_str(arg, source)?;
            serde_json::from_str(&s).ok()
        }
        "Array" => Some(Value::Array(
            args.iter()
                .filter_map(|a| resolve_intrinsic_arg(a, source))
                .collect(),
        )),
        "ArrayPartition" => {
            let arr = resolve_intrinsic_arg(args.first()?, source)?
                .as_array()?
                .clone();
            let size = resolve_intrinsic_arg(args.get(1)?, source)?
                .as_u64()
                .filter(|n| *n > 0)? as usize;
            let chunks: Vec<Value> = arr.chunks(size).map(|c| Value::Array(c.to_vec())).collect();
            Some(Value::Array(chunks))
        }
        "ArrayContains" => {
            let arr = resolve_intrinsic_arg(args.first()?, source)?;
            let needle = resolve_intrinsic_arg(args.get(1)?, source)?;
            Some(Value::Bool(
                arr.as_array()
                    .map(|a| a.iter().any(|v| v == &needle))
                    .unwrap_or(false),
            ))
        }
        "ArrayRange" => {
            let start = resolve_intrinsic_arg(args.first()?, source)?.as_i64()?;
            let end = resolve_intrinsic_arg(args.get(1)?, source)?.as_i64()?;
            let step = resolve_intrinsic_arg(args.get(2)?, source)?.as_i64()?;
            if step == 0 {
                return None;
            }
            let mut out = Vec::new();
            let mut v = start;
            while (step > 0 && v <= end) || (step < 0 && v >= end) {
                out.push(Value::from(v));
                v += step;
            }
            Some(Value::Array(out))
        }
        "ArrayGetItem" => {
            let arr = resolve_intrinsic_arg(args.first()?, source)?
                .as_array()?
                .clone();
            let idx = resolve_intrinsic_arg(args.get(1)?, source)?.as_u64()? as usize;
            arr.get(idx).cloned()
        }
        "ArrayLength" => {
            let arr = resolve_intrinsic_arg(args.first()?, source)?;
            Some(Value::from(arr.as_array().map(|a| a.len() as u64)?))
        }
        "ArrayUnique" => {
            let arr = resolve_intrinsic_arg(args.first()?, source)?
                .as_array()?
                .clone();
            let mut out: Vec<Value> = Vec::with_capacity(arr.len());
            for v in arr {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
            Some(Value::Array(out))
        }
        "ArrayConcat" => {
            let mut out = Vec::new();
            for a in &args {
                let v = resolve_intrinsic_arg(a, source)?;
                if let Some(items) = v.as_array() {
                    out.extend_from_slice(items);
                }
            }
            Some(Value::Array(out))
        }
        "Base64Encode" => {
            use base64::Engine;
            use base64::engine::general_purpose::STANDARD;
            let s = resolve_intrinsic_arg_str(args.first()?, source)?;
            Some(Value::String(STANDARD.encode(s.as_bytes())))
        }
        "Base64Decode" => {
            use base64::Engine;
            use base64::engine::general_purpose::STANDARD;
            let s = resolve_intrinsic_arg_str(args.first()?, source)?;
            let bytes = STANDARD.decode(s.trim()).ok()?;
            String::from_utf8(bytes).ok().map(Value::String)
        }
        "Hash" => {
            use sha2::{Digest, Sha256, Sha384, Sha512};
            let input = resolve_intrinsic_arg_str(args.first()?, source)?;
            let algo = resolve_intrinsic_arg_str(args.get(1)?, source)?;
            let digest_hex = match algo.as_str() {
                "SHA-256" => {
                    let mut h = Sha256::new();
                    h.update(input.as_bytes());
                    format!("{:x}", h.finalize())
                }
                "SHA-384" => {
                    let mut h = Sha384::new();
                    h.update(input.as_bytes());
                    format!("{:x}", h.finalize())
                }
                "SHA-512" => {
                    let mut h = Sha512::new();
                    h.update(input.as_bytes());
                    format!("{:x}", h.finalize())
                }
                _ => return None,
            };
            Some(Value::String(digest_hex))
        }
        "MathRandom" => {
            let start = resolve_intrinsic_arg(args.first()?, source)?.as_i64()?;
            let end = resolve_intrinsic_arg(args.get(1)?, source)?.as_i64()?;
            if end <= start {
                return None;
            }
            let mut nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64)
                .unwrap_or(0);
            // Mix with caller-provided seed when supplied (AWS optional 3rd arg).
            if let Some(seed_arg) = args.get(2)
                && let Some(seed) = resolve_intrinsic_arg(seed_arg, source).and_then(|v| v.as_i64())
            {
                nanos ^= seed as u64;
            }
            let span = (end - start) as u64;
            let v = start + (nanos % span) as i64;
            Some(Value::from(v))
        }
        "MathAdd" => {
            let a = resolve_intrinsic_arg(args.first()?, source)?.as_i64()?;
            let b = resolve_intrinsic_arg(args.get(1)?, source)?.as_i64()?;
            Some(Value::from(a + b))
        }
        "StringSplit" => {
            let s = resolve_intrinsic_arg_str(args.first()?, source)?;
            let sep = resolve_intrinsic_arg_str(args.get(1)?, source)?;
            let parts = if sep.is_empty() {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            } else {
                s.split(&sep[..])
                    .map(|p| Value::String(p.to_string()))
                    .collect::<Vec<_>>()
            };
            Some(Value::Array(parts))
        }
        "JsonMerge" => {
            let mut a = resolve_intrinsic_arg(args.first()?, source)?;
            let b = resolve_intrinsic_arg(args.get(1)?, source)?;
            let deep = args
                .get(2)
                .and_then(|s| resolve_intrinsic_arg(s, source))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            json_merge(&mut a, &b, deep);
            Some(a)
        }
        "UUID" => Some(Value::String(uuid::Uuid::new_v4().to_string())),
        "IsBoolean" => Some(Value::Bool(
            resolve_intrinsic_arg(args.first()?, source)?.is_boolean(),
        )),
        "IsNull" => Some(Value::Bool(
            resolve_intrinsic_arg(args.first()?, source)?.is_null(),
        )),
        "IsNumeric" => Some(Value::Bool(
            resolve_intrinsic_arg(args.first()?, source)?.is_number(),
        )),
        "IsString" => Some(Value::Bool(
            resolve_intrinsic_arg(args.first()?, source)?.is_string(),
        )),
        "IsPresent" => {
            let v = resolve_intrinsic_arg(args.first()?, source);
            Some(Value::Bool(matches!(v, Some(ref x) if !x.is_null())))
        }
        "IsTimestamp" => {
            let raw = resolve_intrinsic_arg_str(args.first()?, source)?;
            // Loose ISO-8601 with optional offset: YYYY-MM-DDTHH:MM:SS(.fff)?(Z|±HH:MM)
            let re = regex::Regex::new(
                r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:?\d{2})?$",
            )
            .ok()?;
            Some(Value::Bool(re.is_match(&raw)))
        }
        _ => None,
    }
}

fn json_merge(target: &mut Value, source: &Value, deep: bool) {
    match (target, source) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                if deep
                    && let Some(existing) = a.get_mut(k)
                    && existing.is_object()
                    && v.is_object()
                {
                    json_merge(existing, v, true);
                } else {
                    a.insert(k.clone(), v.clone());
                }
            }
        }
        (t, s) => {
            *t = s.clone();
        }
    }
}

/// Split intrinsic-function arguments at top-level commas, honoring
/// quoted strings so `States.Format('a, b', $.x)` stays as two args.
fn parse_intrinsic_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut prev_backslash = false;
    for c in s.chars() {
        if prev_backslash {
            current.push(c);
            prev_backslash = false;
            continue;
        }
        match c {
            '\\' => {
                current.push(c);
                prev_backslash = true;
            }
            '\'' => {
                in_single = !in_single;
                current.push(c);
            }
            ',' if !in_single => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    out.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
    out
}

/// Resolve a single intrinsic argument:
/// - `'literal string'` → `Value::String("literal string")`
/// - `42`, `3.14`, `true`, `false`, `null` → corresponding scalar
/// - `$.path...` → JSONPath lookup into `source`
fn resolve_intrinsic_arg(raw: &str, source: &Value) -> Option<Value> {
    let trimmed = raw.trim();
    if let Some(s) = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
    {
        return Some(Value::String(s.replace("\\'", "'").replace("\\\\", "\\")));
    }
    if trimmed == "null" {
        return Some(Value::Null);
    }
    if trimmed == "true" {
        return Some(Value::Bool(true));
    }
    if trimmed == "false" {
        return Some(Value::Bool(false));
    }
    if let Ok(n) = trimmed.parse::<i64>() {
        return Some(Value::Number(n.into()));
    }
    if let Ok(f) = trimmed.parse::<f64>()
        && let Some(num) = serde_json::Number::from_f64(f)
    {
        return Some(Value::Number(num));
    }
    if trimmed.starts_with('$') {
        return Some(resolve_reference_path(source, trimmed));
    }
    None
}

fn resolve_intrinsic_arg_str(raw: &str, source: &Value) -> Option<String> {
    let v = resolve_intrinsic_arg(raw, source)?;
    Some(match v {
        Value::String(s) => s,
        other => other.to_string(),
    })
}

/// `States.Format`'s placeholder substitution stringifies values without
/// JSON-escaping — i.e. a string argument lands in the output as its raw
/// content, not surrounded by quotes.
fn intrinsic_arg_to_format_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Retry / Catch helpers
// ---------------------------------------------------------------------------

/// Walk the state's `Retry` array and return the highest MaxAttempts seen
/// (effectively `max(MaxAttempts)` across applicable entries). When no
/// Retry block exists, the cap is 0 — the state runs once.
fn max_retry_attempts(state: &Value) -> u32 {
    let Some(arr) = state.get("Retry").and_then(|v| v.as_array()) else {
        return 0;
    };
    arr.iter()
        .map(|entry| {
            entry
                .get("MaxAttempts")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u32
        })
        .max()
        .unwrap_or(0)
}

/// Returns true when `error` matches any `ErrorEquals` in any Retry entry.
/// The synthetic name `States.ALL` matches every error.
fn retry_matches(state: &Value, error: &str) -> bool {
    let Some(arr) = state.get("Retry").and_then(|v| v.as_array()) else {
        return false;
    };
    arr.iter()
        .filter_map(|entry| entry.get("ErrorEquals").and_then(|v| v.as_array()))
        .flatten()
        .filter_map(|v| v.as_str())
        .any(|e| e == "States.ALL" || e == error)
}

/// Find the first Catch entry whose ErrorEquals includes `error` and
/// return `(Next state name, ResultPath)`. ResultPath defaults to `$` (
/// the error replaces the input entirely) when the catch entry omits it.
fn catch_target(state: &Value, error: &str) -> Option<(String, String)> {
    let arr = state.get("Catch").and_then(|v| v.as_array())?;
    for entry in arr {
        let matches = entry
            .get("ErrorEquals")
            .and_then(|v| v.as_array())
            .map(|errs| {
                errs.iter()
                    .filter_map(|e| e.as_str())
                    .any(|e| e == "States.ALL" || e == error)
            })
            .unwrap_or(false);
        if !matches {
            continue;
        }
        let next = entry.get("Next").and_then(|v| v.as_str())?.to_string();
        let result_path = entry
            .get("ResultPath")
            .and_then(|v| v.as_str())
            .unwrap_or("$")
            .to_string();
        return Some((next, result_path));
    }
    None
}

// ---------------------------------------------------------------------------
// State transition
// ---------------------------------------------------------------------------

enum StateTransition {
    End,
    Next(String),
}

fn transition(state: &Value) -> StateTransition {
    if state["End"].as_bool() == Some(true) {
        StateTransition::End
    } else if let Some(next) = state["Next"].as_str() {
        StateTransition::Next(next.to_string())
    } else {
        StateTransition::End
    }
}

// ---------------------------------------------------------------------------
// Path utilities
// ---------------------------------------------------------------------------

/// Apply InputPath to select a portion of the input.
/// `None` or `"$"` means use the whole input.
fn apply_input_path(input: &Value, path: Option<&str>) -> Value {
    match path {
        None | Some("$") => input.clone(),
        Some(p) => resolve_reference_path(input, p),
    }
}

/// Apply OutputPath to select a portion of the result.
fn apply_output_path(output: &Value, path: Option<&str>) -> Value {
    match path {
        None | Some("$") => output.clone(),
        Some(p) => resolve_reference_path(output, p),
    }
}

/// Apply ResultPath to merge the result into the input.
///
/// - `None` → replace the entire effective input with the result
/// - `"$"` → same as None
/// - `"$.field"` → set `input.field = result`, return merged
/// - `"null"` → discard result, return input unchanged
fn apply_result_path(input: &Value, result: &Value, result_path: Option<&str>) -> Value {
    match result_path {
        None | Some("$") => result.clone(),
        Some("null") => input.clone(),
        Some(path) => {
            // path like "$.field" or "$.a.b"
            let key = path.trim_start_matches("$.").trim_start_matches('$');
            let mut merged = input.clone();
            set_nested_value(&mut merged, key, result.clone());
            merged
        }
    }
}

/// Simple reference path resolver (supports `$.field.subfield` notation).
fn resolve_reference_path(value: &Value, path: &str) -> Value {
    let path = path.trim_start_matches('$').trim_start_matches('.');
    if path.is_empty() {
        return value.clone();
    }
    let mut current = value;
    for segment in path.split('.') {
        current = &current[segment];
    }
    current.clone()
}

/// Set a nested field in a JSON object by dotted path.
fn set_nested_value(target: &mut Value, path: &str, val: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return;
    }

    if parts.len() == 1 {
        if let Value::Object(map) = target {
            map.insert(parts[0].to_string(), val);
        }
        return;
    }

    if let Value::Object(map) = target {
        let head = parts[0];
        let rest = parts[1..].join(".");
        let child = map.entry(head.to_string()).or_insert_with(|| json!({}));
        set_nested_value(child, &rest, val);
    }
}

// ---------------------------------------------------------------------------
// Choice condition evaluation
// ---------------------------------------------------------------------------

/// Coerce a JSON value to a number for Choice numeric comparisons.
/// Numbers pass through; strings that parse as f64 round-trip. Anything
/// else is None so the comparison evaluates to false.
fn coerce_to_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
}

fn evaluate_condition(choice: &Value, input: &Value) -> bool {
    // Handle And / Or / Not compound conditions
    if let Some(and_conditions) = choice["And"].as_array() {
        return and_conditions.iter().all(|c| evaluate_condition(c, input));
    }
    if let Some(or_conditions) = choice["Or"].as_array() {
        return or_conditions.iter().any(|c| evaluate_condition(c, input));
    }
    if let Some(not_condition) = choice.get("Not") {
        return !evaluate_condition(not_condition, input);
    }

    let var_path = match choice["Variable"].as_str() {
        Some(p) => p,
        None => return false,
    };

    let variable_value = resolve_reference_path(input, var_path);

    // StringEquals
    if let Some(expected) = choice["StringEquals"].as_str() {
        return variable_value.as_str() == Some(expected);
    }
    if let Some(expected) = choice["StringEqualsPath"].as_str() {
        let other = resolve_reference_path(input, expected);
        return variable_value.as_str() == other.as_str();
    }

    // StringLessThan / GreaterThan
    if let Some(expected) = choice["StringLessThan"].as_str() {
        return variable_value
            .as_str()
            .map(|v| v < expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["StringGreaterThan"].as_str() {
        return variable_value
            .as_str()
            .map(|v| v > expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["StringLessThanOrEquals"].as_str() {
        return variable_value
            .as_str()
            .map(|v| v <= expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["StringGreaterThanOrEquals"].as_str() {
        return variable_value
            .as_str()
            .map(|v| v >= expected)
            .unwrap_or(false);
    }

    // NumericEquals / LessThan / GreaterThan. AWS Step Functions
    // coerces the variable to a number when it's stored as a string,
    // so "42" matches NumericEquals:42. Non-numeric strings (and
    // booleans / null / objects / arrays) compare as false.
    if let Some(expected) = choice["NumericEquals"].as_f64() {
        return coerce_to_number(&variable_value)
            .map(|v| (v - expected).abs() < f64::EPSILON)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["NumericLessThan"].as_f64() {
        return coerce_to_number(&variable_value)
            .map(|v| v < expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["NumericGreaterThan"].as_f64() {
        return coerce_to_number(&variable_value)
            .map(|v| v > expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["NumericLessThanOrEquals"].as_f64() {
        return coerce_to_number(&variable_value)
            .map(|v| v <= expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["NumericGreaterThanOrEquals"].as_f64() {
        return coerce_to_number(&variable_value)
            .map(|v| v >= expected)
            .unwrap_or(false);
    }

    // BooleanEquals
    if let Some(expected) = choice["BooleanEquals"].as_bool() {
        return variable_value.as_bool() == Some(expected);
    }

    // IsPresent
    if let Some(expected) = choice["IsPresent"].as_bool() {
        let present = !variable_value.is_null();
        return present == expected;
    }

    // IsNull
    if let Some(expected) = choice["IsNull"].as_bool() {
        return variable_value.is_null() == expected;
    }

    // IsNumeric
    if let Some(expected) = choice["IsNumeric"].as_bool() {
        return variable_value.is_number() == expected;
    }

    // IsString
    if let Some(expected) = choice["IsString"].as_bool() {
        return variable_value.is_string() == expected;
    }

    // IsBoolean
    if let Some(expected) = choice["IsBoolean"].as_bool() {
        return variable_value.is_boolean() == expected;
    }

    false
}

// ---------------------------------------------------------------------------
// Public AwsError-returning wrapper (used in executions.rs)
// ---------------------------------------------------------------------------

pub fn run_execution(
    definition: &str,
    input: &str,
    start_time: &str,
    is_express: bool,
) -> Result<ExecResult, AwsError> {
    Ok(execute_typed(definition, input, start_time, is_express))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(def: &str, input: &str) -> ExecResult {
        execute(def, input, "2024-01-01T00:00:00Z")
    }

    fn run_express(def: &str, input: &str) -> ExecResult {
        execute_typed(def, input, "2024-01-01T00:00:00Z", true)
    }

    #[test]
    fn express_workflow_times_out_when_wait_exceeds_5_minutes() {
        let def = r#"{
            "StartAt": "W",
            "States": {
                "W": { "Type": "Wait", "Seconds": 400, "End": true }
            }
        }"#;
        let result = run_express(def, "{}");
        assert_eq!(result.status, "FAILED");
        assert_eq!(result.error.as_deref(), Some("States.Timeout"));
    }

    #[test]
    fn express_workflow_succeeds_when_wait_stays_within_cap() {
        let def = r#"{
            "StartAt": "W",
            "States": {
                "W": { "Type": "Wait", "Seconds": 60, "End": true }
            }
        }"#;
        let result = run_express(def, "{}");
        assert_eq!(result.status, "SUCCEEDED");
    }

    #[test]
    fn choice_numeric_coerces_stringified_number() {
        let def = r#"{
            "StartAt": "C",
            "States": {
                "C": {
                    "Type": "Choice",
                    "Choices": [{
                        "Variable": "$.count",
                        "NumericGreaterThan": 5,
                        "Next": "Hit"
                    }],
                    "Default": "Miss"
                },
                "Hit": { "Type": "Pass", "Result": "hit", "End": true },
                "Miss": { "Type": "Pass", "Result": "miss", "End": true }
            }
        }"#;
        let result = run(def, r#"{"count":"42"}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!("hit"));
    }

    #[test]
    fn choice_numeric_rejects_non_numeric_string() {
        let def = r#"{
            "StartAt": "C",
            "States": {
                "C": {
                    "Type": "Choice",
                    "Choices": [{
                        "Variable": "$.count",
                        "NumericEquals": 0,
                        "Next": "Hit"
                    }],
                    "Default": "Miss"
                },
                "Hit": { "Type": "Pass", "Result": "hit", "End": true },
                "Miss": { "Type": "Pass", "Result": "miss", "End": true }
            }
        }"#;
        let result = run(def, r#"{"count":"hello"}"#);
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!("miss"));
    }

    #[test]
    fn standard_workflow_not_capped_by_express_limit() {
        let def = r#"{
            "StartAt": "W",
            "States": {
                "W": { "Type": "Wait", "Seconds": 1000, "End": true }
            }
        }"#;
        let result = run(def, "{}");
        assert_eq!(result.status, "SUCCEEDED");
    }

    #[test]
    fn parallel_runs_every_branch_and_collects_outputs() {
        // Two branches, each a Pass that emits a distinct constant.
        let def = r#"{
            "StartAt": "Fan",
            "States": {
                "Fan": {
                    "Type": "Parallel",
                    "End": true,
                    "Branches": [
                        {
                            "StartAt": "A",
                            "States": { "A": { "Type": "Pass", "Result": "alpha", "End": true } }
                        },
                        {
                            "StartAt": "B",
                            "States": { "B": { "Type": "Pass", "Result": "beta", "End": true } }
                        }
                    ]
                }
            }
        }"#;
        let result = run(def, r#"{}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!(["alpha", "beta"]));
    }

    #[test]
    fn parallel_branch_failure_fails_state() {
        let def = r#"{
            "StartAt": "Fan",
            "States": {
                "Fan": {
                    "Type": "Parallel",
                    "End": true,
                    "Branches": [
                        {
                            "StartAt": "Boom",
                            "States": {
                                "Boom": { "Type": "Fail", "Error": "Oops", "Cause": "boom" }
                            }
                        }
                    ]
                }
            }
        }"#;
        let result = run(def, r#"{}"#);
        assert_eq!(result.status, "FAILED");
        assert_eq!(result.error.as_deref(), Some("Oops"));
    }

    #[test]
    fn map_runs_every_item() {
        // Iterator just passes the item through.
        let def = r#"{
            "StartAt": "ForEach",
            "States": {
                "ForEach": {
                    "Type": "Map",
                    "End": true,
                    "ItemsPath": "$",
                    "Iterator": {
                        "StartAt": "Echo",
                        "States": { "Echo": { "Type": "Pass", "End": true } }
                    }
                }
            }
        }"#;
        let result = run(def, r#"[1, 2, 3]"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!([1, 2, 3]));
    }

    #[test]
    fn map_with_empty_input_produces_empty_array() {
        let def = r#"{
            "StartAt": "ForEach",
            "States": {
                "ForEach": {
                    "Type": "Map",
                    "End": true,
                    "ItemsPath": "$",
                    "Iterator": {
                        "StartAt": "Echo",
                        "States": { "Echo": { "Type": "Pass", "End": true } }
                    }
                }
            }
        }"#;
        let result = run(def, r#"[]"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!([]));
    }

    #[test]
    fn catch_routes_failure_to_fallback_state() {
        // Fail state's error matches the Catch entry → execution
        // succeeds, ending in the fallback state with the error info
        // attached at $.error.
        let def = r#"{
            "StartAt": "Try",
            "States": {
                "Try": {
                    "Type": "Fail",
                    "Error": "FlakyError",
                    "Cause": "transient",
                    "Catch": [{
                        "ErrorEquals": ["FlakyError"],
                        "Next": "Fallback",
                        "ResultPath": "$.error"
                    }]
                },
                "Fallback": { "Type": "Pass", "End": true }
            }
        }"#;
        let result = run(def, r#"{"hello":"world"}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out["hello"], json!("world"));
        assert_eq!(out["error"]["Error"], json!("FlakyError"));
        assert_eq!(out["error"]["Cause"], json!("transient"));
    }

    #[test]
    fn catch_states_all_matches_any_error() {
        let def = r#"{
            "StartAt": "Try",
            "States": {
                "Try": {
                    "Type": "Fail",
                    "Error": "AnythingGoesHere",
                    "Catch": [{
                        "ErrorEquals": ["States.ALL"],
                        "Next": "Fallback"
                    }]
                },
                "Fallback": { "Type": "Pass", "Result": "caught", "End": true }
            }
        }"#;
        let result = run(def, r#"{}"#);
        assert_eq!(result.status, "SUCCEEDED");
        assert_eq!(result.output.as_deref(), Some("\"caught\""));
    }

    #[test]
    fn unmatched_error_propagates_failure() {
        let def = r#"{
            "StartAt": "Try",
            "States": {
                "Try": {
                    "Type": "Fail",
                    "Error": "Unhandled",
                    "Catch": [{
                        "ErrorEquals": ["DifferentError"],
                        "Next": "Fallback"
                    }]
                },
                "Fallback": { "Type": "Pass", "End": true }
            }
        }"#;
        let result = run(def, r#"{}"#);
        assert_eq!(result.status, "FAILED");
        assert_eq!(result.error.as_deref(), Some("Unhandled"));
    }

    #[test]
    fn retry_then_catch_handles_exhausted_attempts() {
        // Retry is set but the Fail state always fails the same way;
        // after MaxAttempts, Catch picks up the failure.
        let def = r#"{
            "StartAt": "Try",
            "States": {
                "Try": {
                    "Type": "Fail",
                    "Error": "Boom",
                    "Retry": [{ "ErrorEquals": ["Boom"], "MaxAttempts": 2 }],
                    "Catch": [{ "ErrorEquals": ["Boom"], "Next": "End" }]
                },
                "End": { "Type": "Pass", "Result": "recovered", "End": true }
            }
        }"#;
        let result = run(def, r#"{}"#);
        assert_eq!(result.status, "SUCCEEDED");
        assert_eq!(result.output.as_deref(), Some("\"recovered\""));
    }

    #[test]
    fn parameters_resolve_jsonpath_references() {
        let def = r#"{
            "StartAt": "Build",
            "States": {
                "Build": {
                    "Type": "Pass",
                    "End": true,
                    "Parameters": {
                        "name.$": "$.user.name",
                        "static": "literal",
                        "nested": { "deep.$": "$.user.id" }
                    }
                }
            }
        }"#;
        let result = run(def, r#"{"user": {"name": "Ada", "id": 42}}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out["name"], json!("Ada"));
        assert_eq!(out["static"], json!("literal"));
        assert_eq!(out["nested"]["deep"], json!(42));
    }

    #[test]
    fn result_selector_filters_state_output() {
        // Parameters builds a {a, b} object; ResultSelector keeps only
        // `picked`, derived from `a`.
        let def = r#"{
            "StartAt": "Build",
            "States": {
                "Build": {
                    "Type": "Pass",
                    "End": true,
                    "Parameters": { "a": 1, "b": 2 },
                    "ResultSelector": { "picked.$": "$.a" }
                }
            }
        }"#;
        let result = run(def, r#"{}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!({ "picked": 1 }));
    }

    #[test]
    fn result_path_merges_into_post_input_path_input() {
        // ResultPath should merge the (possibly Parameters/ResultSelector
        // transformed) result back into the *raw* input — not the
        // Parameters output.
        let def = r#"{
            "StartAt": "T",
            "States": {
                "T": {
                    "Type": "Pass",
                    "End": true,
                    "Parameters": { "x.$": "$.outer" },
                    "ResultPath": "$.computed"
                }
            }
        }"#;
        let result = run(def, r#"{"outer": "hi", "untouched": "ok"}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        // Original input keys preserved, computed sits beside them.
        assert_eq!(out["outer"], json!("hi"));
        assert_eq!(out["untouched"], json!("ok"));
        assert_eq!(out["computed"]["x"], json!("hi"));
    }

    #[test]
    fn intrinsic_format_substitutes_placeholders() {
        let def = r#"{
            "StartAt": "Build",
            "States": {
                "Build": {
                    "Type": "Pass",
                    "End": true,
                    "Parameters": {
                        "greeting.$": "States.Format('Hello, {}!', $.name)"
                    }
                }
            }
        }"#;
        let result = run(def, r#"{"name": "Ada"}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out["greeting"], json!("Hello, Ada!"));
    }

    #[test]
    fn intrinsic_array_collects_args() {
        let def = r#"{
            "StartAt": "Build",
            "States": {
                "Build": {
                    "Type": "Pass",
                    "End": true,
                    "Parameters": {
                        "items.$": "States.Array($.a, $.b, 'x')"
                    }
                }
            }
        }"#;
        let result = run(def, r#"{"a": 1, "b": 2}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out["items"], json!([1, 2, "x"]));
    }

    #[test]
    fn intrinsic_json_to_string_serializes_value() {
        let def = r#"{
            "StartAt": "Build",
            "States": {
                "Build": {
                    "Type": "Pass",
                    "End": true,
                    "Parameters": {
                        "encoded.$": "States.JsonToString($.payload)"
                    }
                }
            }
        }"#;
        let result = run(def, r#"{"payload": {"k": "v"}}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out["encoded"], json!("{\"k\":\"v\"}"));
    }

    #[test]
    fn intrinsic_string_to_json_parses_string() {
        let def = r#"{
            "StartAt": "Build",
            "States": {
                "Build": {
                    "Type": "Pass",
                    "End": true,
                    "Parameters": {
                        "decoded.$": "States.StringToJson($.raw)"
                    }
                }
            }
        }"#;
        let result = run(def, r#"{"raw": "{\"n\": 42}"}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out["decoded"]["n"], json!(42));
    }

    #[test]
    fn intrinsic_uuid_is_random_per_call() {
        let def = r#"{
            "StartAt": "Build",
            "States": {
                "Build": {
                    "Type": "Pass",
                    "End": true,
                    "Parameters": {
                        "a.$": "States.UUID()",
                        "b.$": "States.UUID()"
                    }
                }
            }
        }"#;
        let result = run(def, r#"{}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        let a = out["a"].as_str().unwrap();
        let b = out["b"].as_str().unwrap();
        assert_eq!(a.len(), 36);
        assert_ne!(a, b);
    }

    #[test]
    fn task_timeout_propagates_to_retry_and_catch() {
        // Retry says "retry once on States.Timeout"; the Task always
        // times out, so after one retry the failure should hit Catch
        // and route to the handler state.
        let def = r#"{
            "StartAt": "MaybeFail",
            "States": {
                "MaybeFail": {
                    "Type": "Task",
                    "Resource": "arn:aws:states:::lambda:invoke",
                    "TimeoutSeconds": 0,
                    "Retry": [
                        { "ErrorEquals": ["States.Timeout"], "MaxAttempts": 2 }
                    ],
                    "Catch": [
                        { "ErrorEquals": ["States.Timeout"], "Next": "Recover" }
                    ],
                    "End": true
                },
                "Recover": { "Type": "Pass", "Result": "recovered", "End": true }
            }
        }"#;
        let result = run(def, "{}");
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!("recovered"));
        let retries = result
            .history
            .iter()
            .filter(|e| e.event_type == "StateRetrying")
            .count();
        assert_eq!(retries, 2, "should retry exactly MaxAttempts times");
    }

    #[test]
    fn task_timeout_without_catch_surfaces_states_timeout() {
        let def = r#"{
            "StartAt": "Slow",
            "States": {
                "Slow": {
                    "Type": "Task",
                    "Resource": "arn:aws:states:::lambda:invoke",
                    "TimeoutSeconds": 0,
                    "End": true
                }
            }
        }"#;
        let result = run(def, "{}");
        assert_eq!(result.status, "FAILED");
        assert_eq!(result.error.as_deref(), Some("States.Timeout"));
    }

    #[test]
    fn task_simulate_timeout_marker_triggers_states_timeout() {
        let def = r#"{
            "StartAt": "MaybeFail",
            "States": {
                "MaybeFail": {
                    "Type": "Task",
                    "Resource": "arn:aws:states:::lambda:invoke",
                    "End": true
                }
            }
        }"#;
        let result = run(def, r#"{"_simulateTimeout": true}"#);
        assert_eq!(result.status, "FAILED");
        assert_eq!(result.error.as_deref(), Some("States.Timeout"));
    }

    #[test]
    fn async_task_suffix_returns_immediate_acknowledgement() {
        let def = r#"{
            "StartAt": "Notify",
            "States": {
                "Notify": {
                    "Type": "Task",
                    "Resource": "arn:aws:states:::sns:publish.async",
                    "End": true
                }
            }
        }"#;
        let result = run(def, r#"{"topic":"x","message":"hi"}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!({ "Status": "Accepted", "StatusCode": 202 }));
        assert!(
            result
                .history
                .iter()
                .any(|e| e.event_type == "TaskSubmitted"),
            "should emit TaskSubmitted for .async integrations"
        );
    }

    #[test]
    fn task_without_suffix_still_echoes_input() {
        let def = r#"{
            "StartAt": "Echo",
            "States": {
                "Echo": {
                    "Type": "Task",
                    "Resource": "arn:aws:states:::lambda:invoke",
                    "End": true
                }
            }
        }"#;
        let result = run(def, r#"{"k":1}"#);
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!({ "k": 1 }));
    }

    #[test]
    fn map_item_selector_reshapes_each_item_with_jsonpath() {
        let def = r#"{
            "StartAt": "ForEach",
            "States": {
                "ForEach": {
                    "Type": "Map",
                    "End": true,
                    "ItemsPath": "$",
                    "ItemSelector": {
                        "id.$": "$.userId",
                        "label": "constant"
                    },
                    "Iterator": {
                        "StartAt": "Echo",
                        "States": { "Echo": { "Type": "Pass", "End": true } }
                    }
                }
            }
        }"#;
        let input = r#"[ { "userId": 1, "drop": true }, { "userId": 2 } ]"#;
        let result = run(def, input);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(
            out,
            json!([
                { "id": 1, "label": "constant" },
                { "id": 2, "label": "constant" }
            ])
        );
    }

    #[test]
    fn map_without_item_selector_passes_raw_item_through() {
        let def = r#"{
            "StartAt": "ForEach",
            "States": {
                "ForEach": {
                    "Type": "Map",
                    "End": true,
                    "ItemsPath": "$",
                    "Iterator": {
                        "StartAt": "Echo",
                        "States": { "Echo": { "Type": "Pass", "End": true } }
                    }
                }
            }
        }"#;
        let result = run(def, r#"[{"a":1}, {"a":2}]"#);
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!([{"a":1}, {"a":2}]));
    }

    #[test]
    fn map_accepts_item_processor_alongside_iterator() {
        // ItemProcessor (newer ASL spelling) should be honored.
        let def = r#"{
            "StartAt": "ForEach",
            "States": {
                "ForEach": {
                    "Type": "Map",
                    "End": true,
                    "ItemsPath": "$",
                    "ItemProcessor": {
                        "StartAt": "Echo",
                        "States": { "Echo": { "Type": "Pass", "End": true } }
                    }
                }
            }
        }"#;
        let result = run(def, r#"["a", "b"]"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!(["a", "b"]));
    }

    fn run_intrinsic(expr: &str, source: Value) -> Value {
        evaluate_intrinsic(expr, &source).expect("intrinsic should evaluate")
    }

    #[test]
    fn intrinsic_format_substitutes_arguments() {
        let v = run_intrinsic(
            "States.Format('hello {} v{}', $.name, $.ver)",
            json!({ "name": "alex", "ver": 2 }),
        );
        assert_eq!(v, json!("hello alex v2"));
    }

    #[test]
    fn intrinsic_string_split_basic_and_empty_separator() {
        assert_eq!(
            run_intrinsic("States.StringSplit('a,b,c', ',')", Value::Null),
            json!(["a", "b", "c"])
        );
        assert_eq!(
            run_intrinsic("States.StringSplit('abc', '')", Value::Null),
            json!(["a", "b", "c"])
        );
    }

    #[test]
    fn intrinsic_array_partition_and_concat() {
        assert_eq!(
            run_intrinsic(
                "States.ArrayPartition($.items, 2)",
                json!({ "items": [1,2,3,4,5] })
            ),
            json!([[1, 2], [3, 4], [5]])
        );
        assert_eq!(
            run_intrinsic(
                "States.ArrayConcat($.a, $.b)",
                json!({ "a": [1], "b": [2, 3] })
            ),
            json!([1, 2, 3])
        );
    }

    #[test]
    fn intrinsic_array_contains_get_length_range_unique() {
        assert_eq!(
            run_intrinsic(
                "States.ArrayContains($.items, 3)",
                json!({ "items": [1, 2, 3] })
            ),
            json!(true)
        );
        assert_eq!(
            run_intrinsic(
                "States.ArrayGetItem($.items, 1)",
                json!({ "items": [10, 20, 30] })
            ),
            json!(20)
        );
        assert_eq!(
            run_intrinsic(
                "States.ArrayLength($.items)",
                json!({ "items": [1, 2, 3, 4] })
            ),
            json!(4)
        );
        assert_eq!(
            run_intrinsic("States.ArrayRange(0, 4, 1)", Value::Null),
            json!([0, 1, 2, 3, 4])
        );
        assert_eq!(
            run_intrinsic(
                "States.ArrayUnique($.items)",
                json!({ "items": [1, 1, 2, 3, 2] })
            ),
            json!([1, 2, 3])
        );
    }

    #[test]
    fn intrinsic_base64_round_trips_and_math() {
        assert_eq!(
            run_intrinsic("States.Base64Encode('hi')", Value::Null),
            json!("aGk=")
        );
        assert_eq!(
            run_intrinsic("States.Base64Decode('aGk=')", Value::Null),
            json!("hi")
        );
        assert_eq!(run_intrinsic("States.MathAdd(2, 5)", Value::Null), json!(7));
    }

    #[test]
    fn intrinsic_hash_sha256_known_value() {
        let v = run_intrinsic("States.Hash('hello', 'SHA-256')", Value::Null);
        assert_eq!(
            v,
            json!("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    #[test]
    fn intrinsic_json_merge_shallow_and_deep() {
        // Shallow: top-level keys overwrite wholesale.
        let v = run_intrinsic(
            "States.JsonMerge($.a, $.b, false)",
            json!({
                "a": { "x": { "p": 1, "q": 2 } },
                "b": { "x": { "r": 3 } }
            }),
        );
        assert_eq!(v, json!({ "x": { "r": 3 } }));

        // Deep: nested objects merge recursively.
        let v = run_intrinsic(
            "States.JsonMerge($.a, $.b, true)",
            json!({
                "a": { "x": { "p": 1, "q": 2 } },
                "b": { "x": { "r": 3 } }
            }),
        );
        assert_eq!(v, json!({ "x": { "p": 1, "q": 2, "r": 3 } }));
    }

    #[test]
    fn intrinsic_is_predicates() {
        assert_eq!(
            run_intrinsic("States.IsBoolean($.v)", json!({ "v": true })),
            json!(true)
        );
        assert_eq!(
            run_intrinsic("States.IsNull($.v)", json!({ "v": null })),
            json!(true)
        );
        assert_eq!(
            run_intrinsic("States.IsNumeric($.v)", json!({ "v": 2.5 })),
            json!(true)
        );
        assert_eq!(
            run_intrinsic("States.IsString($.v)", json!({ "v": "hi" })),
            json!(true)
        );
        assert_eq!(
            run_intrinsic("States.IsPresent($.v)", json!({ "v": null })),
            json!(false)
        );
        assert_eq!(
            run_intrinsic("States.IsPresent($.v)", json!({ "v": 0 })),
            json!(true)
        );
        assert_eq!(
            run_intrinsic("States.IsTimestamp('2024-01-15T00:00:00Z')", Value::Null),
            json!(true)
        );
        assert_eq!(
            run_intrinsic("States.IsTimestamp('not-a-date')", Value::Null),
            json!(false)
        );
    }

    #[test]
    fn intrinsic_uuid_returns_v4_shaped_string() {
        let v = run_intrinsic("States.UUID()", Value::Null);
        let s = v.as_str().unwrap();
        assert_eq!(s.len(), 36);
        assert!(s.chars().filter(|c| *c == '-').count() == 4);
    }

    #[test]
    fn intrinsic_math_random_within_bounds() {
        for _ in 0..50 {
            let v = run_intrinsic("States.MathRandom(10, 20)", Value::Null);
            let n = v.as_i64().unwrap();
            assert!((10..20).contains(&n));
        }
    }
}
