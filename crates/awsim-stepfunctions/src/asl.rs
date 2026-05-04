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

struct InterpreterContext {
    states: Value,
    history: Vec<HistoryEvent>,
    event_counter: u64,
    start_time: String,
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

        let effective_input = apply_input_path(&input, state["InputPath"].as_str());

        let result = match state_type {
            "Pass" => self.exec_pass(&state, effective_input),
            "Succeed" => self.exec_succeed(&state, effective_input),
            "Fail" => self.exec_fail(&state),
            "Wait" => self.exec_wait(&state, effective_input),
            "Task" => self.exec_task(state_name, &state, effective_input),
            "Choice" => self.exec_choice(&state, effective_input),
            "Parallel" => self.exec_parallel(&state, effective_input),
            "Map" => self.exec_map(&state, effective_input),
            other => Err(StateFailed {
                error: "UnsupportedStateType".to_string(),
                cause: format!("State type '{other}' is not supported"),
            }),
        };

        match result {
            Ok((output, next)) => {
                let final_output = apply_output_path(&output, state["OutputPath"].as_str());

                self.push_event(
                    "StateExited",
                    json!({ "name": state_name, "output": final_output }),
                );

                match next {
                    StateTransition::End => Ok(final_output),
                    StateTransition::Next(next_state) => self.run_state(&next_state, final_output),
                }
            }
            Err(e) => Err(e),
        }
    }

    fn exec_pass(
        &mut self,
        state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        let result = if let Some(result_val) = state.get("Result") {
            result_val.clone()
        } else {
            input.clone()
        };

        let output = apply_result_path(&input, &result, state["ResultPath"].as_str());
        Ok((output, transition(state)))
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
        // In dev emulator, Wait just proceeds immediately
        Ok((input, transition(state)))
    }

    fn exec_task(
        &mut self,
        state_name: &str,
        state: &Value,
        input: Value,
    ) -> Result<(Value, StateTransition), StateFailed> {
        let resource = state["Resource"].as_str().unwrap_or("unknown");

        self.push_event(
            "TaskStateEntered",
            json!({
                "name": state_name,
                "resource": resource,
                "input": input,
            }),
        );

        // Mock output: echo input back (no actual Lambda invocation)
        let mock_output = input.clone();

        self.push_event(
            "TaskSucceeded",
            json!({
                "name": state_name,
                "resource": resource,
                "output": mock_output,
            }),
        );

        let output = apply_result_path(&input, &mock_output, state["ResultPath"].as_str());
        Ok((output, transition(state)))
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
        let parallel_output = Value::Array(outputs);
        let result_output =
            apply_result_path(&input, &parallel_output, state["ResultPath"].as_str());
        Ok((result_output, transition(state)))
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

        let iter_def_str = iterator_def.to_string();
        let mut outputs: Vec<Value> = Vec::with_capacity(item_array.len());
        for item in &item_array {
            let item_result = execute(&iter_def_str, &item.to_string(), &self.start_time);
            if item_result.status == "FAILED" {
                return Err(StateFailed {
                    error: item_result.error.unwrap_or_default(),
                    cause: item_result.cause.unwrap_or_default(),
                });
            }
            let item_output_str = item_result.output.unwrap_or_else(|| "null".to_string());
            outputs.push(serde_json::from_str(&item_output_str).unwrap_or(Value::Null));
        }
        let map_output = Value::Array(outputs);
        let result_output = apply_result_path(&input, &map_output, state["ResultPath"].as_str());
        Ok((result_output, transition(state)))
    }
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

    // NumericEquals / LessThan / GreaterThan
    if let Some(expected) = choice["NumericEquals"].as_f64() {
        return variable_value.as_f64() == Some(expected);
    }
    if let Some(expected) = choice["NumericLessThan"].as_f64() {
        return variable_value
            .as_f64()
            .map(|v| v < expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["NumericGreaterThan"].as_f64() {
        return variable_value
            .as_f64()
            .map(|v| v > expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["NumericLessThanOrEquals"].as_f64() {
        return variable_value
            .as_f64()
            .map(|v| v <= expected)
            .unwrap_or(false);
    }
    if let Some(expected) = choice["NumericGreaterThanOrEquals"].as_f64() {
        return variable_value
            .as_f64()
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
) -> Result<ExecResult, AwsError> {
    Ok(execute(definition, input, start_time))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(def: &str, input: &str) -> ExecResult {
        execute(def, input, "2024-01-01T00:00:00Z")
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
}
