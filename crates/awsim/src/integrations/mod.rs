/// Cross-service integration handlers invoked by the background event router.
///
/// When CloudFormation creates or deletes a stack, it emits one
/// `cloudformation:CreateResource` / `cloudformation:DeleteResource` event per
/// resource.  The functions in this module receive those events and forward them
/// to the appropriate service handler so that the resources actually exist in
/// the target service (S3, SQS, SNS, etc.).
use std::collections::HashMap;
use std::sync::Arc;

use awsim_core::{AccountRegionStore, InternalEvent, RequestContext, ServiceHandler};
use awsim_lambda::state::LambdaState;
use serde_json::Value;
use tracing::{debug, info, warn};

mod esm;
pub mod pipes;

/// Snapshot of the fields the SQS poller needs from an EventSourceMapping.
/// Tuple aliased to keep clippy's type-complexity lint quiet.
type SqsMappingSnapshot = (
    String,         // uuid
    String,         // event_source_arn
    String,         // function_arn
    u32,            // batch_size
    Option<Value>,  // filter_criteria
    Option<String>, // destination_on_failure
);

/// Snapshot of the fields the Kinesis poller needs from an EventSourceMapping.
type KinesisMappingSnapshot = (
    String,         // uuid
    String,         // event_source_arn
    String,         // function_arn
    u32,            // batch_size
    Option<String>, // starting_position
    Option<f64>,    // starting_position_timestamp
    Option<Value>,  // filter_criteria
    Option<String>, // destination_on_failure
    Option<String>, // saved iterator for shard 0
);

/// Poll SQS queues for every enabled Lambda event source mapping in every
/// (account, region) and invoke Lambda with batches of messages. Honors
/// FilterCriteria when configured, and routes failed batches to the
/// DestinationConfig.OnFailure target if one is set.
pub async fn poll_sqs_event_sources(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    lambda_store: &AccountRegionStore<LambdaState>,
) {
    let lambda = match services.get("lambda") {
        Some(l) => l.clone(),
        None => return,
    };
    let sqs = match services.get("sqs") {
        Some(s) => s.clone(),
        None => return,
    };

    for ((account_id, region), state) in lambda_store.iter_all() {
        let mappings: Vec<SqsMappingSnapshot> = state
            .event_source_mappings
            .iter()
            .filter_map(|entry| {
                let m = entry.value();
                if m.state != "Enabled" {
                    return None;
                }
                if !m.event_source_arn.contains(":sqs:") {
                    return None;
                }
                Some((
                    m.uuid.clone(),
                    m.event_source_arn.clone(),
                    m.function_arn.clone(),
                    m.batch_size,
                    m.filter_criteria.clone(),
                    m.destination_on_failure.clone(),
                ))
            })
            .collect();

        for (uuid, event_source_arn, function_arn, batch_size, filter_criteria, dlq_arn) in mappings
        {
            let parts: Vec<&str> = event_source_arn.split(':').collect();
            if parts.len() < 6 {
                continue;
            }
            let queue_region = parts[3];
            let queue_account = parts[4];
            let queue_name = parts[5];
            let queue_url =
                format!("http://sqs.{queue_region}.localhost:4566/{queue_account}/{queue_name}");

            let receive_input = serde_json::json!({
                "QueueUrl": queue_url,
                "MaxNumberOfMessages": batch_size,
                "WaitTimeSeconds": 0,
            });
            let sqs_ctx = RequestContext::new("sqs", queue_region);
            let receive_result = match sqs.handle("ReceiveMessage", receive_input, &sqs_ctx).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            let messages = match receive_result["Messages"].as_array() {
                Some(m) if !m.is_empty() => m.clone(),
                _ => continue,
            };

            let raw_records: Vec<Value> = messages
                .iter()
                .map(|msg| {
                    serde_json::json!({
                        "messageId": msg["MessageId"],
                        "receiptHandle": msg["ReceiptHandle"],
                        "body": msg["Body"],
                        "attributes": msg.get("Attributes").unwrap_or(&Value::Object(Default::default())),
                        "messageAttributes": msg.get("MessageAttributes").unwrap_or(&Value::Object(Default::default())),
                        "md5OfBody": msg["MD5OfBody"],
                        "eventSource": "aws:sqs",
                        "eventSourceARN": event_source_arn,
                        "awsRegion": region,
                    })
                })
                .collect();

            let (kept, filtered_handles) =
                esm::partition_by_filter(&raw_records, filter_criteria.as_ref(), |rec| {
                    rec.get("receiptHandle")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });

            // Filtered-out messages are considered consumed — delete them so the
            // queue doesn't loop them forever. This matches real Lambda ESM behavior.
            for handle in &filtered_handles {
                let _ = sqs
                    .handle(
                        "DeleteMessage",
                        serde_json::json!({ "QueueUrl": queue_url, "ReceiptHandle": handle }),
                        &sqs_ctx,
                    )
                    .await;
            }

            if kept.is_empty() {
                set_last_result(&state, &uuid, "OK");
                continue;
            }

            let lambda_event = serde_json::json!({ "Records": kept });
            let invoke_input = serde_json::json!({
                "FunctionName": function_arn,
                "Payload": serde_json::to_string(&lambda_event).unwrap_or_default(),
                "InvocationType": "Event",
            });
            let lambda_ctx = RequestContext::new_with_account("lambda", &region, &account_id);
            match lambda.handle("Invoke", invoke_input, &lambda_ctx).await {
                Ok(_) => {
                    for rec in &kept {
                        if let Some(handle) = rec.get("receiptHandle").and_then(|v| v.as_str()) {
                            let _ = sqs
                                .handle(
                                    "DeleteMessage",
                                    serde_json::json!({ "QueueUrl": queue_url, "ReceiptHandle": handle }),
                                    &sqs_ctx,
                                )
                                .await;
                        }
                    }
                    debug!(
                        function = %function_arn,
                        queue = queue_name,
                        account = %account_id,
                        region = %region,
                        count = kept.len(),
                        "SQS->Lambda: delivered batch"
                    );
                    set_last_result(&state, &uuid, "OK");
                }
                Err(e) => {
                    warn!(
                        function = %function_arn,
                        queue = queue_name,
                        error = %e.message,
                        "SQS->Lambda: invocation failed; messages remain in queue"
                    );
                    if let Some(dlq) = &dlq_arn {
                        esm::route_to_destination(
                            services,
                            dlq,
                            &lambda_event,
                            &account_id,
                            &region,
                        )
                        .await;
                    }
                    set_last_result(
                        &state,
                        &uuid,
                        &format!("PROBLEM: invoke failed: {}", e.message),
                    );
                }
            }
        }
    }
}

fn set_last_result(state: &Arc<LambdaState>, uuid: &str, result: &str) {
    if let Some(mut m) = state.event_source_mappings.get_mut(uuid) {
        m.last_processing_result = result.to_string();
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
    let key = event.detail["object"]["key"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let size = event.detail["object"]["size"].as_u64().unwrap_or(0);
    let etag = event.detail["object"]["eTag"]
        .as_str()
        .unwrap_or("")
        .to_string();

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
                // Partition left literal: InternalEvent carries no partition.
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
                        format!(
                            "http://sqs.{}.localhost:4566/{}/{}",
                            parts[3], parts[4], parts[5]
                        )
                    } else {
                        continue;
                    };
                    let sqs_ctx = RequestContext {
                        account_id: event.account_id.clone(),
                        region: event.region.clone(),
                        partition: awsim_core::DEFAULT_PARTITION.to_string(),
                        service: "sqs".to_string(),
                        access_key: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        method: "POST".to_string(),
                        uri: "/".to_string(),
                        event_bus: None,
                        source_ip: None,
                        is_secure: false,
                        internal_bypass: false,
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
                        partition: awsim_core::DEFAULT_PARTITION.to_string(),
                        service: "sns".to_string(),
                        access_key: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        method: "POST".to_string(),
                        uri: "/".to_string(),
                        event_bus: None,
                        source_ip: None,
                        is_secure: false,
                        internal_bypass: false,
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
                        partition: awsim_core::DEFAULT_PARTITION.to_string(),
                        service: "lambda".to_string(),
                        access_key: None,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        method: "POST".to_string(),
                        uri: "/".to_string(),
                        event_bus: None,
                        source_ip: None,
                        is_secure: false,
                        internal_bypass: false,
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

    let lambda_handler = match services.get("lambda") {
        Some(h) => h.clone(),
        None => return,
    };

    // List all event source mappings and filter those that match the stream ARN.
    let ctx = RequestContext {
        account_id: event.account_id.clone(),
        region: event.region.clone(),
        partition: awsim_core::DEFAULT_PARTITION.to_string(),
        service: "lambda".to_string(),
        access_key: None,
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "GET".to_string(),
        uri: "/".to_string(),
        event_bus: None,
        source_ip: None,
        is_secure: false,
        internal_bypass: false,
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

        let filter_criteria = mapping.get("FilterCriteria").cloned();
        let dlq_arn = mapping
            .get("DestinationConfig")
            .and_then(|d| d.get("OnFailure"))
            .and_then(|f| f.get("Destination"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let (kept, _) = esm::partition_by_filter(&records, filter_criteria.as_ref(), |_| None);
        if kept.is_empty() {
            continue;
        }
        let per_mapping_payload = serde_json::json!({ "Records": kept });

        let invoke_ctx = RequestContext {
            account_id: event.account_id.clone(),
            region: event.region.clone(),
            partition: awsim_core::DEFAULT_PARTITION.to_string(),
            service: "lambda".to_string(),
            access_key: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            method: "POST".to_string(),
            uri: format!("/2015-03-31/functions/{function_arn}/invocations"),
            event_bus: None,
            source_ip: None,
            is_secure: false,
            internal_bypass: false,
        };

        let invoke_input = serde_json::json!({
            "FunctionName": function_arn,
            "InvocationType": "Event",
            "Payload": per_mapping_payload,
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
            Err(e) => {
                warn!(
                    function = %function_arn,
                    stream = %stream_arn,
                    error = %e.message,
                    "DynamoDB stream Lambda invocation failed"
                );
                if let Some(dlq) = &dlq_arn {
                    esm::route_to_destination(
                        services,
                        dlq,
                        &per_mapping_payload,
                        &event.account_id,
                        &event.region,
                    )
                    .await;
                }
            }
        }
    }
}

/// Handle an `eventbridge:TargetInvocation` event by dispatching to the
/// appropriate service (Lambda, SQS, or SNS) based on the target ARN.
/// Fan out a `ses:EmailEvent` to its configured event-destination
/// target. SES configuration-set event destinations forward send /
/// delivery / bounce notifications to SNS, Kinesis Firehose, or
/// CloudWatch metrics; this re-dispatches to the in-process handler
/// named in `detail.destination.kind`.
pub async fn handle_ses_event(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let dest = &event.detail["destination"];
    let event_type = event.detail["eventType"].as_str().unwrap_or("SEND");
    let body = event.detail.to_string();
    match dest["kind"].as_str() {
        Some("sns") => {
            if let Some(sns) = services.get("sns") {
                let arn = dest["arn"].as_str().unwrap_or("");
                let input = serde_json::json!({ "TopicArn": arn, "Message": body });
                let ctx = RequestContext::new_with_account("sns", &event.region, &event.account_id);
                match sns.handle("Publish", input, &ctx).await {
                    Ok(_) => info!(topic = %arn, "SES->SNS event delivered"),
                    Err(e) => {
                        warn!(topic = %arn, error = %e.message, "SES->SNS event delivery failed")
                    }
                }
            }
        }
        Some("firehose") => {
            if let Some(fh) = services.get("firehose") {
                let name = dest["arn"]
                    .as_str()
                    .and_then(|a| a.rsplit_once("deliverystream/").map(|(_, n)| n))
                    .unwrap_or("");
                use base64::Engine as _;
                let data = base64::engine::general_purpose::STANDARD.encode(&body);
                let input =
                    serde_json::json!({ "DeliveryStreamName": name, "Record": { "Data": data } });
                let ctx =
                    RequestContext::new_with_account("firehose", &event.region, &event.account_id);
                match fh.handle("PutRecord", input, &ctx).await {
                    Ok(_) => info!(stream = %name, "SES->Firehose event delivered"),
                    Err(e) => {
                        warn!(stream = %name, error = %e.message, "SES->Firehose event delivery failed")
                    }
                }
            }
        }
        Some("cloudwatch") => {
            // CloudWatch metrics registers under the "monitoring" key.
            if let Some(cw) = services.get("monitoring") {
                let input = serde_json::json!({
                    "Namespace": "AWS/SES",
                    "MetricData": [{ "MetricName": event_type, "Value": 1.0, "Unit": "Count" }],
                });
                let ctx = RequestContext::new_with_account(
                    "monitoring",
                    &event.region,
                    &event.account_id,
                );
                match cw.handle("PutMetricData", input, &ctx).await {
                    Ok(_) => info!(metric = %event_type, "SES->CloudWatch metric delivered"),
                    Err(e) => {
                        warn!(metric = %event_type, error = %e.message, "SES->CloudWatch metric failed")
                    }
                }
            }
        }
        _ => {}
    }
}

/// Fan out one `ses:ReceiptAction` emitted by synthetic inbound
/// delivery. SNS and Lambda actions are dispatched to the in-process
/// handler; S3 / Bounce / AddHeader / Stop actions are recorded in the
/// delivery summary and need no live fan-out.
pub async fn handle_ses_receipt_action(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let action_type = event.detail["actionType"].as_str().unwrap_or("");
    let action = &event.detail["action"][action_type];
    let message_id = event.detail["messageId"].as_str().unwrap_or("");
    match action_type {
        "SNSAction" => {
            if let Some(sns) = services.get("sns") {
                let arn = action["TopicArn"].as_str().unwrap_or("");
                let input = serde_json::json!({
                    "TopicArn": arn,
                    "Message": event.detail.to_string(),
                });
                let ctx = RequestContext::new_with_account("sns", &event.region, &event.account_id);
                match sns.handle("Publish", input, &ctx).await {
                    Ok(_) => info!(topic = %arn, message_id, "SES receipt SNSAction delivered"),
                    Err(e) => {
                        warn!(topic = %arn, error = %e.message, "SES receipt SNSAction failed")
                    }
                }
            }
        }
        "LambdaAction" => {
            if let Some(lambda) = services.get("lambda") {
                let func = action["FunctionArn"].as_str().unwrap_or("");
                let func_name = func.rsplit(":function:").next().unwrap_or(func);
                let input = serde_json::json!({
                    "FunctionName": func_name,
                    "Payload": event.detail.to_string(),
                    "InvocationType": action["InvocationType"].as_str().unwrap_or("Event"),
                });
                let ctx =
                    RequestContext::new_with_account("lambda", &event.region, &event.account_id);
                match lambda.handle("Invoke", input, &ctx).await {
                    Ok(_) => {
                        info!(function = %func_name, message_id, "SES receipt LambdaAction delivered")
                    }
                    Err(e) => {
                        warn!(function = %func_name, error = %e.message, "SES receipt LambdaAction failed")
                    }
                }
            }
        }
        other => {
            debug!(
                action = other,
                message_id, "SES receipt action recorded (no fan-out)"
            );
        }
    }
}

pub async fn handle_eventbridge_target(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let target_arn = event.detail["targetArn"].as_str().unwrap_or("");
    let payload = &event.detail["event"];

    if target_arn.contains(":function:") {
        // Lambda target
        if let Some(lambda) = services.get("lambda") {
            let func_name = target_arn.split(":function:").last().unwrap_or("");
            let input = serde_json::json!({
                "FunctionName": func_name,
                "Payload": serde_json::to_string(payload).unwrap_or_default(),
                "InvocationType": "Event",
            });
            let ctx = RequestContext::new("lambda", &event.region);
            match lambda.handle("Invoke", input, &ctx).await {
                Ok(_) => {
                    info!(function = %func_name, rule = ?event.detail["ruleName"], "EventBridge->Lambda invocation delivered")
                }
                Err(e) => {
                    warn!(function = %func_name, error = %e.message, "EventBridge->Lambda invocation failed")
                }
            }
        }
    } else if target_arn.contains(":sqs:") {
        // SQS target — ARN format: arn:aws:sqs:{region}:{account}:{queue_name}
        if let Some(sqs) = services.get("sqs") {
            let parts: Vec<&str> = target_arn.splitn(6, ':').collect();
            let queue_url = if parts.len() == 6 {
                format!(
                    "http://sqs.{}.localhost:4566/{}/{}",
                    parts[3], parts[4], parts[5]
                )
            } else {
                // Fallback: extract last segment as queue name
                let queue_name = target_arn.split(':').next_back().unwrap_or("");
                format!(
                    "http://sqs.{}.localhost:4566/000000000000/{}",
                    event.region, queue_name
                )
            };
            let input = serde_json::json!({
                "QueueUrl": queue_url,
                "MessageBody": serde_json::to_string(payload).unwrap_or_default(),
            });
            let ctx = RequestContext::new("sqs", &event.region);
            match sqs.handle("SendMessage", input, &ctx).await {
                Ok(_) => {
                    info!(queue = %target_arn, rule = ?event.detail["ruleName"], "EventBridge->SQS message delivered")
                }
                Err(e) => {
                    warn!(queue = %target_arn, error = %e.message, "EventBridge->SQS delivery failed")
                }
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
                Ok(_) => {
                    info!(topic = %target_arn, rule = ?event.detail["ruleName"], "EventBridge->SNS message delivered")
                }
                Err(e) => {
                    warn!(topic = %target_arn, error = %e.message, "EventBridge->SNS delivery failed")
                }
            }
        }
    } else if target_arn.contains(":kinesis:") {
        // Kinesis stream — arn:aws:kinesis:{region}:{account}:stream/{name}
        if let Some(kinesis) = services.get("kinesis") {
            let stream_name = target_arn
                .rsplit_once("stream/")
                .map(|(_, n)| n)
                .unwrap_or("");
            let payload_str = serde_json::to_string(payload).unwrap_or_default();
            // Real EventBridge supports a KinesisParameters.PartitionKeyPath
            // pointer into the event; we don't track per-target params here,
            // so default to the rule name as the partition key. Stable
            // enough that all events from the same rule land in the same
            // shard, which is the common intent.
            let partition_key = event.detail["ruleName"]
                .as_str()
                .unwrap_or("eventbridge")
                .to_string();
            use base64::Engine as _;
            let data_b64 = base64::engine::general_purpose::STANDARD.encode(payload_str);
            let input = serde_json::json!({
                "StreamName": stream_name,
                "Data": data_b64,
                "PartitionKey": partition_key,
            });
            let ctx = RequestContext::new("kinesis", &event.region);
            match kinesis.handle("PutRecord", input, &ctx).await {
                Ok(_) => {
                    info!(stream = %stream_name, rule = ?event.detail["ruleName"], "EventBridge->Kinesis record delivered")
                }
                Err(e) => {
                    warn!(stream = %stream_name, error = %e.message, "EventBridge->Kinesis delivery failed")
                }
            }
        }
    } else if target_arn.contains(":states:") {
        // Step Functions — arn:aws:states:{region}:{account}:stateMachine:{name}
        if let Some(sfn) = services.get("stepfunctions") {
            let input_str = serde_json::to_string(payload).unwrap_or_default();
            let input = serde_json::json!({
                "stateMachineArn": target_arn,
                "input": input_str,
            });
            let ctx = RequestContext::new("stepfunctions", &event.region);
            match sfn.handle("StartExecution", input, &ctx).await {
                Ok(_) => {
                    info!(arn = %target_arn, rule = ?event.detail["ruleName"], "EventBridge->StepFunctions execution started")
                }
                Err(e) => {
                    warn!(arn = %target_arn, error = %e.message, "EventBridge->StepFunctions delivery failed")
                }
            }
        }
    } else if target_arn.contains(":logs:") {
        // CloudWatch Logs — arn:aws:logs:{region}:{account}:log-group:{name}[:*]
        if let Some(logs) = services.get("logs") {
            // Strip optional :* suffix and the log-group: prefix.
            let log_group_name = target_arn
                .rsplit_once("log-group:")
                .map(|(_, rest)| rest.trim_end_matches(":*"))
                .unwrap_or("");
            let payload_str = serde_json::to_string(payload).unwrap_or_default();
            // Use a single stream per rule so EB-sourced events stay
            // grouped and don't fan out into hundreds of streams. The
            // SDK auto-creates the stream when missing.
            let log_stream_name = format!(
                "events/{}",
                event.detail["ruleName"].as_str().unwrap_or("default")
            );
            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let input = serde_json::json!({
                "logGroupName": log_group_name,
                "logStreamName": log_stream_name,
                "logEvents": [{
                    "timestamp": timestamp_ms,
                    "message": payload_str,
                }],
            });
            let ctx = RequestContext::new("logs", &event.region);
            match logs.handle("PutLogEvents", input, &ctx).await {
                Ok(_) => {
                    info!(log_group = %log_group_name, rule = ?event.detail["ruleName"], "EventBridge->Logs event delivered")
                }
                Err(e) => {
                    warn!(log_group = %log_group_name, error = %e.message, "EventBridge->Logs delivery failed")
                }
            }
        }
    } else {
        warn!(target_arn = %target_arn, "EventBridge target type not supported");
    }
}

/// Poll Kinesis streams for every enabled Lambda event source mapping in
/// every (account, region). The shard iterator returned by GetRecords is
/// persisted on the mapping so the next tick resumes where we left off,
/// instead of re-fetching `TRIM_HORIZON` and re-delivering records forever.
pub async fn poll_kinesis_event_sources(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    lambda_store: &AccountRegionStore<LambdaState>,
) {
    let lambda = match services.get("lambda") {
        Some(l) => l.clone(),
        None => return,
    };
    let kinesis = match services.get("kinesis") {
        Some(k) => k.clone(),
        None => return,
    };

    const SHARD_ID: &str = "shardId-000000000000";

    for ((account_id, region), state) in lambda_store.iter_all() {
        // Snapshot the mappings up front so we don't hold a DashMap reference
        // across .await points.
        let mappings: Vec<KinesisMappingSnapshot> = state
            .event_source_mappings
            .iter()
            .filter_map(|entry| {
                let m = entry.value();
                if m.state != "Enabled" {
                    return None;
                }
                if !m.event_source_arn.contains(":kinesis:") {
                    return None;
                }
                Some((
                    m.uuid.clone(),
                    m.event_source_arn.clone(),
                    m.function_arn.clone(),
                    m.batch_size,
                    m.starting_position.clone(),
                    m.starting_position_timestamp,
                    m.filter_criteria.clone(),
                    m.destination_on_failure.clone(),
                    m.shard_iterators.get(SHARD_ID).cloned(),
                ))
            })
            .collect();

        for (
            uuid,
            event_source_arn,
            function_arn,
            batch_size,
            starting_position,
            starting_position_timestamp,
            filter_criteria,
            dlq_arn,
            saved_iterator,
        ) in mappings
        {
            let stream_name = event_source_arn.split('/').next_back().unwrap_or("");
            if stream_name.is_empty() {
                continue;
            }
            let parts: Vec<&str> = event_source_arn.splitn(6, ':').collect();
            let stream_region = if parts.len() >= 4 { parts[3] } else { &region };
            let kinesis_ctx =
                RequestContext::new_with_account("kinesis", stream_region, &account_id);

            let iterator = match saved_iterator {
                Some(it) => it,
                None => {
                    let iter_type = starting_position.as_deref().unwrap_or("TRIM_HORIZON");
                    let mut iter_input = serde_json::json!({
                        "StreamName": stream_name,
                        "ShardId": SHARD_ID,
                        "ShardIteratorType": iter_type,
                    });
                    if iter_type == "AT_TIMESTAMP"
                        && let Some(ts) = starting_position_timestamp
                    {
                        iter_input["Timestamp"] = serde_json::json!(ts);
                    }
                    match kinesis
                        .handle("GetShardIterator", iter_input, &kinesis_ctx)
                        .await
                    {
                        Ok(r) => match r["ShardIterator"].as_str() {
                            Some(s) => s.to_string(),
                            None => continue,
                        },
                        Err(e) => {
                            warn!(stream = stream_name, error = %e.message, "Kinesis->Lambda: GetShardIterator failed");
                            continue;
                        }
                    }
                }
            };

            let records_input = serde_json::json!({
                "ShardIterator": iterator,
                "Limit": batch_size,
            });
            let records_result = match kinesis
                .handle("GetRecords", records_input, &kinesis_ctx)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(stream = stream_name, error = %e.message, "Kinesis->Lambda: GetRecords failed");
                    set_last_result(
                        &state,
                        &uuid,
                        &format!("PROBLEM: GetRecords failed: {}", e.message),
                    );
                    continue;
                }
            };

            // Always advance to NextShardIterator if the Kinesis service supplied one.
            // Empty batches still need to advance, otherwise we'd starve when the stream
            // has no records and never see new ones.
            if let Some(next) = records_result["NextShardIterator"].as_str()
                && let Some(mut m) = state.event_source_mappings.get_mut(&uuid)
            {
                m.shard_iterators
                    .insert(SHARD_ID.to_string(), next.to_string());
            }

            let records = match records_result["Records"].as_array() {
                Some(r) if !r.is_empty() => r.clone(),
                _ => {
                    set_last_result(&state, &uuid, "OK");
                    continue;
                }
            };

            let (kept, _filtered) =
                esm::partition_by_filter(&records, filter_criteria.as_ref(), |_| None);
            if kept.is_empty() {
                set_last_result(&state, &uuid, "OK");
                continue;
            }

            let lambda_event = serde_json::json!({ "Records": kept });
            let invoke_input = serde_json::json!({
                "FunctionName": function_arn,
                "Payload": serde_json::to_string(&lambda_event).unwrap_or_default(),
                "InvocationType": "Event",
            });
            let lambda_ctx = RequestContext::new_with_account("lambda", &region, &account_id);
            match lambda.handle("Invoke", invoke_input, &lambda_ctx).await {
                Ok(_) => {
                    debug!(
                        function = %function_arn,
                        stream = stream_name,
                        account = %account_id,
                        region = %region,
                        count = kept.len(),
                        "Kinesis->Lambda: delivered batch"
                    );
                    set_last_result(&state, &uuid, "OK");
                }
                Err(e) => {
                    warn!(
                        function = %function_arn,
                        stream = stream_name,
                        error = %e.message,
                        "Kinesis->Lambda: invocation failed"
                    );
                    if let Some(dlq) = &dlq_arn {
                        esm::route_to_destination(
                            services,
                            dlq,
                            &lambda_event,
                            &account_id,
                            &region,
                        )
                        .await;
                    }
                    set_last_result(
                        &state,
                        &uuid,
                        &format!("PROBLEM: invoke failed: {}", e.message),
                    );
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
        partition: awsim_core::DEFAULT_PARTITION.to_string(),
        service: "cloudformation".to_string(),
        access_key: None,
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        uri: "/".to_string(),
        event_bus: None,
        source_ip: None,
        is_secure: false,
        internal_bypass: false,
    };

    match resource_type {
        "AWS::S3::Bucket" => {
            if let Some(s3) = services.get("s3") {
                let bucket_name = properties["BucketName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        format!("cf-bucket-{}", &uuid::Uuid::new_v4().to_string()[..8])
                    });
                let input = serde_json::json!({ "Bucket": bucket_name });
                match s3.handle("CreateBucket", input, &ctx).await {
                    Ok(_) => info!(bucket = %bucket_name, "CloudFormation created S3 bucket"),
                    Err(e) => {
                        warn!(bucket = %bucket_name, error = %e.message, "CloudFormation S3 bucket creation failed")
                    }
                }
            }
        }
        "AWS::SQS::Queue" => {
            if let Some(sqs) = services.get("sqs") {
                let queue_name = properties["QueueName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        format!("cf-queue-{}", &uuid::Uuid::new_v4().to_string()[..8])
                    });
                let input = serde_json::json!({ "QueueName": queue_name });
                match sqs.handle("CreateQueue", input, &ctx).await {
                    Ok(_) => info!(queue = %queue_name, "CloudFormation created SQS queue"),
                    Err(e) => {
                        warn!(queue = %queue_name, error = %e.message, "CloudFormation SQS queue creation failed")
                    }
                }
            }
        }
        "AWS::SNS::Topic" => {
            if let Some(sns) = services.get("sns") {
                let topic_name = properties["TopicName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        format!("cf-topic-{}", &uuid::Uuid::new_v4().to_string()[..8])
                    });
                let input = serde_json::json!({ "Name": topic_name });
                match sns.handle("CreateTopic", input, &ctx).await {
                    Ok(_) => info!(topic = %topic_name, "CloudFormation created SNS topic"),
                    Err(e) => {
                        warn!(topic = %topic_name, error = %e.message, "CloudFormation SNS topic creation failed")
                    }
                }
            }
        }
        "AWS::DynamoDB::Table" => {
            if let Some(dynamodb) = services.get("dynamodb") {
                match dynamodb
                    .handle("CreateTable", properties.clone(), &ctx)
                    .await
                {
                    Ok(_) => info!("CloudFormation created DynamoDB table"),
                    Err(e) => {
                        warn!(error = %e.message, "CloudFormation DynamoDB table creation failed")
                    }
                }
            }
        }
        "AWS::IAM::Role" => {
            if let Some(iam) = services.get("iam") {
                let role_name = properties["RoleName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        format!("cf-role-{}", &uuid::Uuid::new_v4().to_string()[..8])
                    });
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
                    Err(e) => {
                        warn!(role = %role_name, error = %e.message, "CloudFormation IAM role creation failed")
                    }
                }
            }
        }
        "AWS::Lambda::Function" => {
            if let Some(lambda) = services.get("lambda") {
                match lambda
                    .handle("CreateFunction", properties.clone(), &ctx)
                    .await
                {
                    Ok(_) => info!("CloudFormation created Lambda function"),
                    Err(e) => {
                        warn!(error = %e.message, "CloudFormation Lambda function creation failed")
                    }
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
                    Ok(_) => {
                        info!(log_group = %name, "CloudFormation created CloudWatch log group")
                    }
                    Err(e) => {
                        warn!(log_group = %name, error = %e.message, "CloudFormation log group creation failed")
                    }
                }
            }
        }
        "AWS::IAM::Policy" => {
            if let Some(iam) = services.get("iam") {
                let policy_name = properties["PolicyName"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        format!("cf-policy-{}", &uuid::Uuid::new_v4().to_string()[..8])
                    });
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
                    Err(e) => {
                        warn!(policy = %policy_name, error = %e.message, "CloudFormation IAM policy creation failed")
                    }
                }
            }
        }
        "AWS::Kinesis::Stream" => {
            if let Some(kinesis) = services.get("kinesis") {
                let stream_name = properties["Name"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        format!("cf-stream-{}", &uuid::Uuid::new_v4().to_string()[..8])
                    });
                let shard_count = properties["ShardCount"].as_u64().unwrap_or(1);
                let input = serde_json::json!({
                    "StreamName": stream_name,
                    "ShardCount": shard_count,
                });
                match kinesis.handle("CreateStream", input, &ctx).await {
                    Ok(_) => info!(stream = %stream_name, "CloudFormation created Kinesis stream"),
                    Err(e) => {
                        warn!(stream = %stream_name, error = %e.message, "CloudFormation Kinesis stream creation failed")
                    }
                }
            }
        }
        "AWS::SSM::Parameter" => {
            if let Some(ssm) = services.get("ssm") {
                let name = properties["Name"].as_str().unwrap_or("/cf/parameter");
                let param_type = properties["Type"].as_str().unwrap_or("String");
                let value = properties["Value"].as_str().unwrap_or("");
                let input = serde_json::json!({
                    "Name": name,
                    "Type": param_type,
                    "Value": value,
                });
                match ssm.handle("PutParameter", input, &ctx).await {
                    Ok(_) => info!(param = %name, "CloudFormation created SSM parameter"),
                    Err(e) => {
                        warn!(param = %name, error = %e.message, "CloudFormation SSM parameter creation failed")
                    }
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
    let physical_id = event.detail["physicalResourceId"].as_str().unwrap_or("");

    let ctx = RequestContext {
        account_id: event.account_id.clone(),
        region: event.region.clone(),
        partition: awsim_core::DEFAULT_PARTITION.to_string(),
        service: "cloudformation".to_string(),
        access_key: None,
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        uri: "/".to_string(),
        event_bus: None,
        source_ip: None,
        is_secure: false,
        internal_bypass: false,
    };

    match resource_type {
        "AWS::S3::Bucket" => {
            if let Some(s3) = services.get("s3") {
                // physical_id for S3 is the bucket name
                let input = serde_json::json!({ "Bucket": physical_id });
                match s3.handle("DeleteBucket", input, &ctx).await {
                    Ok(_) => info!(bucket = %physical_id, "CloudFormation deleted S3 bucket"),
                    Err(e) => {
                        warn!(bucket = %physical_id, error = %e.message, "CloudFormation S3 bucket deletion failed")
                    }
                }
            }
        }
        "AWS::SQS::Queue" => {
            if let Some(sqs) = services.get("sqs") {
                // For SQS the physical ID is a queue URL
                let input = serde_json::json!({ "QueueUrl": physical_id });
                match sqs.handle("DeleteQueue", input, &ctx).await {
                    Ok(_) => info!(queue = %physical_id, "CloudFormation deleted SQS queue"),
                    Err(e) => {
                        warn!(queue = %physical_id, error = %e.message, "CloudFormation SQS queue deletion failed")
                    }
                }
            }
        }
        "AWS::SNS::Topic" => {
            if let Some(sns) = services.get("sns") {
                let input = serde_json::json!({ "TopicArn": physical_id });
                match sns.handle("DeleteTopic", input, &ctx).await {
                    Ok(_) => info!(topic = %physical_id, "CloudFormation deleted SNS topic"),
                    Err(e) => {
                        warn!(topic = %physical_id, error = %e.message, "CloudFormation SNS topic deletion failed")
                    }
                }
            }
        }
        "AWS::DynamoDB::Table" => {
            if let Some(dynamodb) = services.get("dynamodb") {
                let input = serde_json::json!({ "TableName": physical_id });
                match dynamodb.handle("DeleteTable", input, &ctx).await {
                    Ok(_) => info!(table = %physical_id, "CloudFormation deleted DynamoDB table"),
                    Err(e) => {
                        warn!(table = %physical_id, error = %e.message, "CloudFormation DynamoDB table deletion failed")
                    }
                }
            }
        }
        "AWS::IAM::Role" => {
            if let Some(iam) = services.get("iam") {
                let input = serde_json::json!({ "RoleName": physical_id });
                match iam.handle("DeleteRole", input, &ctx).await {
                    Ok(_) => info!(role = %physical_id, "CloudFormation deleted IAM role"),
                    Err(e) => {
                        warn!(role = %physical_id, error = %e.message, "CloudFormation IAM role deletion failed")
                    }
                }
            }
        }
        "AWS::Lambda::Function" => {
            if let Some(lambda) = services.get("lambda") {
                let input = serde_json::json!({ "FunctionName": physical_id });
                match lambda.handle("DeleteFunction", input, &ctx).await {
                    Ok(_) => {
                        info!(function = %physical_id, "CloudFormation deleted Lambda function")
                    }
                    Err(e) => {
                        warn!(function = %physical_id, error = %e.message, "CloudFormation Lambda function deletion failed")
                    }
                }
            }
        }
        "AWS::Logs::LogGroup" => {
            if let Some(logs) = services.get("logs") {
                let input = serde_json::json!({ "logGroupName": physical_id });
                match logs.handle("DeleteLogGroup", input, &ctx).await {
                    Ok(_) => {
                        info!(log_group = %physical_id, "CloudFormation deleted CloudWatch log group")
                    }
                    Err(e) => {
                        warn!(log_group = %physical_id, error = %e.message, "CloudFormation log group deletion failed")
                    }
                }
            }
        }
        "AWS::IAM::Policy" => {
            if let Some(iam) = services.get("iam") {
                let input = serde_json::json!({ "PolicyArn": physical_id });
                match iam.handle("DeletePolicy", input, &ctx).await {
                    Ok(_) => info!(policy = %physical_id, "CloudFormation deleted IAM policy"),
                    Err(e) => {
                        warn!(policy = %physical_id, error = %e.message, "CloudFormation IAM policy deletion failed")
                    }
                }
            }
        }
        "AWS::Kinesis::Stream" => {
            if let Some(kinesis) = services.get("kinesis") {
                let input = serde_json::json!({ "StreamName": physical_id });
                match kinesis.handle("DeleteStream", input, &ctx).await {
                    Ok(_) => info!(stream = %physical_id, "CloudFormation deleted Kinesis stream"),
                    Err(e) => {
                        warn!(stream = %physical_id, error = %e.message, "CloudFormation Kinesis stream deletion failed")
                    }
                }
            }
        }
        "AWS::SSM::Parameter" => {
            if let Some(ssm) = services.get("ssm") {
                let input = serde_json::json!({ "Name": physical_id });
                match ssm.handle("DeleteParameter", input, &ctx).await {
                    Ok(_) => info!(param = %physical_id, "CloudFormation deleted SSM parameter"),
                    Err(e) => {
                        warn!(param = %physical_id, error = %e.message, "CloudFormation SSM parameter deletion failed")
                    }
                }
            }
        }
        other => {
            debug!(resource_type = %other, "Unsupported CloudFormation resource type — skipping delete");
        }
    }
}

/// Handle a `cognito:LambdaTrigger` event by invoking the configured Lambda
/// function with the trigger payload.
pub async fn handle_cognito_trigger(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    event: &InternalEvent,
) {
    let lambda = match services.get("lambda") {
        Some(l) => l,
        None => return,
    };

    let arn = event.detail["functionArn"].as_str().unwrap_or("");
    let trigger_event = &event.detail["event"];
    let trigger_source = event.detail["triggerSource"].as_str().unwrap_or("");

    // Extract function name from the ARN: arn:aws:lambda:{region}:{account}:function:{name}
    let func_name = if arn.contains(":function:") {
        arn.split(":function:").last().unwrap_or(arn)
    } else {
        arn
    };

    let input = serde_json::json!({
        "FunctionName": func_name,
        "Payload": serde_json::to_string(trigger_event).unwrap_or_default(),
        "InvocationType": "Event",
    });

    let ctx = RequestContext::new("lambda", &event.region);
    match lambda.handle("Invoke", input, &ctx).await {
        Ok(_) => info!(
            function = %func_name,
            trigger = %trigger_source,
            "Cognito trigger → Lambda invocation delivered"
        ),
        Err(e) => warn!(
            function = %func_name,
            trigger = %trigger_source,
            error = %e.message,
            "Cognito trigger → Lambda invocation failed"
        ),
    }
}
