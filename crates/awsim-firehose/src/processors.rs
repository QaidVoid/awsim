//! Firehose data-transformation processors. Today only the `Lambda`
//! processor is executed: the configured function is invoked with the
//! AWS Firehose transform contract `{records:[{recordId,data}]}` and is
//! expected to return `{records:[{recordId,result,data}]}` where result
//! is `Ok` / `Dropped` / `ProcessingFailed`. Records that fail (or a
//! whole batch when the invoke itself errors) are routed to the S3
//! backup (ErrorOutputPrefix) by the delivery path.

use awsim_core::LambdaInvoker;
use serde_json::{Value, json};

/// Split of a processed batch: `transformed` records deliver to the main
/// prefix, `failed` records to the ErrorOutputPrefix, `dropped` are
/// silently discarded (but counted).
pub struct ProcessOutcome {
    pub transformed: Vec<String>,
    pub failed: Vec<String>,
    pub dropped: u64,
}

/// Invoke the Lambda transform over base64 record data. On an invoke or
/// FunctionError the entire batch is treated as failed (AWS retries then
/// backs up; we do the conservative single-shot equivalent). A
/// well-formed response splits records by their per-record `result`.
pub fn run_processors(
    invoker: &dyn LambdaInvoker,
    lambda_arn: &str,
    records: &[String],
    account: &str,
    region: &str,
) -> ProcessOutcome {
    let payload = json!({
        "records": records
            .iter()
            .enumerate()
            .map(|(i, d)| json!({ "recordId": format!("r{i}"), "data": d }))
            .collect::<Vec<_>>(),
    });

    match invoker.invoke(lambda_arn, &payload, account, region) {
        Err(_) => ProcessOutcome {
            transformed: Vec::new(),
            failed: records.to_vec(),
            dropped: 0,
        },
        Ok(resp) => {
            let mut out = ProcessOutcome {
                transformed: Vec::new(),
                failed: Vec::new(),
                dropped: 0,
            };
            let by_id: std::collections::HashMap<String, String> = records
                .iter()
                .enumerate()
                .map(|(i, d)| (format!("r{i}"), d.clone()))
                .collect();
            match resp.get("records").and_then(Value::as_array) {
                // Malformed/empty response: deliver everything unchanged.
                None => out.transformed = records.to_vec(),
                Some(returned) => {
                    for rec in returned {
                        let id = rec.get("recordId").and_then(Value::as_str).unwrap_or("");
                        let orig = by_id.get(id).cloned().unwrap_or_default();
                        match rec.get("result").and_then(Value::as_str).unwrap_or("Ok") {
                            "Dropped" => out.dropped += 1,
                            "ProcessingFailed" => out.failed.push(orig),
                            _ => out.transformed.push(
                                rec.get("data")
                                    .and_then(Value::as_str)
                                    .map(String::from)
                                    .unwrap_or(orig),
                            ),
                        }
                    }
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use awsim_core::AwsError;
    use std::sync::Mutex;

    struct MockInvoker {
        response: Mutex<Result<Value, ()>>,
    }

    impl LambdaInvoker for MockInvoker {
        fn invoke(
            &self,
            _function_name: &str,
            _payload: &Value,
            _account: &str,
            _region: &str,
        ) -> Result<Value, AwsError> {
            match &*self.response.lock().unwrap() {
                Ok(v) => Ok(v.clone()),
                Err(_) => Err(AwsError::bad_request("LambdaInvocationError", "boom")),
            }
        }
    }

    fn records() -> Vec<String> {
        vec!["aGVsbG8=".to_string(), "d29ybGQ=".to_string()]
    }

    #[test]
    fn splits_ok_failed_and_dropped() {
        let inv = MockInvoker {
            response: Mutex::new(Ok(json!({ "records": [
                { "recordId": "r0", "result": "Ok", "data": "transformed0" },
                { "recordId": "r1", "result": "ProcessingFailed" },
            ]}))),
        };
        let out = run_processors(&inv, "arn", &records(), "000000000000", "us-east-1");
        assert_eq!(out.transformed, vec!["transformed0".to_string()]);
        assert_eq!(out.failed, vec!["d29ybGQ=".to_string()]);
        assert_eq!(out.dropped, 0);
    }

    #[test]
    fn dropped_result_counts_and_excludes() {
        let inv = MockInvoker {
            response: Mutex::new(Ok(json!({ "records": [
                { "recordId": "r0", "result": "Dropped" },
                { "recordId": "r1", "result": "Ok", "data": "kept" },
            ]}))),
        };
        let out = run_processors(&inv, "arn", &records(), "000000000000", "us-east-1");
        assert_eq!(out.transformed, vec!["kept".to_string()]);
        assert!(out.failed.is_empty());
        assert_eq!(out.dropped, 1);
    }

    #[test]
    fn invoke_error_routes_whole_batch_to_failed() {
        let inv = MockInvoker {
            response: Mutex::new(Err(())),
        };
        let out = run_processors(&inv, "arn", &records(), "000000000000", "us-east-1");
        assert!(out.transformed.is_empty());
        assert_eq!(out.failed, records());
    }
}
