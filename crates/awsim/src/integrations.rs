/// Cross-service integration handlers invoked by the background event router.
///
/// When CloudFormation creates or deletes a stack, it emits one
/// `cloudformation:CreateResource` / `cloudformation:DeleteResource` event per
/// resource.  The functions in this module receive those events and forward them
/// to the appropriate service handler so that the resources actually exist in
/// the target service (S3, SQS, SNS, etc.).
use std::collections::HashMap;
use std::sync::Arc;

use awsim_core::{InternalEvent, RequestContext, ServiceHandler};
use tracing::{debug, info, warn};

/// Handle a `cloudformation:CreateResource` event by calling the appropriate
/// service's Create operation.
pub async fn handle_cf_create_resource(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let resource_type = match event.detail["resourceType"].as_str() {
        Some(t) => t,
        None => {
            warn!("cloudformation:CreateResource event missing resourceType");
            return;
        }
    };
    let properties = &event.detail["properties"];

    let ctx = RequestContext {
        account_id: event.account_id.clone(),
        region: event.region.clone(),
        service: "cloudformation".to_string(),
        access_key: None,
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        uri: "/".to_string(),
        event_bus: None,
    };

    match resource_type {
        "AWS::S3::Bucket" => {
            if let Some(s3) = services.get("s3") {
                let bucket_name = properties["BucketName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("cf-bucket-{}", &uuid::Uuid::new_v4().to_string()[..8]));
                let input = serde_json::json!({ "Bucket": bucket_name });
                match s3.handle("CreateBucket", input, &ctx).await {
                    Ok(_) => info!(bucket = %bucket_name, "CloudFormation created S3 bucket"),
                    Err(e) => warn!(bucket = %bucket_name, error = %e.message, "CloudFormation S3 bucket creation failed"),
                }
            }
        }
        "AWS::SQS::Queue" => {
            if let Some(sqs) = services.get("sqs") {
                let queue_name = properties["QueueName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("cf-queue-{}", &uuid::Uuid::new_v4().to_string()[..8]));
                let input = serde_json::json!({ "QueueName": queue_name });
                match sqs.handle("CreateQueue", input, &ctx).await {
                    Ok(_) => info!(queue = %queue_name, "CloudFormation created SQS queue"),
                    Err(e) => warn!(queue = %queue_name, error = %e.message, "CloudFormation SQS queue creation failed"),
                }
            }
        }
        "AWS::SNS::Topic" => {
            if let Some(sns) = services.get("sns") {
                let topic_name = properties["TopicName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("cf-topic-{}", &uuid::Uuid::new_v4().to_string()[..8]));
                let input = serde_json::json!({ "Name": topic_name });
                match sns.handle("CreateTopic", input, &ctx).await {
                    Ok(_) => info!(topic = %topic_name, "CloudFormation created SNS topic"),
                    Err(e) => warn!(topic = %topic_name, error = %e.message, "CloudFormation SNS topic creation failed"),
                }
            }
        }
        "AWS::DynamoDB::Table" => {
            if let Some(dynamodb) = services.get("dynamodb") {
                match dynamodb.handle("CreateTable", properties.clone(), &ctx).await {
                    Ok(_) => info!("CloudFormation created DynamoDB table"),
                    Err(e) => warn!(error = %e.message, "CloudFormation DynamoDB table creation failed"),
                }
            }
        }
        "AWS::IAM::Role" => {
            if let Some(iam) = services.get("iam") {
                let role_name = properties["RoleName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("cf-role-{}", &uuid::Uuid::new_v4().to_string()[..8]));
                let assume_role_doc = properties
                    .get("AssumeRolePolicyDocument")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let input = serde_json::json!({
                    "RoleName": role_name,
                    "AssumeRolePolicyDocument": assume_role_doc,
                });
                match iam.handle("CreateRole", input, &ctx).await {
                    Ok(_) => info!(role = %role_name, "CloudFormation created IAM role"),
                    Err(e) => warn!(role = %role_name, error = %e.message, "CloudFormation IAM role creation failed"),
                }
            }
        }
        "AWS::Lambda::Function" => {
            if let Some(lambda) = services.get("lambda") {
                match lambda.handle("CreateFunction", properties.clone(), &ctx).await {
                    Ok(_) => info!("CloudFormation created Lambda function"),
                    Err(e) => warn!(error = %e.message, "CloudFormation Lambda function creation failed"),
                }
            }
        }
        "AWS::Logs::LogGroup" => {
            if let Some(logs) = services.get("logs") {
                let name = properties["LogGroupName"]
                    .as_str()
                    .unwrap_or("cf-log-group");
                let input = serde_json::json!({ "logGroupName": name });
                match logs.handle("CreateLogGroup", input, &ctx).await {
                    Ok(_) => info!(log_group = %name, "CloudFormation created CloudWatch log group"),
                    Err(e) => warn!(log_group = %name, error = %e.message, "CloudFormation log group creation failed"),
                }
            }
        }
        "AWS::IAM::Policy" => {
            if let Some(iam) = services.get("iam") {
                let policy_name = properties["PolicyName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("cf-policy-{}", &uuid::Uuid::new_v4().to_string()[..8]));
                let policy_doc = properties
                    .get("PolicyDocument")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let input = serde_json::json!({
                    "PolicyName": policy_name,
                    "PolicyDocument": policy_doc,
                });
                match iam.handle("CreatePolicy", input, &ctx).await {
                    Ok(_) => info!(policy = %policy_name, "CloudFormation created IAM policy"),
                    Err(e) => warn!(policy = %policy_name, error = %e.message, "CloudFormation IAM policy creation failed"),
                }
            }
        }
        "AWS::Kinesis::Stream" => {
            if let Some(kinesis) = services.get("kinesis") {
                let stream_name = properties["Name"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("cf-stream-{}", &uuid::Uuid::new_v4().to_string()[..8]));
                let shard_count = properties["ShardCount"].as_u64().unwrap_or(1);
                let input = serde_json::json!({
                    "StreamName": stream_name,
                    "ShardCount": shard_count,
                });
                match kinesis.handle("CreateStream", input, &ctx).await {
                    Ok(_) => info!(stream = %stream_name, "CloudFormation created Kinesis stream"),
                    Err(e) => warn!(stream = %stream_name, error = %e.message, "CloudFormation Kinesis stream creation failed"),
                }
            }
        }
        "AWS::SSM::Parameter" => {
            if let Some(ssm) = services.get("ssm") {
                let name = properties["Name"]
                    .as_str()
                    .unwrap_or("/cf/parameter");
                let param_type = properties["Type"].as_str().unwrap_or("String");
                let value = properties["Value"].as_str().unwrap_or("");
                let input = serde_json::json!({
                    "Name": name,
                    "Type": param_type,
                    "Value": value,
                });
                match ssm.handle("PutParameter", input, &ctx).await {
                    Ok(_) => info!(param = %name, "CloudFormation created SSM parameter"),
                    Err(e) => warn!(param = %name, error = %e.message, "CloudFormation SSM parameter creation failed"),
                }
            }
        }
        other => {
            debug!(resource_type = %other, "Unsupported CloudFormation resource type — skipping");
        }
    }
}

/// Handle a `cloudformation:DeleteResource` event by calling the appropriate
/// service's Delete operation.
pub async fn handle_cf_delete_resource(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let resource_type = match event.detail["resourceType"].as_str() {
        Some(t) => t,
        None => {
            warn!("cloudformation:DeleteResource event missing resourceType");
            return;
        }
    };
    let physical_id = event.detail["physicalResourceId"]
        .as_str()
        .unwrap_or("");

    let ctx = RequestContext {
        account_id: event.account_id.clone(),
        region: event.region.clone(),
        service: "cloudformation".to_string(),
        access_key: None,
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        uri: "/".to_string(),
        event_bus: None,
    };

    match resource_type {
        "AWS::S3::Bucket" => {
            if let Some(s3) = services.get("s3") {
                // physical_id for S3 is the bucket name
                let input = serde_json::json!({ "Bucket": physical_id });
                match s3.handle("DeleteBucket", input, &ctx).await {
                    Ok(_) => info!(bucket = %physical_id, "CloudFormation deleted S3 bucket"),
                    Err(e) => warn!(bucket = %physical_id, error = %e.message, "CloudFormation S3 bucket deletion failed"),
                }
            }
        }
        "AWS::SQS::Queue" => {
            if let Some(sqs) = services.get("sqs") {
                // For SQS the physical ID is a queue URL
                let input = serde_json::json!({ "QueueUrl": physical_id });
                match sqs.handle("DeleteQueue", input, &ctx).await {
                    Ok(_) => info!(queue = %physical_id, "CloudFormation deleted SQS queue"),
                    Err(e) => warn!(queue = %physical_id, error = %e.message, "CloudFormation SQS queue deletion failed"),
                }
            }
        }
        "AWS::SNS::Topic" => {
            if let Some(sns) = services.get("sns") {
                let input = serde_json::json!({ "TopicArn": physical_id });
                match sns.handle("DeleteTopic", input, &ctx).await {
                    Ok(_) => info!(topic = %physical_id, "CloudFormation deleted SNS topic"),
                    Err(e) => warn!(topic = %physical_id, error = %e.message, "CloudFormation SNS topic deletion failed"),
                }
            }
        }
        "AWS::DynamoDB::Table" => {
            if let Some(dynamodb) = services.get("dynamodb") {
                let input = serde_json::json!({ "TableName": physical_id });
                match dynamodb.handle("DeleteTable", input, &ctx).await {
                    Ok(_) => info!(table = %physical_id, "CloudFormation deleted DynamoDB table"),
                    Err(e) => warn!(table = %physical_id, error = %e.message, "CloudFormation DynamoDB table deletion failed"),
                }
            }
        }
        "AWS::IAM::Role" => {
            if let Some(iam) = services.get("iam") {
                let input = serde_json::json!({ "RoleName": physical_id });
                match iam.handle("DeleteRole", input, &ctx).await {
                    Ok(_) => info!(role = %physical_id, "CloudFormation deleted IAM role"),
                    Err(e) => warn!(role = %physical_id, error = %e.message, "CloudFormation IAM role deletion failed"),
                }
            }
        }
        "AWS::Lambda::Function" => {
            if let Some(lambda) = services.get("lambda") {
                let input = serde_json::json!({ "FunctionName": physical_id });
                match lambda.handle("DeleteFunction", input, &ctx).await {
                    Ok(_) => info!(function = %physical_id, "CloudFormation deleted Lambda function"),
                    Err(e) => warn!(function = %physical_id, error = %e.message, "CloudFormation Lambda function deletion failed"),
                }
            }
        }
        "AWS::Logs::LogGroup" => {
            if let Some(logs) = services.get("logs") {
                let input = serde_json::json!({ "logGroupName": physical_id });
                match logs.handle("DeleteLogGroup", input, &ctx).await {
                    Ok(_) => info!(log_group = %physical_id, "CloudFormation deleted CloudWatch log group"),
                    Err(e) => warn!(log_group = %physical_id, error = %e.message, "CloudFormation log group deletion failed"),
                }
            }
        }
        "AWS::IAM::Policy" => {
            if let Some(iam) = services.get("iam") {
                let input = serde_json::json!({ "PolicyArn": physical_id });
                match iam.handle("DeletePolicy", input, &ctx).await {
                    Ok(_) => info!(policy = %physical_id, "CloudFormation deleted IAM policy"),
                    Err(e) => warn!(policy = %physical_id, error = %e.message, "CloudFormation IAM policy deletion failed"),
                }
            }
        }
        "AWS::Kinesis::Stream" => {
            if let Some(kinesis) = services.get("kinesis") {
                let input = serde_json::json!({ "StreamName": physical_id });
                match kinesis.handle("DeleteStream", input, &ctx).await {
                    Ok(_) => info!(stream = %physical_id, "CloudFormation deleted Kinesis stream"),
                    Err(e) => warn!(stream = %physical_id, error = %e.message, "CloudFormation Kinesis stream deletion failed"),
                }
            }
        }
        "AWS::SSM::Parameter" => {
            if let Some(ssm) = services.get("ssm") {
                let input = serde_json::json!({ "Name": physical_id });
                match ssm.handle("DeleteParameter", input, &ctx).await {
                    Ok(_) => info!(param = %physical_id, "CloudFormation deleted SSM parameter"),
                    Err(e) => warn!(param = %physical_id, error = %e.message, "CloudFormation SSM parameter deletion failed"),
                }
            }
        }
        other => {
            debug!(resource_type = %other, "Unsupported CloudFormation resource type — skipping delete");
        }
    }
}
