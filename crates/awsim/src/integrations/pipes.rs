//! Pipes runner: drives RUNNING pipes by polling their source and forwarding
//! batches to the target. Currently supports SQS-as-source and Lambda /
//! StepFunctions / SQS / SNS as targets — enough to cover the common
//! "queue → processor" integration pattern.

use std::collections::HashMap;
use std::sync::Arc;

use awsim_core::{AccountRegionStore, RequestContext, ServiceHandler};
use awsim_pipes::state::PipesState;
use serde_json::{Value, json};
use tracing::{debug, warn};

use super::esm;

/// Snapshot of fields the pipes runner needs from a Pipe entry.
type PipeSnapshot = (
    String,         // name
    String,         // source
    String,         // target
    Option<Value>,  // source_parameters
    Option<Value>,  // target_parameters
    Option<String>, // enrichment
);

pub async fn run_pipes_once(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    pipes_store: &AccountRegionStore<PipesState>,
) {
    for ((account_id, region), state) in pipes_store.iter_all() {
        let snapshots: Vec<PipeSnapshot> = state
            .pipes
            .iter()
            .filter_map(|e| {
                let p = e.value();
                if p.current_state != "RUNNING" {
                    return None;
                }
                Some((
                    p.name.clone(),
                    p.source.clone(),
                    p.target.clone(),
                    p.source_parameters.clone(),
                    p.target_parameters.clone(),
                    p.enrichment.clone(),
                ))
            })
            .collect();

        for (name, source_arn, target_arn, source_params, _target_params, enrichment) in snapshots {
            if !source_arn.contains(":sqs:") {
                debug!(
                    pipe = %name,
                    source = %source_arn,
                    "pipes runner: only SQS sources are supported"
                );
                continue;
            }
            forward_sqs_pipe(
                services,
                &name,
                &source_arn,
                &target_arn,
                source_params.as_ref(),
                enrichment.as_deref(),
                &account_id,
                &region,
            )
            .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn forward_sqs_pipe(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    name: &str,
    source_arn: &str,
    target_arn: &str,
    source_parameters: Option<&Value>,
    enrichment: Option<&str>,
    account_id: &str,
    region: &str,
) {
    let Some(sqs) = services.get("sqs") else {
        return;
    };

    let parts: Vec<&str> = source_arn.split(':').collect();
    if parts.len() < 6 {
        return;
    }
    let queue_region = parts[3];
    let queue_account = parts[4];
    let queue_name = parts[5];
    let queue_url =
        format!("http://sqs.{queue_region}.localhost:4566/{queue_account}/{queue_name}");

    let batch_size = source_parameters
        .and_then(|sp| sp.get("SqsQueueParameters"))
        .and_then(|q| q.get("BatchSize"))
        .and_then(|v| v.as_u64())
        .unwrap_or(10);

    let sqs_ctx = RequestContext::new_with_account("sqs", queue_region, queue_account);
    let receive_input = json!({
        "QueueUrl": queue_url,
        "MaxNumberOfMessages": batch_size,
        "WaitTimeSeconds": 0,
    });
    let received = match sqs.handle("ReceiveMessage", receive_input, &sqs_ctx).await {
        Ok(r) => r,
        Err(_) => return,
    };
    let messages = match received["Messages"].as_array() {
        Some(m) if !m.is_empty() => m.clone(),
        _ => return,
    };

    let raw_records: Vec<Value> = messages
        .iter()
        .map(|m| {
            json!({
                "messageId": m["MessageId"],
                "receiptHandle": m["ReceiptHandle"],
                "body": m["Body"],
                "attributes": m.get("Attributes").unwrap_or(&Value::Object(Default::default())),
                "messageAttributes": m.get("MessageAttributes").unwrap_or(&Value::Object(Default::default())),
                "md5OfBody": m["MD5OfBody"],
                "eventSource": "aws:sqs",
                "eventSourceARN": source_arn,
                "awsRegion": queue_region,
            })
        })
        .collect();

    let filter_criteria = source_parameters.and_then(|sp| sp.get("FilterCriteria"));
    let (kept, filtered_handles) = esm::partition_by_filter(&raw_records, filter_criteria, |r| {
        r.get("receiptHandle")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });

    for handle in &filtered_handles {
        let _ = sqs
            .handle(
                "DeleteMessage",
                json!({ "QueueUrl": queue_url, "ReceiptHandle": handle }),
                &sqs_ctx,
            )
            .await;
    }

    if kept.is_empty() {
        return;
    }

    // Optional enrichment: invoke a Lambda with the batch and use its
    // response as the next-stage payload. Errors short-circuit and leave
    // messages in the queue so the next tick retries.
    let payload = match enrichment {
        Some(arn) if arn.contains(":function:") => {
            let Some(lambda) = services.get("lambda") else {
                return;
            };
            let func_name = arn.rsplit(":function:").next().unwrap_or(arn);
            let lambda_ctx = RequestContext::new_with_account("lambda", region, account_id);
            let invoke_input = json!({
                "FunctionName": func_name,
                "InvocationType": "RequestResponse",
                "Payload": serde_json::to_string(&kept).unwrap_or_else(|_| "[]".to_string()),
            });
            match lambda.handle("Invoke", invoke_input, &lambda_ctx).await {
                Ok(r) => r
                    .get("Payload")
                    .and_then(|v| v.as_str())
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .unwrap_or_else(|| Value::Array(kept.clone())),
                Err(e) => {
                    warn!(pipe = name, error = %e.message, "pipes runner: enrichment failed");
                    return;
                }
            }
        }
        Some(other) => {
            debug!(
                pipe = name,
                enrichment = other,
                "pipes runner: enrichment ARN type unsupported, skipping"
            );
            Value::Array(kept.clone())
        }
        None => Value::Array(kept.clone()),
    };

    let delivered = dispatch_to_target(services, target_arn, &payload, account_id, region).await;
    if delivered {
        for rec in &kept {
            if let Some(handle) = rec.get("receiptHandle").and_then(|v| v.as_str()) {
                let _ = sqs
                    .handle(
                        "DeleteMessage",
                        json!({ "QueueUrl": queue_url, "ReceiptHandle": handle }),
                        &sqs_ctx,
                    )
                    .await;
            }
        }
        debug!(
            pipe = name,
            target = target_arn,
            count = kept.len(),
            "pipes runner: delivered batch"
        );
    } else {
        warn!(
            pipe = name,
            target = target_arn,
            "pipes runner: target dispatch failed; messages remain in source queue"
        );
    }
}

async fn dispatch_to_target(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    target_arn: &str,
    payload: &Value,
    account_id: &str,
    region: &str,
) -> bool {
    if target_arn.contains(":function:") {
        let Some(lambda) = services.get("lambda") else {
            return false;
        };
        let func_name = target_arn.rsplit(":function:").next().unwrap_or(target_arn);
        let ctx = RequestContext::new_with_account("lambda", region, account_id);
        let input = json!({
            "FunctionName": func_name,
            "InvocationType": "Event",
            "Payload": serde_json::to_string(payload).unwrap_or_else(|_| "[]".to_string()),
        });
        return lambda.handle("Invoke", input, &ctx).await.is_ok();
    }
    if target_arn.contains(":states:") && target_arn.contains(":stateMachine:") {
        let Some(sfn) = services.get("stepfunctions") else {
            return false;
        };
        let ctx = RequestContext::new_with_account("stepfunctions", region, account_id);
        let input = json!({
            "stateMachineArn": target_arn,
            "input": serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string()),
        });
        return sfn.handle("StartExecution", input, &ctx).await.is_ok();
    }
    if target_arn.contains(":sqs:") {
        let Some(sqs) = services.get("sqs") else {
            return false;
        };
        let parts: Vec<&str> = target_arn.split(':').collect();
        if parts.len() < 6 {
            return false;
        }
        let queue_url = format!(
            "http://sqs.{}.localhost:4566/{}/{}",
            parts[3], parts[4], parts[5]
        );
        let ctx = RequestContext::new_with_account("sqs", region, account_id);
        let input = json!({
            "QueueUrl": queue_url,
            "MessageBody": payload.to_string(),
        });
        return sqs.handle("SendMessage", input, &ctx).await.is_ok();
    }
    if target_arn.contains(":sns:") {
        let Some(sns) = services.get("sns") else {
            return false;
        };
        let ctx = RequestContext::new_with_account("sns", region, account_id);
        let input = json!({
            "TopicArn": target_arn,
            "Message": payload.to_string(),
        });
        return sns.handle("Publish", input, &ctx).await.is_ok();
    }
    warn!(target = target_arn, "pipes runner: unsupported target ARN");
    false
}
