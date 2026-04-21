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

/// Poll SQS queues for all enabled Lambda event source mappings and invoke Lambda
/// with batches of messages.
pub async fn poll_sqs_event_sources(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
) {
    let lambda = match services.get("lambda") {
        Some(l) => l,
        None => return,
    };
    let sqs = match services.get("sqs") {
        Some(s) => s,
        None => return,
    };

    // List all event source mappings from Lambda
    let ctx = RequestContext::new("lambda", "us-east-1");
    let mappings_result = lambda.handle("ListEventSourceMappings", serde_json::json!({}), &ctx).await;

    if let Ok(result) = mappings_result {
        if let Some(mappings) = result["EventSourceMappings"].as_array() {
            for mapping in mappings {
                let enabled = mapping["State"].as_str() == Some("Enabled");
                if !enabled { continue; }

                let event_source_arn = match mapping["EventSourceArn"].as_str() {
                    Some(arn) if arn.contains(":sqs:") => arn,
                    _ => continue,
                };
                let function_name = match mapping["FunctionName"].as_str() {
                    Some(n) => n,
                    None => continue,
                };
                let batch_size = mapping["BatchSize"].as_u64().unwrap_or(10) as u32;

                // Extract queue URL from ARN
                // ARN format: arn:aws:sqs:{region}:{account}:{queue_name}
                let parts: Vec<&str> = event_source_arn.split(':').collect();
                if parts.len() < 6 { continue; }
                let region = parts[3];
                let account = parts[4];
                let queue_name = parts[5];
                let queue_url = format!("http://sqs.{region}.localhost:4566/{account}/{queue_name}");

                // Receive messages from SQS
                let receive_input = serde_json::json!({
                    "QueueUrl": queue_url,
                    "MaxNumberOfMessages": batch_size,
                    "WaitTimeSeconds": 0,
                });
                let sqs_ctx = RequestContext::new("sqs", region);
                if let Ok(receive_result) = sqs.handle("ReceiveMessage", receive_input, &sqs_ctx).await {
                    if let Some(messages) = receive_result["Messages"].as_array() {
                        if messages.is_empty() { continue; }

                        // Build SQS event for Lambda
                        let records: Vec<serde_json::Value> = messages.iter().map(|msg| {
                            serde_json::json!({
                                "messageId": msg["MessageId"],
                                "receiptHandle": msg["ReceiptHandle"],
                                "body": msg["Body"],
                                "attributes": msg.get("Attributes").unwrap_or(&serde_json::json!({})),
                                "messageAttributes": msg.get("MessageAttributes").unwrap_or(&serde_json::json!({})),
                                "md5OfBody": msg["MD5OfBody"],
                                "eventSource": "aws:sqs",
                                "eventSourceARN": event_source_arn,
                                "awsRegion": region,
                            })
                        }).collect();

                        let lambda_event = serde_json::json!({ "Records": records });

                        // Invoke Lambda
                        let invoke_input = serde_json::json!({
                            "FunctionName": function_name,
                            "Payload": serde_json::to_string(&lambda_event).unwrap_or_default(),
                            "InvocationType": "Event",
                        });
                        let lambda_ctx = RequestContext::new("lambda", region);
                        if lambda.handle("Invoke", invoke_input, &lambda_ctx).await.is_ok() {
                            // Delete messages on successful invocation
                            for msg in messages {
                                if let Some(receipt) = msg["ReceiptHandle"].as_str() {
                                    let delete_input = serde_json::json!({
                                        "QueueUrl": queue_url,
                                        "ReceiptHandle": receipt,
                                    });
                                    let _ = sqs.handle("DeleteMessage", delete_input, &sqs_ctx).await;
                                }
                            }

                            debug!(
                                function = function_name,
                                queue = queue_name,
                                count = messages.len(),
                                "SQS->Lambda: delivered batch"
                            );
                        } else {
                            warn!(
                                function = function_name,
                                queue = queue_name,
                                "SQS->Lambda: Lambda invocation failed, messages remain in queue"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Handle an S3 object event (ObjectCreated or ObjectRemoved) by routing it to
/// the configured SNS, SQS, or Lambda destinations.
pub async fn handle_s3_event(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let bucket_name = match event.detail["bucket"]["name"].as_str() {
        Some(n) => n.to_string(),
        None => {
            warn!("S3 event missing bucket name");
            return;
        }
    };
    let key = event.detail["object"]["key"].as_str().unwrap_or("").to_string();
    let size = event.detail["object"]["size"].as_u64().unwrap_or(0);
    let etag = event.detail["object"]["eTag"].as_str().unwrap_or("").to_string();

    let configured_destinations = match event.detail["configuredDestinations"].as_array() {
        Some(d) => d.clone(),
        None => return,
    };

    if configured_destinations.is_empty() {
        return;
    }

    // Build the S3 event record following the real AWS S3 notification format
    let event_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let s3_record = serde_json::json!({
        "eventVersion": "2.1",
        "eventSource": "aws:s3",
        "awsRegion": event.region,
        "eventTime": event_time,
        "eventName": event.event_type.trim_start_matches("s3:"),
        "s3": {
            "s3SchemaVersion": "1.0",
            "bucket": {
                "name": bucket_name,
                "arn": format!("arn:aws:s3:::{}", bucket_name),
            },
            "object": {
                "key": key,
                "size": size,
                "eTag": etag,
            }
        }
    });

    let s3_event = serde_json::json!({ "Records": [s3_record] });

    for dest in &configured_destinations {
        let dest_type = dest["type"].as_str().unwrap_or("");
        let dest_arn = dest["arn"].as_str().unwrap_or("");

        match dest_type {
            "sqs" => {
                if let Some(sqs) = services.get("sqs") {
                    // ARN format: arn:aws:sqs:{region}:{account}:{queue_name}
                    let parts: Vec<&str> = dest_arn.splitn(6, ':').collect();
                    let queue_url = if parts.len() == 6 {
                        format!("http://sqs.{}.localhost:4566/{}/{}", parts[3], parts[4], parts[5])
                    } else {
                        continue;
                    };
                    let sqs_ctx = RequestContext {
                        account_id: event.account_id.clone(),
                        region: event.region.clone(),
                        service: "sqs".to_string(),
                        access_key: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        method: "POST".to_string(),
                        uri: "/".to_string(),
                        event_bus: None,
                    };
                    let input = serde_json::json!({
                        "QueueUrl": queue_url,
                        "MessageBody": s3_event.to_string(),
                    });
                    match sqs.handle("SendMessage", input, &sqs_ctx).await {
                        Ok(_) => info!(
                            bucket = %bucket_name,
                            event_type = %event.event_type,
                            queue = %dest_arn,
                            "S3->SQS notification delivered"
                        ),
                        Err(e) => warn!(
                            bucket = %bucket_name,
                            queue = %dest_arn,
                            error = %e.message,
                            "S3->SQS notification delivery failed"
                        ),
                    }
                }
            }
            "sns" => {
                if let Some(sns) = services.get("sns") {
                    let sns_ctx = RequestContext {
                        account_id: event.account_id.clone(),
                        region: event.region.clone(),
                        service: "sns".to_string(),
                        access_key: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        method: "POST".to_string(),
                        uri: "/".to_string(),
                        event_bus: None,
                    };
                    let input = serde_json::json!({
                        "TopicArn": dest_arn,
                        "Message": s3_event.to_string(),
                        "Subject": format!("Amazon S3 Notification: {}", event.event_type),
                    });
                    match sns.handle("Publish", input, &sns_ctx).await {
                        Ok(_) => info!(
                            bucket = %bucket_name,
                            event_type = %event.event_type,
                            topic = %dest_arn,
                            "S3->SNS notification delivered"
                        ),
                        Err(e) => warn!(
                            bucket = %bucket_name,
                            topic = %dest_arn,
                            error = %e.message,
                            "S3->SNS notification delivery failed"
                        ),
                    }
                }
            }
            "lambda" => {
                if let Some(lambda) = services.get("lambda") {
                    let lambda_ctx = RequestContext {
                        account_id: event.account_id.clone(),
                        region: event.region.clone(),
                        service: "lambda".to_string(),
                        access_key: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        method: "POST".to_string(),
                        uri: "/".to_string(),
                        event_bus: None,
                    };
                    let invoke_input = serde_json::json!({
                        "FunctionName": dest_arn,
                        "Payload": s3_event.to_string(),
                        "InvocationType": "Event",
                    });
                    match lambda.handle("Invoke", invoke_input, &lambda_ctx).await {
                        Ok(_) => info!(
                            bucket = %bucket_name,
                            event_type = %event.event_type,
                            function = %dest_arn,
                            "S3->Lambda notification delivered"
                        ),
                        Err(e) => warn!(
                            bucket = %bucket_name,
                            function = %dest_arn,
                            error = %e.message,
                            "S3->Lambda notification delivery failed"
                        ),
                    }
                }
            }
            other => {
                warn!(dest_type = %other, "Unknown S3 notification destination type");
            }
        }
    }
}

/// Handle a `dynamodb:StreamRecord` event.
///
/// Looks up all Lambda event source mappings whose `EventSourceArn` matches
/// the stream ARN in the event, then invokes each matching function with the
/// DynamoDB stream event payload (the standard `{ "Records": [...] }` envelope
/// that the real AWS Lambda runtime receives).
pub async fn handle_dynamodb_stream(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let stream_arn = match event.detail["streamArn"].as_str() {
        Some(a) => a.to_string(),
        None => {
            warn!("dynamodb:StreamRecord event missing streamArn");
            return;
        }
    };

    let records = match event.detail["records"].as_array() {
        Some(r) => r.clone(),
        None => {
            warn!("dynamodb:StreamRecord event missing records array");
            return;
        }
    };

    // Build the standard Lambda DynamoDB stream event envelope.
    let lambda_payload = serde_json::json!({ "Records": records });

    let lambda_handler = match services.get("lambda") {
        Some(h) => h.clone(),
        None => return,
    };

    // List all event source mappings and filter those that match the stream ARN.
    let ctx = RequestContext {
        account_id: event.account_id.clone(),
        region: event.region.clone(),
        service: "lambda".to_string(),
        access_key: None,
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "GET".to_string(),
        uri: "/".to_string(),
        event_bus: None,
    };

    let list_input = serde_json::json!({ "EventSourceArn": stream_arn });
    let mappings = match lambda_handler
        .handle("ListEventSourceMappings", list_input, &ctx)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e.message, "Failed to list event source mappings for DDB stream");
            return;
        }
    };

    let mapping_list = match mappings["EventSourceMappings"].as_array() {
        Some(m) => m.clone(),
        None => return,
    };

    for mapping in mapping_list {
        let state = mapping["State"].as_str().unwrap_or("Disabled");
        if state != "Enabled" {
            continue;
        }

        let function_arn = match mapping["FunctionArn"].as_str() {
            Some(f) => f.to_string(),
            None => continue,
        };

        let invoke_ctx = RequestContext {
            account_id: event.account_id.clone(),
            region: event.region.clone(),
            service: "lambda".to_string(),
            access_key: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            method: "POST".to_string(),
            uri: format!("/2015-03-31/functions/{function_arn}/invocations"),
            event_bus: None,
        };

        let invoke_input = serde_json::json!({
            "FunctionName": function_arn,
            "InvocationType": "Event",
            "Payload": lambda_payload,
        });

        match lambda_handler
            .handle("Invoke", invoke_input, &invoke_ctx)
            .await
        {
            Ok(_) => info!(
                function = %function_arn,
                stream = %stream_arn,
                "DynamoDB stream triggered Lambda function"
            ),
            Err(e) => warn!(
                function = %function_arn,
                stream = %stream_arn,
                error = %e.message,
                "DynamoDB stream Lambda invocation failed"
            ),
        }
    }
}

/// Handle an `eventbridge:TargetInvocation` event by dispatching to the
/// appropriate service (Lambda, SQS, or SNS) based on the target ARN.
pub async fn handle_eventbridge_target(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let target_arn = event.detail["targetArn"].as_str().unwrap_or("");
    let payload = &event.detail["event"];

    if target_arn.contains(":function:") {
        // Lambda target
        if let Some(lambda) = services.get("lambda") {
            let func_name = target_arn
                .split(":function:")
                .last()
                .unwrap_or("");
            let input = serde_json::json!({
                "FunctionName": func_name,
                "Payload": serde_json::to_string(payload).unwrap_or_default(),
                "InvocationType": "Event",
            });
            let ctx = RequestContext::new("lambda", &event.region);
            match lambda.handle("Invoke", input, &ctx).await {
                Ok(_) => info!(function = %func_name, rule = ?event.detail["ruleName"], "EventBridge->Lambda invocation delivered"),
                Err(e) => warn!(function = %func_name, error = %e.message, "EventBridge->Lambda invocation failed"),
            }
        }
    } else if target_arn.contains(":sqs:") {
        // SQS target — ARN format: arn:aws:sqs:{region}:{account}:{queue_name}
        if let Some(sqs) = services.get("sqs") {
            let parts: Vec<&str> = target_arn.splitn(6, ':').collect();
            let queue_url = if parts.len() == 6 {
                format!("http://sqs.{}.localhost:4566/{}/{}", parts[3], parts[4], parts[5])
            } else {
                // Fallback: extract last segment as queue name
                let queue_name = target_arn.split(':').last().unwrap_or("");
                format!("http://sqs.{}.localhost:4566/000000000000/{}", event.region, queue_name)
            };
            let input = serde_json::json!({
                "QueueUrl": queue_url,
                "MessageBody": serde_json::to_string(payload).unwrap_or_default(),
            });
            let ctx = RequestContext::new("sqs", &event.region);
            match sqs.handle("SendMessage", input, &ctx).await {
                Ok(_) => info!(queue = %target_arn, rule = ?event.detail["ruleName"], "EventBridge->SQS message delivered"),
                Err(e) => warn!(queue = %target_arn, error = %e.message, "EventBridge->SQS delivery failed"),
            }
        }
    } else if target_arn.contains(":sns:") {
        // SNS target
        if let Some(sns) = services.get("sns") {
            let input = serde_json::json!({
                "TopicArn": target_arn,
                "Message": serde_json::to_string(payload).unwrap_or_default(),
            });
            let ctx = RequestContext::new("sns", &event.region);
            match sns.handle("Publish", input, &ctx).await {
                Ok(_) => info!(topic = %target_arn, rule = ?event.detail["ruleName"], "EventBridge->SNS message delivered"),
                Err(e) => warn!(topic = %target_arn, error = %e.message, "EventBridge->SNS delivery failed"),
            }
        }
    } else {
        warn!(target_arn = %target_arn, "EventBridge target type not supported");
    }
}

/// Poll Kinesis streams for all enabled Lambda event source mappings and invoke
/// Lambda with batches of records.
pub async fn poll_kinesis_event_sources(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
) {
    let lambda = match services.get("lambda") {
        Some(l) => l,
        None => return,
    };
    let kinesis = match services.get("kinesis") {
        Some(k) => k,
        None => return,
    };

    // List all event source mappings from Lambda
    let ctx = RequestContext::new("lambda", "us-east-1");
    let mappings_result = lambda.handle("ListEventSourceMappings", serde_json::json!({}), &ctx).await;

    if let Ok(result) = mappings_result {
        if let Some(mappings) = result["EventSourceMappings"].as_array() {
            for mapping in mappings {
                if mapping["State"].as_str() != Some("Enabled") {
                    continue;
                }

                let event_source_arn = match mapping["EventSourceArn"].as_str() {
                    Some(arn) if arn.contains(":kinesis:") => arn,
                    _ => continue,
                };

                let function_name = match mapping["FunctionName"].as_str() {
                    Some(f) => f,
                    None => continue,
                };
                let batch_size = mapping["BatchSize"].as_u64().unwrap_or(100);

                // Extract stream name from ARN: arn:aws:kinesis:{region}:{account}:stream/{name}
                let stream_name = event_source_arn.split('/').last().unwrap_or("");
                if stream_name.is_empty() {
                    continue;
                }

                // Derive the region from the ARN if possible
                let parts: Vec<&str> = event_source_arn.splitn(6, ':').collect();
                let region = if parts.len() >= 4 { parts[3] } else { "us-east-1" };

                let kinesis_ctx = RequestContext::new("kinesis", region);

                // Get shard iterator (TRIM_HORIZON to pick up unread records)
                let iter_input = serde_json::json!({
                    "StreamName": stream_name,
                    "ShardId": "shardId-000000000000",
                    "ShardIteratorType": "TRIM_HORIZON",
                });
                let iter_result = match kinesis.handle("GetShardIterator", iter_input, &kinesis_ctx).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(stream = %stream_name, error = %e.message, "Kinesis->Lambda: GetShardIterator failed");
                        continue;
                    }
                };

                let iterator = match iter_result["ShardIterator"].as_str() {
                    Some(i) => i.to_string(),
                    None => continue,
                };

                let records_input = serde_json::json!({
                    "ShardIterator": iterator,
                    "Limit": batch_size,
                });
                let records_result = match kinesis.handle("GetRecords", records_input, &kinesis_ctx).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(stream = %stream_name, error = %e.message, "Kinesis->Lambda: GetRecords failed");
                        continue;
                    }
                };

                let records = match records_result["Records"].as_array() {
                    Some(r) if !r.is_empty() => r.clone(),
                    _ => continue,
                };

                let lambda_event = serde_json::json!({ "Records": records });
                let invoke_input = serde_json::json!({
                    "FunctionName": function_name,
                    "Payload": serde_json::to_string(&lambda_event).unwrap_or_default(),
                    "InvocationType": "Event",
                });
                let lambda_ctx = RequestContext::new("lambda", region);
                match lambda.handle("Invoke", invoke_input, &lambda_ctx).await {
                    Ok(_) => debug!(
                        function = %function_name,
                        stream = %stream_name,
                        count = records.len(),
                        "Kinesis->Lambda: delivered batch"
                    ),
                    Err(e) => warn!(
                        function = %function_name,
                        stream = %stream_name,
                        error = %e.message,
                        "Kinesis->Lambda: Lambda invocation failed"
                    ),
                }
            }
        }
    }
}

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
