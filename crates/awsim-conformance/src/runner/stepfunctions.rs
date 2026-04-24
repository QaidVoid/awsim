use crate::chk;
use crate::runner::common::*;

pub async fn test_stepfunctions(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_sfn::Client::new(&config);
    let mut results = Vec::new();

    let asl = r#"{"Comment":"Conformance test","StartAt":"Pass","States":{"Pass":{"Type":"Pass","End":true}}}"#;

    // CreateStateMachine
    let sm_r = client
        .create_state_machine()
        .name("conformance-sm")
        .definition(asl)
        .role_arn("arn:aws:iam::000000000000:role/conformance-role")
        .send()
        .await;
    let sm_arn = sm_r
        .as_ref()
        .ok()
        .map(|r| r.state_machine_arn.clone())
        .unwrap_or_else(|| {
            "arn:aws:states:us-east-1:000000000000:stateMachine:conformance-sm".to_string()
        });
    results.push(chk!("CreateStateMachine", sm_r, verbose));

    // ListStateMachines
    results.push(chk!(
        "ListStateMachines",
        client.list_state_machines().send().await,
        verbose
    ));

    // DescribeStateMachine
    results.push(chk!(
        "DescribeStateMachine",
        client
            .describe_state_machine()
            .state_machine_arn(&sm_arn)
            .send()
            .await,
        verbose
    ));

    // StartExecution
    let exec_r = client
        .start_execution()
        .state_machine_arn(&sm_arn)
        .name("conformance-exec")
        .input(r#"{"key":"value"}"#)
        .send()
        .await;
    let exec_arn = exec_r.as_ref().ok().map(|r| r.execution_arn.clone());
    results.push(chk!("StartExecution", exec_r, verbose));

    // ListExecutions
    results.push(chk!(
        "ListExecutions",
        client
            .list_executions()
            .state_machine_arn(&sm_arn)
            .send()
            .await,
        verbose
    ));

    if let Some(ref earn) = exec_arn {
        // DescribeExecution
        results.push(chk!(
            "DescribeExecution",
            client.describe_execution().execution_arn(earn).send().await,
            verbose
        ));

        // GetExecutionHistory
        results.push(chk!(
            "GetExecutionHistory",
            client
                .get_execution_history()
                .execution_arn(earn)
                .send()
                .await,
            verbose
        ));

        // StopExecution
        results.push(chk!(
            "StopExecution",
            client.stop_execution().execution_arn(earn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeExecution".to_string()));
        results.push(OpResult::Skipped("GetExecutionHistory".to_string()));
        results.push(OpResult::Skipped("StopExecution".to_string()));
    }

    // CreateActivity
    let act_r = client
        .create_activity()
        .name("conformance-activity")
        .send()
        .await;
    let act_arn = act_r.as_ref().ok().map(|r| r.activity_arn.clone());
    results.push(chk!("CreateActivity", act_r, verbose));

    // ListActivities
    results.push(chk!(
        "ListActivities",
        client.list_activities().send().await,
        verbose
    ));

    if let Some(ref aarn) = act_arn {
        // DescribeActivity
        results.push(chk!(
            "DescribeActivity",
            client.describe_activity().activity_arn(aarn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeActivity".to_string()));
    }

    // TagResource (SFN)
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(&sm_arn)
            .tags(
                aws_sdk_sfn::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (SFN)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(&sm_arn)
            .send()
            .await,
        verbose
    ));

    // UntagResource (SFN)
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(&sm_arn)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // DeleteActivity
    if let Some(ref aarn) = act_arn {
        results.push(chk!(
            "DeleteActivity",
            client.delete_activity().activity_arn(aarn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteActivity".to_string()));
    }

    // DeleteStateMachine
    results.push(chk!(
        "DeleteStateMachine",
        client
            .delete_state_machine()
            .state_machine_arn(&sm_arn)
            .send()
            .await,
        verbose
    ));

    results
}
