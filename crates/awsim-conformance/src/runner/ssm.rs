use crate::chk;
use crate::runner::common::*;
use aws_sdk_ssm::types::ParameterType;

pub async fn test_ssm(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_ssm::Client::new(&config);
    let mut results = Vec::new();

    // PutParameter
    results.push(chk!(
        "PutParameter",
        client
            .put_parameter()
            .name("/conformance/param")
            .value("test-value")
            .r#type(ParameterType::String)
            .send()
            .await,
        verbose
    ));

    // GetParameter
    results.push(chk!(
        "GetParameter",
        client
            .get_parameter()
            .name("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // GetParameters
    results.push(chk!(
        "GetParameters",
        client
            .get_parameters()
            .names("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // DescribeParameters
    results.push(chk!(
        "DescribeParameters",
        client.describe_parameters().send().await,
        verbose
    ));

    // PutParameter (second one for path-based tests)
    let _ = client
        .put_parameter()
        .name("/conformance/param2")
        .value("value2")
        .r#type(ParameterType::String)
        .send()
        .await;

    // GetParametersByPath
    results.push(chk!(
        "GetParametersByPath",
        client
            .get_parameters_by_path()
            .path("/conformance")
            .send()
            .await,
        verbose
    ));

    // GetParameterHistory
    results.push(chk!(
        "GetParameterHistory",
        client
            .get_parameter_history()
            .name("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // AddTagsToResource (SSM)
    results.push(chk!(
        "AddTagsToResource",
        client
            .add_tags_to_resource()
            .resource_type(aws_sdk_ssm::types::ResourceTypeForTagging::Parameter)
            .resource_id("/conformance/param")
            .tags(
                aws_sdk_ssm::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource (SSM)
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_type(aws_sdk_ssm::types::ResourceTypeForTagging::Parameter)
            .resource_id("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // RemoveTagsFromResource (SSM)
    results.push(chk!(
        "RemoveTagsFromResource",
        client
            .remove_tags_from_resource()
            .resource_type(aws_sdk_ssm::types::ResourceTypeForTagging::Parameter)
            .resource_id("/conformance/param")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // LabelParameterVersion
    results.push(chk!(
        "LabelParameterVersion",
        client
            .label_parameter_version()
            .name("/conformance/param")
            .labels("conformance-label")
            .send()
            .await,
        verbose
    ));

    // SendCommand
    let send_cmd_r = client
        .send_command()
        .document_name("AWS-RunShellScript")
        .instance_ids("i-0000000000000000")
        .parameters("commands", vec!["echo hello".to_string()])
        .send()
        .await;
    let command_id = send_cmd_r
        .as_ref()
        .ok()
        .and_then(|r| r.command.as_ref())
        .and_then(|c| c.command_id.clone());
    results.push(chk!("SendCommand", send_cmd_r, verbose));

    // ListCommands
    results.push(chk!(
        "ListCommands",
        client.list_commands().send().await,
        verbose
    ));

    // GetCommandInvocation (expect service error — command on non-existent instance)
    if let Some(ref cid) = command_id {
        results.push(chk!(
            "GetCommandInvocation",
            client
                .get_command_invocation()
                .command_id(cid)
                .instance_id("i-0000000000000000")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetCommandInvocation".to_string()));
    }

    // PutInventory
    results.push(chk!(
        "PutInventory",
        client
            .put_inventory()
            .instance_id("i-0000000000000000")
            .items(
                aws_sdk_ssm::types::InventoryItem::builder()
                    .type_name("AWS:Application")
                    .schema_version("1.1")
                    .capture_time("2024-01-01T00:00:00Z")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetInventory
    results.push(chk!(
        "GetInventory",
        client.get_inventory().send().await,
        verbose
    ));

    // CreateDocument
    let create_doc_r = client
        .create_document()
        .name("ConformanceDocument")
        .content(r#"{"schemaVersion":"2.2","description":"Conformance test doc","mainSteps":[]}"#)
        .document_type(aws_sdk_ssm::types::DocumentType::Command)
        .send()
        .await;
    results.push(chk!("CreateDocument", create_doc_r, verbose));

    // GetDocument
    results.push(chk!(
        "GetDocument",
        client
            .get_document()
            .name("ConformanceDocument")
            .send()
            .await,
        verbose
    ));

    // DescribeDocument
    results.push(chk!(
        "DescribeDocument",
        client
            .describe_document()
            .name("ConformanceDocument")
            .send()
            .await,
        verbose
    ));

    // ListDocuments
    results.push(chk!(
        "ListDocuments",
        client.list_documents().send().await,
        verbose
    ));

    // CreateAssociation
    let create_assoc_r = client
        .create_association()
        .name("ConformanceDocument")
        .instance_id("i-0000000000000000")
        .send()
        .await;
    let association_id = create_assoc_r
        .as_ref()
        .ok()
        .and_then(|r| r.association_description.as_ref())
        .and_then(|d| d.association_id.clone());
    results.push(chk!("CreateAssociation", create_assoc_r, verbose));

    // DescribeAssociation
    if let Some(ref aid) = association_id {
        results.push(chk!(
            "DescribeAssociation",
            client
                .describe_association()
                .association_id(aid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeAssociation".to_string()));
    }

    // ListAssociations
    results.push(chk!(
        "ListAssociations",
        client.list_associations().send().await,
        verbose
    ));

    // DeleteAssociation
    if let Some(ref aid) = association_id {
        results.push(chk!(
            "DeleteAssociation",
            client.delete_association().association_id(aid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteAssociation".to_string()));
    }

    // CreateMaintenanceWindow
    let create_mw_r = client
        .create_maintenance_window()
        .name("ConformanceMW")
        .schedule("cron(0 0 * * ? *)")
        .duration(1)
        .cutoff(0)
        .allow_unassociated_targets(false)
        .send()
        .await;
    let window_id = create_mw_r.as_ref().ok().and_then(|r| r.window_id.clone());
    results.push(chk!("CreateMaintenanceWindow", create_mw_r, verbose));

    // DescribeMaintenanceWindows
    results.push(chk!(
        "DescribeMaintenanceWindows",
        client.describe_maintenance_windows().send().await,
        verbose
    ));

    // DeleteMaintenanceWindow
    if let Some(ref wid) = window_id {
        results.push(chk!(
            "DeleteMaintenanceWindow",
            client
                .delete_maintenance_window()
                .window_id(wid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteMaintenanceWindow".to_string()));
    }

    // CreateOpsItem
    let create_ops_r = client
        .create_ops_item()
        .title("Conformance OpsItem")
        .description("Created by conformance test")
        .source("conformance")
        .send()
        .await;
    let ops_item_id = create_ops_r
        .as_ref()
        .ok()
        .and_then(|r| r.ops_item_id.clone());
    results.push(chk!("CreateOpsItem", create_ops_r, verbose));

    // GetOpsItem
    if let Some(ref oid) = ops_item_id {
        results.push(chk!(
            "GetOpsItem",
            client.get_ops_item().ops_item_id(oid).send().await,
            verbose
        ));

        // UpdateOpsItem
        results.push(chk!(
            "UpdateOpsItem",
            client
                .update_ops_item()
                .ops_item_id(oid)
                .description("Updated by conformance test")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetOpsItem".to_string()));
        results.push(OpResult::Skipped("UpdateOpsItem".to_string()));
    }

    // DescribeOpsItems
    results.push(chk!(
        "DescribeOpsItems",
        client.describe_ops_items().send().await,
        verbose
    ));

    // CreatePatchBaseline
    let create_pb_r = client
        .create_patch_baseline()
        .name("ConformancePatchBaseline")
        .operating_system(aws_sdk_ssm::types::OperatingSystem::Windows)
        .description("Conformance test patch baseline")
        .send()
        .await;
    let baseline_id = create_pb_r
        .as_ref()
        .ok()
        .and_then(|r| r.baseline_id.clone());
    results.push(chk!("CreatePatchBaseline", create_pb_r, verbose));

    // DescribePatchBaselines
    results.push(chk!(
        "DescribePatchBaselines",
        client.describe_patch_baselines().send().await,
        verbose
    ));

    // GetPatchBaseline
    if let Some(ref bid) = baseline_id {
        results.push(chk!(
            "GetPatchBaseline",
            client.get_patch_baseline().baseline_id(bid).send().await,
            verbose
        ));

        // DeletePatchBaseline
        results.push(chk!(
            "DeletePatchBaseline",
            client.delete_patch_baseline().baseline_id(bid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetPatchBaseline".to_string()));
        results.push(OpResult::Skipped("DeletePatchBaseline".to_string()));
    }

    // StartAutomationExecution
    let start_auto_r = client
        .start_automation_execution()
        .document_name("AWS-RunShellScript")
        .send()
        .await;
    let auto_exec_id = start_auto_r
        .as_ref()
        .ok()
        .and_then(|r| r.automation_execution_id.clone());
    results.push(chk!("StartAutomationExecution", start_auto_r, verbose));

    // GetAutomationExecution
    if let Some(ref aid) = auto_exec_id {
        results.push(chk!(
            "GetAutomationExecution",
            client
                .get_automation_execution()
                .automation_execution_id(aid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetAutomationExecution".to_string()));
    }

    // DescribeAutomationExecutions
    results.push(chk!(
        "DescribeAutomationExecutions",
        client.describe_automation_executions().send().await,
        verbose
    ));

    // StartSession
    let start_sess_r = client
        .start_session()
        .target("i-0000000000000000")
        .send()
        .await;
    let session_id = start_sess_r
        .as_ref()
        .ok()
        .and_then(|r| r.session_id.clone());
    results.push(chk!("StartSession", start_sess_r, verbose));

    // DescribeSessions
    results.push(chk!(
        "DescribeSessions",
        client
            .describe_sessions()
            .state(aws_sdk_ssm::types::SessionState::Active)
            .send()
            .await,
        verbose
    ));

    // TerminateSession
    if let Some(ref sid) = session_id {
        results.push(chk!(
            "TerminateSession",
            client.terminate_session().session_id(sid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("TerminateSession".to_string()));
    }

    // DeleteDocument (cleanup)
    results.push(chk!(
        "DeleteDocument",
        client
            .delete_document()
            .name("ConformanceDocument")
            .send()
            .await,
        verbose
    ));

    // DeleteParameters (batch delete)
    results.push(chk!(
        "DeleteParameters",
        client
            .delete_parameters()
            .names("/conformance/param")
            .names("/conformance/param2")
            .send()
            .await,
        verbose
    ));

    // DeleteParameter (may already be deleted by DeleteParameters — will get service error = pass)
    results.push(chk!(
        "DeleteParameter",
        client
            .delete_parameter()
            .name("/conformance/param")
            .send()
            .await,
        verbose
    ));

    // CreateActivation / DescribeActivations / DeleteActivation
    let act_r = client
        .create_activation()
        .iam_role("SSMServiceRole")
        .description("conformance activation")
        .registration_limit(5)
        .send()
        .await;
    let activation_id = act_r.as_ref().ok().and_then(|r| r.activation_id.clone());
    results.push(chk!("CreateActivation", act_r, verbose));

    results.push(chk!(
        "DescribeActivations",
        client.describe_activations().send().await,
        verbose
    ));

    if let Some(ref aid) = activation_id {
        results.push(chk!(
            "DeleteActivation",
            client.delete_activation().activation_id(aid).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteActivation".to_string()));
    }

    // PutComplianceItems / ListComplianceItems
    results.push(chk!(
        "PutComplianceItems",
        client
            .put_compliance_items()
            .resource_id("i-conformance")
            .resource_type("ManagedInstance")
            .compliance_type("Custom:Conformance")
            .execution_summary(
                aws_sdk_ssm::types::ComplianceExecutionSummary::builder()
                    .execution_time(aws_sdk_ssm::primitives::DateTime::from_secs(0))
                    .build()
                    .unwrap(),
            )
            .items(
                aws_sdk_ssm::types::ComplianceItemEntry::builder()
                    .id("Conformance:1")
                    .title("Conformance Check")
                    .severity(aws_sdk_ssm::types::ComplianceSeverity::Informational)
                    .status(aws_sdk_ssm::types::ComplianceStatus::Compliant)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListComplianceItems",
        client.list_compliance_items().send().await,
        verbose
    ));

    // CreateOpsMetadata / GetOpsMetadata / UpdateOpsMetadata / ListOpsMetadata / DeleteOpsMetadata
    let opsm_r = client
        .create_ops_metadata()
        .resource_id("/conformance/ops-meta")
        .send()
        .await;
    let opsm_arn = opsm_r
        .as_ref()
        .ok()
        .and_then(|r| r.ops_metadata_arn.clone());
    results.push(chk!("CreateOpsMetadata", opsm_r, verbose));

    if let Some(ref arn) = opsm_arn {
        results.push(chk!(
            "GetOpsMetadata",
            client.get_ops_metadata().ops_metadata_arn(arn).send().await,
            verbose
        ));

        results.push(chk!(
            "UpdateOpsMetadata",
            client
                .update_ops_metadata()
                .ops_metadata_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("GetOpsMetadata".to_string()));
        results.push(OpResult::Skipped("UpdateOpsMetadata".to_string()));
    }

    results.push(chk!(
        "ListOpsMetadata",
        client.list_ops_metadata().send().await,
        verbose
    ));

    if let Some(ref arn) = opsm_arn {
        results.push(chk!(
            "DeleteOpsMetadata",
            client
                .delete_ops_metadata()
                .ops_metadata_arn(arn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteOpsMetadata".to_string()));
    }

    // PutResourcePolicy / GetResourcePolicies / DeleteResourcePolicy
    let res_arn = "arn:aws:ssm:us-east-1:000000000000:parameter/conformance";
    let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Sid":"x","Effect":"Allow","Principal":"*","Action":"ssm:GetParameter","Resource":"*"}]}"#;
    let prp_r = client
        .put_resource_policy()
        .resource_arn(res_arn)
        .policy(policy_doc)
        .send()
        .await;
    let resource_policy_id = prp_r.as_ref().ok().and_then(|r| r.policy_id.clone());
    results.push(chk!("PutResourcePolicy", prp_r, verbose));

    results.push(chk!(
        "GetResourcePolicies",
        client
            .get_resource_policies()
            .resource_arn(res_arn)
            .send()
            .await,
        verbose
    ));

    if let Some(ref pid) = resource_policy_id {
        results.push(chk!(
            "DeleteResourcePolicy",
            client
                .delete_resource_policy()
                .resource_arn(res_arn)
                .policy_id(pid)
                .policy_hash("dummyhash")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteResourcePolicy".to_string()));
    }

    // CreateResourceDataSync / ListResourceDataSync
    results.push(chk!(
        "CreateResourceDataSync",
        client
            .create_resource_data_sync()
            .sync_name("conformance-sync")
            .s3_destination(
                aws_sdk_ssm::types::ResourceDataSyncS3Destination::builder()
                    .bucket_name("conformance-bucket")
                    .region("us-east-1")
                    .sync_format(aws_sdk_ssm::types::ResourceDataSyncS3Format::JsonSerde)
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "ListResourceDataSync",
        client.list_resource_data_sync().send().await,
        verbose
    ));

    let _ = client
        .delete_resource_data_sync()
        .sync_name("conformance-sync")
        .send()
        .await;

    // DescribeInstanceInformation / DescribeInstanceProperties
    results.push(chk!(
        "DescribeInstanceInformation",
        client.describe_instance_information().send().await,
        verbose
    ));

    results.push(chk!(
        "DescribeInstanceProperties",
        client.describe_instance_properties().send().await,
        verbose
    ));

    // GetInventorySchema
    results.push(chk!(
        "GetInventorySchema",
        client.get_inventory_schema().send().await,
        verbose
    ));

    // ListResourceComplianceSummaries
    results.push(chk!(
        "ListResourceComplianceSummaries",
        client.list_resource_compliance_summaries().send().await,
        verbose
    ));

    // ListComplianceSummaries
    results.push(chk!(
        "ListComplianceSummaries",
        client.list_compliance_summaries().send().await,
        verbose
    ));

    results
}
