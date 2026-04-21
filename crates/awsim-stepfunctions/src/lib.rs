mod asl;
mod handler;
mod operations;
mod state;

pub use handler::StepFunctionsService;

#[cfg(test)]
mod tests {
    use awsim_core::RequestContext;
    use serde_json::json;

    use super::handler::StepFunctionsService;
    use awsim_core::ServiceHandler;

    fn ctx() -> RequestContext {
        RequestContext::new("states", "us-east-1")
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    // Simple state machine definitions for tests
    fn pass_definition() -> String {
        json!({
            "StartAt": "Start",
            "States": {
                "Start": {
                    "Type": "Pass",
                    "End": true
                }
            }
        })
        .to_string()
    }

    fn succeed_definition() -> String {
        json!({
            "StartAt": "Ok",
            "States": {
                "Ok": {
                    "Type": "Succeed"
                }
            }
        })
        .to_string()
    }

    fn fail_definition() -> String {
        json!({
            "StartAt": "Boom",
            "States": {
                "Boom": {
                    "Type": "Fail",
                    "Error": "MyError",
                    "Cause": "test failure"
                }
            }
        })
        .to_string()
    }

    fn choice_definition() -> String {
        json!({
            "StartAt": "Route",
            "States": {
                "Route": {
                    "Type": "Choice",
                    "Choices": [
                        {
                            "Variable": "$.value",
                            "NumericGreaterThan": 10,
                            "Next": "High"
                        },
                        {
                            "Variable": "$.value",
                            "NumericLessThanOrEquals": 10,
                            "Next": "Low"
                        }
                    ]
                },
                "High": { "Type": "Succeed" },
                "Low": { "Type": "Succeed" }
            }
        })
        .to_string()
    }

    // -----------------------------------------------------------------------
    // State machine CRUD
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_and_describe_state_machine() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({
                "name": "my-machine",
                "definition": pass_definition(),
                "roleArn": "arn:aws:iam::000000000000:role/StepRole",
            }),
            &ctx,
        ))
        .unwrap();

        let arn = create["stateMachineArn"].as_str().unwrap();
        assert!(arn.contains("my-machine"), "arn={arn}");

        let desc = block_on(svc.handle(
            "DescribeStateMachine",
            json!({ "stateMachineArn": arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["name"].as_str().unwrap(), "my-machine");
        assert_eq!(desc["status"].as_str().unwrap(), "ACTIVE");
    }

    #[test]
    fn test_create_state_machine_duplicate() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "dup", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap();

        let err = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "dup", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "StateMachineAlreadyExists");
    }

    #[test]
    fn test_create_state_machine_invalid_definition() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "bad", "definition": "not json" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidDefinition");
    }

    #[test]
    fn test_list_state_machines() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "m1", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "m2", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap();

        let list =
            block_on(svc.handle("ListStateMachines", json!({}), &ctx)).unwrap();
        assert_eq!(list["stateMachines"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_delete_state_machine() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "del-me", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap();
        let arn = create["stateMachineArn"].as_str().unwrap();

        block_on(svc.handle(
            "DeleteStateMachine",
            json!({ "stateMachineArn": arn }),
            &ctx,
        ))
        .unwrap();

        let list = block_on(svc.handle("ListStateMachines", json!({}), &ctx)).unwrap();
        assert_eq!(list["stateMachines"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_update_state_machine() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "upd", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap();
        let arn = create["stateMachineArn"].as_str().unwrap();

        block_on(svc.handle(
            "UpdateStateMachine",
            json!({
                "stateMachineArn": arn,
                "definition": succeed_definition(),
            }),
            &ctx,
        ))
        .unwrap();

        let desc = block_on(svc.handle(
            "DescribeStateMachine",
            json!({ "stateMachineArn": arn }),
            &ctx,
        ))
        .unwrap();
        assert!(desc["definition"].as_str().unwrap().contains("Succeed"));
    }

    // -----------------------------------------------------------------------
    // Executions
    // -----------------------------------------------------------------------

    #[test]
    fn test_start_execution_pass_succeeds() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "pass-sm", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap();
        let sm_arn = create["stateMachineArn"].as_str().unwrap();

        let exec = block_on(svc.handle(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": r#"{"hello": "world"}"# }),
            &ctx,
        ))
        .unwrap();
        let exec_arn = exec["executionArn"].as_str().unwrap();
        assert!(exec_arn.contains("pass-sm"), "exec_arn={exec_arn}");

        let desc = block_on(svc.handle(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["status"].as_str().unwrap(), "SUCCEEDED");
    }

    #[test]
    fn test_start_execution_fail_state() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "fail-sm", "definition": fail_definition() }),
            &ctx,
        ))
        .unwrap();
        let sm_arn = create["stateMachineArn"].as_str().unwrap();

        let exec = block_on(svc.handle(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": "{}" }),
            &ctx,
        ))
        .unwrap();
        let exec_arn = exec["executionArn"].as_str().unwrap();

        let desc = block_on(svc.handle(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["status"].as_str().unwrap(), "FAILED");
        assert_eq!(desc["error"].as_str().unwrap(), "MyError");
    }

    #[test]
    fn test_choice_state_high_branch() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "choice-sm", "definition": choice_definition() }),
            &ctx,
        ))
        .unwrap();
        let sm_arn = create["stateMachineArn"].as_str().unwrap();

        let exec = block_on(svc.handle(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": r#"{"value": 20}"# }),
            &ctx,
        ))
        .unwrap();
        let exec_arn = exec["executionArn"].as_str().unwrap();

        let desc = block_on(svc.handle(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["status"].as_str().unwrap(), "SUCCEEDED");
    }

    #[test]
    fn test_choice_state_low_branch() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "choice-low", "definition": choice_definition() }),
            &ctx,
        ))
        .unwrap();
        let sm_arn = create["stateMachineArn"].as_str().unwrap();

        let exec = block_on(svc.handle(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": r#"{"value": 5}"# }),
            &ctx,
        ))
        .unwrap();
        let exec_arn = exec["executionArn"].as_str().unwrap();

        let desc = block_on(svc.handle(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["status"].as_str().unwrap(), "SUCCEEDED");
    }

    #[test]
    fn test_list_executions() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "list-exec-sm", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap();
        let sm_arn = create["stateMachineArn"].as_str().unwrap();

        for i in 0..3 {
            block_on(svc.handle(
                "StartExecution",
                json!({ "stateMachineArn": sm_arn, "name": format!("exec-{i}"), "input": "{}" }),
                &ctx,
            ))
            .unwrap();
        }

        let list = block_on(svc.handle(
            "ListExecutions",
            json!({ "stateMachineArn": sm_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(list["executions"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_get_execution_history() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "hist-sm", "definition": pass_definition() }),
            &ctx,
        ))
        .unwrap();
        let sm_arn = create["stateMachineArn"].as_str().unwrap();

        let exec = block_on(svc.handle(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": "{}" }),
            &ctx,
        ))
        .unwrap();
        let exec_arn = exec["executionArn"].as_str().unwrap();

        let history = block_on(svc.handle(
            "GetExecutionHistory",
            json!({ "executionArn": exec_arn }),
            &ctx,
        ))
        .unwrap();

        let events = history["events"].as_array().unwrap();
        // At minimum: ExecutionStarted, StateEntered, StateExited, ExecutionSucceeded
        assert!(events.len() >= 4, "events.len()={}", events.len());
        assert_eq!(events[0]["type"].as_str().unwrap(), "ExecutionStarted");
    }

    #[test]
    fn test_start_execution_nonexistent_sm() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle(
            "StartExecution",
            json!({ "stateMachineArn": "arn:aws:states:us-east-1:000000000000:stateMachine:ghost" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "StateMachineDoesNotExist");
    }

    #[test]
    fn test_unknown_operation() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("FooBarBaz", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    // -----------------------------------------------------------------------
    // ASL: Wait state (proceeds immediately in dev)
    // -----------------------------------------------------------------------

    #[test]
    fn test_wait_state_proceeds() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let definition = json!({
            "StartAt": "W",
            "States": {
                "W": {
                    "Type": "Wait",
                    "Seconds": 60,
                    "Next": "Done"
                },
                "Done": { "Type": "Succeed" }
            }
        })
        .to_string();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "wait-sm", "definition": definition }),
            &ctx,
        ))
        .unwrap();
        let sm_arn = create["stateMachineArn"].as_str().unwrap();

        let exec = block_on(svc.handle(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": "{}" }),
            &ctx,
        ))
        .unwrap();
        let exec_arn = exec["executionArn"].as_str().unwrap();

        let desc = block_on(svc.handle(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["status"].as_str().unwrap(), "SUCCEEDED");
    }

    // -----------------------------------------------------------------------
    // ASL: ResultPath merging
    // -----------------------------------------------------------------------

    #[test]
    fn test_pass_result_path() {
        let svc = StepFunctionsService::new();
        let ctx = ctx();

        let definition = json!({
            "StartAt": "Enrich",
            "States": {
                "Enrich": {
                    "Type": "Pass",
                    "Result": { "added": true },
                    "ResultPath": "$.enrichment",
                    "End": true
                }
            }
        })
        .to_string();

        let create = block_on(svc.handle(
            "CreateStateMachine",
            json!({ "name": "rp-sm", "definition": definition }),
            &ctx,
        ))
        .unwrap();
        let sm_arn = create["stateMachineArn"].as_str().unwrap();

        let exec = block_on(svc.handle(
            "StartExecution",
            json!({ "stateMachineArn": sm_arn, "input": r#"{"original": 1}"# }),
            &ctx,
        ))
        .unwrap();
        let exec_arn = exec["executionArn"].as_str().unwrap();

        let desc = block_on(svc.handle(
            "DescribeExecution",
            json!({ "executionArn": exec_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(desc["status"].as_str().unwrap(), "SUCCEEDED");

        let output: serde_json::Value =
            serde_json::from_str(desc["output"].as_str().unwrap()).unwrap();
        assert_eq!(output["enrichment"]["added"], true);
        assert_eq!(output["original"], 1);
    }
}
