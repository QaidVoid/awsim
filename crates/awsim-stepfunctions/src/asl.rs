//! Basic Amazon States Language (ASL) interpreter.
//!
//! Supports: Pass, Succeed, Fail, Wait, Task, Choice, Parallel, Map.
//! InputPath / OutputPath / ResultPath transformations are supported.

use std::cell::RefCell;
use std::sync::Arc;

use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::HistoryEvent;

/// (reader, account, region) for the Distributed Map S3 reader context.
type S3ReaderCtx = (Arc<dyn awsim_core::S3ObjectReader>, String, String);

thread_local! {
    /// Per-thread S3 reader used by Distributed Map `ItemReader`. The
    /// interpreter is fully synchronous, so a context set immediately
    /// before `run_execution` stays valid for the whole run on this
    /// thread (no await intervenes).
    static S3_CTX: RefCell<Option<S3ReaderCtx>> = const { RefCell::new(None) };
}

/// Install the S3 reader context for Distributed Map. Pass `None` to run
/// without S3 access.
pub fn set_s3_context(
    reader: Option<Arc<dyn awsim_core::S3ObjectReader>>,
    account: &str,
    region: &str,
) {
    S3_CTX.with(|c| {
        *c.borrow_mut() = reader.map(|r| (r, account.to_string(), region.to_string()));
    });
}

/// Clear the S3 reader context after a run completes.
pub fn clear_s3_context() {
    S3_CTX.with(|c| *c.borrow_mut() = None);
}

fn read_item_reader_object(bucket: &str, key: &str) -> Result<Vec<u8>, StateFailed> {
    S3_CTX.with(|c| match c.borrow().as_ref() {
        Some((reader, account, region)) => {
            reader
                .get_object(bucket, key, account, region)
                .map_err(|e| StateFailed {
                    error: "States.ItemReaderFailed".to_string(),
                    cause: e.message,
                })
        }
        None => Err(StateFailed {
            error: "States.ItemReaderFailed".to_string(),
            cause: "no S3 reader is configured for ItemReader".to_string(),
        }),
    })
}

/// Result of executing an ASL state machine.
pub struct ExecResult {
    pub status: String, // SUCCEEDED, FAILED, or WAITING
    pub output: Option<String>,
    pub error: Option<String>,
    pub cause: Option<String>,
    pub history: Vec<HistoryEvent>,
    /// Populated only when `status == "WAITING"` (a `.waitForTaskToken`
    /// Task suspended the execution). `waiting_next` is the state to
    /// resume from once the token is answered; `None` means the waiting
    /// Task was the terminal state.
    pub waiting_token: Option<String>,
    pub waiting_state: Option<String>,
    pub waiting_next: Option<String>,
    pub waiting_input: Option<String>,
    pub waiting_result_path: Option<String>,
}

/// Sentinel error used to unwind the recursive interpreter when a
/// `.waitForTaskToken` Task suspends. It bypasses Retry/Catch and is
/// recognized by `execute_typed_with_context` as a WAITING outcome.
const WAIT_SENTINEL: &str = "__awsim.WaitForTaskToken";

impl ExecResult {
    fn failed(
        error: impl Into<String>,
        cause: impl Into<String>,
        history: Vec<HistoryEvent>,
    ) -> Self {
        Self {
            status: "FAILED".to_string(),
            output: None,
            error: Some(error.into()),
            cause: Some(cause.into()),
            history,
            waiting_token: None,
            waiting_state: None,
            waiting_next: None,
            waiting_input: None,
            waiting_result_path: None,
        }
    }

    fn succeeded(output: String, history: Vec<HistoryEvent>) -> Self {
        Self {
            status: "SUCCEEDED".to_string(),
            output: Some(output),
            error: None,
            cause: None,
            history,
            waiting_token: None,
            waiting_state: None,
            waiting_next: None,
            waiting_input: None,
            waiting_result_path: None,
        }
    }
}

/// A `.waitForTaskToken` Task that suspended the current run.
struct PendingWait {
    token: String,
    state_name: String,
    next: Option<String>,
    input: Value,
    result_path: Option<String>,
}

/// Like [`execute_typed`] but seeds the AWS States context object so
/// child scopes (Map iterations, Parallel branches) can read identifiers
/// like `$$.Map.Item.Index` from their Parameters / ItemSelector blocks.
pub fn execute_with_context(
    definition: &str,
    input: &str,
    start_time: &str,
    context: Value,
) -> ExecResult {
    execute_typed_with_context(definition, input, start_time, false, context)
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
    execute_typed_with_context(definition, input, start_time, is_express, Value::Null)
}

fn execute_typed_with_context(
    definition: &str,
    input: &str,
    start_time: &str,
    is_express: bool,
    context: Value,
) -> ExecResult {
    let def: Value = match serde_json::from_str(definition) {
        Ok(v) => v,
        Err(e) => {
            return ExecResult::failed("InvalidDefinition", e.to_string(), Vec::new());
        }
    };

    let input_val: Value = serde_json::from_str(input).unwrap_or(Value::Null);

    let mut ctx = new_context(def["States"].clone(), start_time, is_express, context);

    let start_at = match def["StartAt"].as_str() {
        Some(s) => s.to_string(),
        None => {
            return ExecResult::failed("InvalidDefinition", "Missing StartAt", ctx.history);
        }
    };

    ctx.push_event("ExecutionStarted", json!({ "input": input }));

    let run = ctx.run_state(&start_at, input_val);
    ctx.into_result(run)
}

/// Construct a fresh interpreter context.
fn new_context(
    states: Value,
    start_time: &str,
    is_express: bool,
    context: Value,
) -> InterpreterContext {
    InterpreterContext {
        states,
        history: Vec::new(),
        event_counter: 0,
        start_time: start_time.to_string(),
        is_express,
        simulated_wait_secs: 0,
        context_object: context,
        pending_wait: None,
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
    /// AWS States context object accessible via `$$.<path>` in
    /// Parameters / ResultSelector / ItemSelector references. Seeded
    /// by Map iterations (`Map.Item.Index`, `Map.Item.Value`) and
    /// Parallel branches (`Execution.BranchName`).
    context_object: Value,
    /// Set when a top-level `.waitForTaskToken` Task suspends the run.
    pending_wait: Option<PendingWait>,
}

impl InterpreterContext {
    /// Convert a top-level `run_state` outcome into an [`ExecResult`],
    /// surfacing a WAITING result when a `.waitForTaskToken` Task
    /// suspended the execution.
    fn into_result(mut self, run: Result<Value, StateFailed>) -> ExecResult {
        match run {
            Ok(output) => {
                let output_str = output.to_string();
                self.push_event("ExecutionSucceeded", json!({ "output": output_str }));
                ExecResult::succeeded(output_str, self.history)
            }
            Err(failure) => {
                if let Some(wait) = self.pending_wait.take() {
                    return ExecResult {
                        status: "WAITING".to_string(),
                        output: None,
                        error: None,
                        cause: None,
                        history: self.history,
                        waiting_token: Some(wait.token),
                        waiting_state: Some(wait.state_name),
                        waiting_next: wait.next,
                        waiting_input: Some(wait.input.to_string()),
                        waiting_result_path: wait.result_path,
                    };
                }
                self.push_event(
                    "ExecutionFailed",
                    json!({ "error": failure.error, "cause": failure.cause }),
                );
                ExecResult::failed(failure.error, failure.cause, self.history)
            }
        }
    }

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
            Some(p) => apply_parameters_with_ctx(p, &after_input_path, &self.context_object),
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
                    if err.error == WAIT_SENTINEL {
                        // A waitForTaskToken suspension unwinds straight up,
                        // skipping Retry and Catch.
                        break Err(err);
                    }
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
                        Some(rs) => {
                            apply_parameters_with_ctx(rs, &raw_output, &self.context_object)
                        }
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
                // A waitForTaskToken suspension is not a real failure; let
                // it propagate to the top-level WAITING handler untouched.
                if err.error == WAIT_SENTINEL {
                    return Err(err);
                }
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

        // `.waitForTaskToken` suspends the execution: register a token and
        // unwind via the WAIT sentinel. The run is resumed from this
        // Task's Next by SendTaskSuccess / SendTaskFailure. Only a single
        // top-level token is modeled (not inside Map/Parallel branches).
        if matches!(integration, TaskIntegration::WaitForTaskToken) {
            let token = uuid::Uuid::new_v4().to_string();
            self.push_event(
                "TaskScheduled",
                json!({ "name": state_name, "resource": resource, "taskToken": token }),
            );
            let next = match transition(state) {
                StateTransition::Next(n) => Some(n),
                StateTransition::End => None,
            };
            self.pending_wait = Some(PendingWait {
                token: token.clone(),
                state_name: state_name.to_string(),
                next,
                input: input.clone(),
                result_path: state
                    .get("ResultPath")
                    .and_then(Value::as_str)
                    .map(String::from),
            });
            return Err(StateFailed {
                error: WAIT_SENTINEL.to_string(),
                cause: token,
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
        for (i, branch) in branches.iter().enumerate() {
            let branch_def = branch.to_string();
            let mut branch_context = self.context_object.clone();
            merge_context_into(
                &mut branch_context,
                "Execution",
                json!({ "BranchName": format!("Branch-{i}") }),
            );
            let branch_result = execute_with_context(
                &branch_def,
                &input.to_string(),
                &self.start_time,
                branch_context,
            );
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
        // Distributed Map ItemReader (S3 CSV) takes precedence over the
        // in-memory ItemsPath. Only CSV input is supported.
        let item_array: Vec<Value> = if let Some(reader) =
            state.get("ItemReader").filter(|r| !r.is_null())
        {
            let params = reader
                .get("Parameters")
                .map(|p| apply_parameters_with_ctx(p, &input, &self.context_object))
                .unwrap_or(Value::Null);
            let bucket = params.get("Bucket").and_then(Value::as_str).unwrap_or("");
            let key = params.get("Key").and_then(Value::as_str).unwrap_or("");
            let reader_config = reader.get("ReaderConfig");
            let input_type = reader_config
                .and_then(|c| c.get("InputType"))
                .and_then(Value::as_str)
                .unwrap_or("CSV");
            if input_type != "CSV" {
                return Err(StateFailed {
                    error: "States.ItemReaderFailed".to_string(),
                    cause: format!("ItemReader InputType '{input_type}' is unsupported (CSV only)"),
                });
            }
            let bytes = read_item_reader_object(bucket, key)?;
            csv_to_items(&bytes, reader_config)
        } else {
            let items_path = state["ItemsPath"].as_str().unwrap_or("$");
            let items = resolve_reference_path(&input, items_path);
            items
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![items.clone()])
        };

        // ItemProcessor (newer ASL) supersedes Iterator (legacy) but the
        // payload shape is identical; honor either.
        let iterator_def = if state.get("ItemProcessor").is_some() {
            state["ItemProcessor"].clone()
        } else {
            state["Iterator"].clone()
        };

        // ItemSelector (Map 2.0) reshapes each item into the payload the
        // iterator receives. AWS evaluates ItemSelector keys ending in
        // `.$` against the raw item, mirroring Parameters. Absent
        // selector falls through to the bare item.
        let item_selector = state.get("ItemSelector").cloned();
        let iter_def_str = iterator_def.to_string();

        // ItemBatcher groups the (selector-applied) items into batches; the
        // iterator receives `{ ...BatchInput, "Items": [...] }`. AWS order
        // is ItemReader -> ItemSelector -> ItemBatcher.
        if let Some(batcher) = state.get("ItemBatcher").filter(|b| !b.is_null()) {
            let selected: Vec<Value> = item_array
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let mut iter_context = self.context_object.clone();
                    merge_context_into(
                        &mut iter_context,
                        "Map",
                        json!({ "Item": { "Index": i, "Value": item } }),
                    );
                    match &item_selector {
                        Some(sel) if !sel.is_null() => {
                            apply_parameters_with_ctx(sel, item, &iter_context)
                        }
                        _ => item.clone(),
                    }
                })
                .collect();
            let batches = batch_items(selected, batcher, &input, &self.context_object);
            let mut outputs: Vec<Value> = Vec::with_capacity(batches.len());
            for batch in &batches {
                let item_result = execute_with_context(
                    &iter_def_str,
                    &batch.to_string(),
                    &self.start_time,
                    self.context_object.clone(),
                );
                if item_result.status == "FAILED" {
                    return Err(StateFailed {
                        error: item_result.error.unwrap_or_default(),
                        cause: item_result.cause.unwrap_or_default(),
                    });
                }
                let out = item_result.output.unwrap_or_else(|| "null".to_string());
                outputs.push(serde_json::from_str(&out).unwrap_or(Value::Null));
            }
            return Ok((Value::Array(outputs), transition(state)));
        }

        let mut outputs: Vec<Value> = Vec::with_capacity(item_array.len());
        for (i, item) in item_array.iter().enumerate() {
            let mut iter_context = self.context_object.clone();
            merge_context_into(
                &mut iter_context,
                "Map",
                json!({ "Item": { "Index": i, "Value": item } }),
            );
            let effective = match &item_selector {
                Some(sel) if !sel.is_null() => apply_parameters_with_ctx(sel, item, &iter_context),
                _ => item.clone(),
            };
            let item_result = execute_with_context(
                &iter_def_str,
                &effective.to_string(),
                &self.start_time,
                iter_context,
            );
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

fn merge_context_into(context: &mut Value, key: &str, value: Value) {
    if !context.is_object() {
        *context = json!({});
    }
    if let Value::Object(map) = context {
        map.insert(key.to_string(), value);
    }
}

/// Minimal RFC-4180 CSV parser: handles quoted fields with embedded
/// commas / newlines and doubled-quote escaping. Returns rows of fields.
fn parse_csv(bytes: &[u8]) -> Vec<Vec<String>> {
    let text = String::from_utf8_lossy(bytes);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut field = String::new();
    let mut row: Vec<String> = Vec::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else {
            match ch {
                '"' => in_quotes = true,
                ',' => row.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(ch),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    // Drop fully-empty trailing rows (e.g. a blank final line).
    rows.retain(|r| !(r.len() == 1 && r[0].is_empty()));
    rows
}

fn csv_row_to_object(headers: &[String], row: &[String]) -> Value {
    let mut obj = serde_json::Map::new();
    for (i, h) in headers.iter().enumerate() {
        obj.insert(
            h.clone(),
            Value::String(row.get(i).cloned().unwrap_or_default()),
        );
    }
    Value::Object(obj)
}

/// Turn CSV bytes into Map items per `ReaderConfig.CSVHeaderLocation`:
/// `FIRST_ROW` (default) uses the first row as headers; `GIVEN` uses
/// `CSVHeaders`.
fn csv_to_items(bytes: &[u8], reader_config: Option<&Value>) -> Vec<Value> {
    let rows = parse_csv(bytes);
    if rows.is_empty() {
        return Vec::new();
    }
    let header_loc = reader_config
        .and_then(|c| c.get("CSVHeaderLocation"))
        .and_then(Value::as_str)
        .unwrap_or("FIRST_ROW");
    match header_loc {
        "GIVEN" => {
            let headers: Vec<String> = reader_config
                .and_then(|c| c.get("CSVHeaders"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            rows.iter()
                .map(|r| csv_row_to_object(&headers, r))
                .collect()
        }
        _ => {
            let headers = rows[0].clone();
            rows[1..]
                .iter()
                .map(|r| csv_row_to_object(&headers, r))
                .collect()
        }
    }
}

/// Group selected items into `ItemBatcher` batches. Each batch is
/// `{ ...BatchInput, "Items": [...] }`; `MaxItemsPerBatch` bounds the
/// chunk size.
fn batch_items(items: Vec<Value>, batcher: &Value, input: &Value, context: &Value) -> Vec<Value> {
    let max = batcher
        .get("MaxItemsPerBatch")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let chunk = if max == 0 { items.len().max(1) } else { max };
    let batch_input = batcher
        .get("BatchInput")
        .map(|bi| apply_parameters_with_ctx(bi, input, context))
        .unwrap_or(Value::Null);
    items
        .chunks(chunk)
        .map(|c| {
            let mut obj = serde_json::Map::new();
            if let Value::Object(bi) = &batch_input {
                for (k, v) in bi {
                    obj.insert(k.clone(), v.clone());
                }
            }
            obj.insert("Items".to_string(), Value::Array(c.to_vec()));
            Value::Object(obj)
        })
        .collect()
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
/// Transform a `Parameters` / `ResultSelector` / `ItemSelector`
/// template against a source object and the AWS States context. The
/// context object mirrors AWS's runtime context — `{ "Map": { "Item":
/// { "Index": 0, "Value": ... } } }` inside a Map iteration or
/// `{ "Execution": { "BranchName": "Branch-0" } }` inside a Parallel
/// branch.
fn apply_parameters_with_ctx(template: &Value, source: &Value, context: &Value) -> Value {
    match template {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if let Some(stripped_key) = k.strip_suffix(".$") {
                    let resolved = match v.as_str() {
                        Some(s) if s.starts_with("States.") => {
                            evaluate_intrinsic(s, source).unwrap_or_else(|| v.clone())
                        }
                        Some(path) if path.starts_with("$$") => {
                            resolve_reference_path(context, &path[1..])
                        }
                        Some(path) if path.starts_with('$') => resolve_reference_path(source, path),
                        _ => v.clone(),
                    };
                    out.insert(stripped_key.to_string(), resolved);
                } else {
                    out.insert(k.clone(), apply_parameters_with_ctx(v, source, context));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| apply_parameters_with_ctx(v, source, context))
                .collect(),
        ),
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
            // Loose ISO-8601 with optional offset: YYYY-MM-DDTHH:MM:SS(.fff)?(Z|±HH:MM).
            // Compiled once and reused; recompiling on every call was the
            // only per-invocation regex build in the interpreter.
            static TS_REGEX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
            let re = TS_REGEX.get_or_init(|| {
                regex::Regex::new(
                    r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:?\d{2})?$",
                )
                .expect("IsTimestamp regex is valid")
            });
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
/// Bounded cache of parsed reference-path segments. AWS state machines
/// reuse the same JSONPath strings across every state and every Map item,
/// so caching the split avoids re-parsing on each resolve. The hard
/// capacity plus insertion-order eviction keep a long-running process
/// with many distinct paths from growing the cache without bound.
struct PathSegmentCache {
    map: std::collections::HashMap<String, std::sync::Arc<Vec<String>>>,
    order: std::collections::VecDeque<String>,
    cap: usize,
}

impl PathSegmentCache {
    fn get_or_parse(&mut self, path: &str) -> std::sync::Arc<Vec<String>> {
        if let Some(v) = self.map.get(path) {
            return std::sync::Arc::clone(v);
        }
        let segs: Vec<String> = path
            .trim_start_matches('$')
            .trim_start_matches('.')
            .split('.')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let arc = std::sync::Arc::new(segs);
        if self.order.len() >= self.cap
            && let Some(old) = self.order.pop_front()
        {
            self.map.remove(&old);
        }
        self.map
            .insert(path.to_string(), std::sync::Arc::clone(&arc));
        self.order.push_back(path.to_string());
        arc
    }
}

static PATH_CACHE: std::sync::OnceLock<std::sync::Mutex<PathSegmentCache>> =
    std::sync::OnceLock::new();

/// Cache capacity for parsed reference-path segments.
const PATH_CACHE_CAP: usize = 256;

fn cached_segments(path: &str) -> std::sync::Arc<Vec<String>> {
    PATH_CACHE
        .get_or_init(|| {
            std::sync::Mutex::new(PathSegmentCache {
                map: std::collections::HashMap::new(),
                order: std::collections::VecDeque::new(),
                cap: PATH_CACHE_CAP,
            })
        })
        .lock()
        .unwrap()
        .get_or_parse(path)
}

fn resolve_reference_path(value: &Value, path: &str) -> Value {
    let segments = cached_segments(path);
    if segments.is_empty() {
        return value.clone();
    }
    let mut current = value;
    for segment in segments.iter() {
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

/// Resume a `.waitForTaskToken` execution after SendTaskSuccess. The
/// `output` becomes the waiting Task's result (merged into the pre-wait
/// input via its ResultPath); the run then continues from `next_state`,
/// or completes if the Task was terminal. A further waitForTaskToken in
/// the tail yields another WAITING result.
#[allow(clippy::too_many_arguments)]
pub fn resume_execution_success(
    definition: &str,
    start_time: &str,
    is_express: bool,
    next_state: Option<&str>,
    input_at_wait: &str,
    result_path: Option<&str>,
    output: Value,
) -> ExecResult {
    let def: Value = match serde_json::from_str(definition) {
        Ok(v) => v,
        Err(e) => return ExecResult::failed("InvalidDefinition", e.to_string(), Vec::new()),
    };
    let input_val: Value = serde_json::from_str(input_at_wait).unwrap_or(Value::Null);
    let merged = apply_result_path(&input_val, &output, result_path);
    let mut ctx = new_context(def["States"].clone(), start_time, is_express, Value::Null);
    ctx.push_event("TaskSucceeded", json!({ "output": output }));
    match next_state {
        None => {
            let out = merged.to_string();
            ctx.push_event("ExecutionSucceeded", json!({ "output": out }));
            ExecResult::succeeded(out, ctx.history)
        }
        Some(ns) => {
            let run = ctx.run_state(ns, merged);
            ctx.into_result(run)
        }
    }
}

/// Resume a `.waitForTaskToken` execution after SendTaskFailure. The
/// failure routes through the waiting Task's Catch when one matches;
/// otherwise the execution ends FAILED with the supplied error/cause.
pub fn resume_execution_failure(
    definition: &str,
    start_time: &str,
    is_express: bool,
    waiting_state_name: &str,
    input_at_wait: &str,
    error: &str,
    cause: &str,
) -> ExecResult {
    let def: Value = match serde_json::from_str(definition) {
        Ok(v) => v,
        Err(e) => return ExecResult::failed("InvalidDefinition", e.to_string(), Vec::new()),
    };
    let input_val: Value = serde_json::from_str(input_at_wait).unwrap_or(Value::Null);
    let state = def["States"][waiting_state_name].clone();
    let mut ctx = new_context(def["States"].clone(), start_time, is_express, Value::Null);
    ctx.push_event("TaskFailed", json!({ "error": error, "cause": cause }));
    if let Some((next_state, result_path)) = catch_target(&state, error) {
        let error_payload = json!({ "Error": error, "Cause": cause });
        let merged = apply_result_path(&input_val, &error_payload, Some(&result_path));
        ctx.push_event("StateCaught", json!({ "next": next_state, "error": error }));
        let run = ctx.run_state(&next_state, merged);
        ctx.into_result(run)
    } else {
        ctx.push_event("ExecutionFailed", json!({ "error": error, "cause": cause }));
        ExecResult::failed(error, cause, ctx.history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(def: &str, input: &str) -> ExecResult {
        execute_typed(def, input, "2024-01-01T00:00:00Z", false)
    }

    #[test]
    fn path_cache_parses_segments_and_resolves() {
        let v = json!({ "a": { "b": 7 } });
        assert_eq!(resolve_reference_path(&v, "$.a.b"), json!(7));
        assert_eq!(resolve_reference_path(&v, "$"), v);
        // Repeated lookups return the same cached segments.
        let s1 = cached_segments("$.a.b");
        let s2 = cached_segments("$.a.b");
        assert_eq!(*s1, vec!["a".to_string(), "b".to_string()]);
        assert!(std::sync::Arc::ptr_eq(&s1, &s2));
    }

    #[test]
    fn path_cache_evicts_beyond_capacity() {
        let mut cache = PathSegmentCache {
            map: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
            cap: 3,
        };
        for i in 0..10 {
            cache.get_or_parse(&format!("$.p{i}"));
            assert!(cache.map.len() <= 3, "cache exceeded capacity");
            assert!(cache.order.len() <= 3);
        }
        // The earliest entries were evicted; the most recent remain.
        assert!(cache.map.contains_key("$.p9"));
        assert!(!cache.map.contains_key("$.p0"));
    }

    #[test]
    fn parse_csv_handles_quotes_and_embedded_commas() {
        let rows = parse_csv(b"a,b\n1,\"x,y\"\n2,\"he said \"\"hi\"\"\"\n");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], vec!["1".to_string(), "x,y".to_string()]);
        assert_eq!(rows[2], vec!["2".to_string(), "he said \"hi\"".to_string()]);
    }

    #[test]
    fn distributed_map_reads_csv_first_row() {
        struct CsvReader;
        impl awsim_core::S3ObjectReader for CsvReader {
            fn get_object(
                &self,
                _b: &str,
                _k: &str,
                _a: &str,
                _r: &str,
            ) -> Result<Vec<u8>, awsim_core::AwsError> {
                Ok(b"id,name\n1,alice\n2,bob\n".to_vec())
            }
        }
        set_s3_context(Some(Arc::new(CsvReader)), "000000000000", "us-east-1");
        let def = r#"{
            "StartAt": "M",
            "States": {
                "M": { "Type": "Map", "End": true,
                    "ItemReader": {
                        "Resource": "arn:aws:states:::s3:getObject",
                        "ReaderConfig": { "InputType": "CSV", "CSVHeaderLocation": "FIRST_ROW" },
                        "Parameters": { "Bucket": "b", "Key": "k.csv" }
                    },
                    "ItemProcessor": { "StartAt": "P", "States": { "P": { "Type": "Pass", "End": true } } }
                }
            }
        }"#;
        let result = run(def, "{}");
        clear_s3_context();
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(result.output.as_ref().unwrap()).unwrap();
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "1");
        assert_eq!(arr[0]["name"], "alice");
        assert_eq!(arr[1]["name"], "bob");
    }

    #[test]
    fn distributed_map_item_batcher_groups_with_batch_input() {
        let def = r#"{
            "StartAt": "M",
            "States": {
                "M": { "Type": "Map", "End": true,
                    "ItemsPath": "$.items",
                    "ItemBatcher": { "MaxItemsPerBatch": 2, "BatchInput": { "factor": 10 } },
                    "ItemProcessor": { "StartAt": "P", "States": { "P": { "Type": "Pass", "End": true } } }
                }
            }
        }"#;
        let result = run(def, r#"{"items":[1,2,3,4,5]}"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(result.output.as_ref().unwrap()).unwrap();
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["Items"], json!([1, 2]));
        assert_eq!(arr[0]["factor"], 10);
        assert_eq!(arr[2]["Items"], json!([5]));
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
    fn map_item_selector_can_read_map_item_index_from_context() {
        // ItemSelector uses $$.Map.Item.Index to attach a position to
        // each output. Bare $ still references the raw item.
        let def = r#"{
            "StartAt": "ForEach",
            "States": {
                "ForEach": {
                    "Type": "Map",
                    "End": true,
                    "ItemsPath": "$",
                    "ItemSelector": {
                        "idx.$": "$$.Map.Item.Index",
                        "value.$": "$"
                    },
                    "Iterator": {
                        "StartAt": "Echo",
                        "States": { "Echo": { "Type": "Pass", "End": true } }
                    }
                }
            }
        }"#;
        let result = run(def, r#"["a", "b", "c"]"#);
        assert_eq!(result.status, "SUCCEEDED");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(
            out,
            json!([
                { "idx": 0, "value": "a" },
                { "idx": 1, "value": "b" },
                { "idx": 2, "value": "c" }
            ])
        );
    }

    #[test]
    fn map_iterator_state_can_read_context_via_parameters() {
        // The Iterator's own Pass state references $$.Map.Item.Index via
        // Parameters, proving the context propagates into the child
        // sub-execution.
        let def = r#"{
            "StartAt": "ForEach",
            "States": {
                "ForEach": {
                    "Type": "Map",
                    "End": true,
                    "ItemsPath": "$",
                    "Iterator": {
                        "StartAt": "Tag",
                        "States": {
                            "Tag": {
                                "Type": "Pass",
                                "Parameters": { "idx.$": "$$.Map.Item.Index" },
                                "End": true
                            }
                        }
                    }
                }
            }
        }"#;
        let result = run(def, r#"["x", "y"]"#);
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out, json!([{ "idx": 0 }, { "idx": 1 }]));
    }

    #[test]
    fn parallel_branch_state_can_read_execution_branch_name() {
        let def = r#"{
            "StartAt": "Fan",
            "States": {
                "Fan": {
                    "Type": "Parallel",
                    "End": true,
                    "Branches": [
                        {
                            "StartAt": "A",
                            "States": {
                                "A": {
                                    "Type": "Pass",
                                    "Parameters": { "branch.$": "$$.Execution.BranchName" },
                                    "End": true
                                }
                            }
                        },
                        {
                            "StartAt": "B",
                            "States": {
                                "B": {
                                    "Type": "Pass",
                                    "Parameters": { "branch.$": "$$.Execution.BranchName" },
                                    "End": true
                                }
                            }
                        }
                    ]
                }
            }
        }"#;
        let result = run(def, "{}");
        let out: Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(
            out,
            json!([{ "branch": "Branch-0" }, { "branch": "Branch-1" }])
        );
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
