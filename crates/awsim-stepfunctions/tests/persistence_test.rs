//! Step Functions state machines and in-flight executions must survive a
//! restart. A suspended `.waitForTaskToken` execution is the case that
//! matters most: without its pending token it can never be answered.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_stepfunctions::StepFunctionsService;
use serde_json::json;

fn ctx() -> RequestContext {
    RequestContext::new("states", "us-east-1")
}

const PASS_DEFINITION: &str = r#"{"StartAt":"Done","States":{"Done":{"Type":"Pass","End":true}}}"#;

async fn create_machine(svc: &StepFunctionsService, name: &str) -> String {
    let out = svc
        .handle(
            "CreateStateMachine",
            json!({
                "name": name,
                "definition": PASS_DEFINITION,
                "roleArn": "arn:aws:iam::000000000000:role/sfn",
            }),
            &ctx(),
        )
        .await
        .expect("CreateStateMachine");
    out["stateMachineArn"]
        .as_str()
        .expect("stateMachineArn")
        .to_string()
}

fn round_trip(svc: &StepFunctionsService) -> StepFunctionsService {
    let bytes = svc.snapshot().expect("snapshot");
    let restored = StepFunctionsService::new();
    restored.restore(&bytes).expect("restore");
    restored
}

#[tokio::test]
async fn state_machine_survives_round_trip() {
    let svc = StepFunctionsService::new();
    let arn = create_machine(&svc, "sm1").await;

    let restored = round_trip(&svc);
    let described = restored
        .handle(
            "DescribeStateMachine",
            json!({ "stateMachineArn": arn }),
            &ctx(),
        )
        .await
        .expect("state machine should exist after restore");
    assert_eq!(described["name"], "sm1");
}

#[tokio::test]
async fn list_state_machines_is_populated_after_restore() {
    let svc = StepFunctionsService::new();
    for name in ["a", "b"] {
        create_machine(&svc, name).await;
    }

    let restored = round_trip(&svc);
    let listed = restored
        .handle("ListStateMachines", json!({}), &ctx())
        .await
        .expect("ListStateMachines");
    let count = listed["stateMachines"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(count, 2, "expected both machines back, got {listed:?}");
}

#[tokio::test]
async fn executions_survive_round_trip() {
    let svc = StepFunctionsService::new();
    let arn = create_machine(&svc, "exec-sm").await;
    svc.handle(
        "StartExecution",
        json!({ "stateMachineArn": arn, "input": "{}" }),
        &ctx(),
    )
    .await
    .expect("StartExecution");

    let restored = round_trip(&svc);
    let listed = restored
        .handle("ListExecutions", json!({ "stateMachineArn": arn }), &ctx())
        .await
        .expect("ListExecutions");
    let count = listed["executions"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(count, 1, "execution lost across restore: {listed:?}");
}

#[tokio::test]
async fn restore_is_implemented() {
    let svc = StepFunctionsService::new();
    let bytes = svc.snapshot().expect("snapshot");
    assert!(svc.restore(&bytes).is_ok(), "restore should be implemented");
}
