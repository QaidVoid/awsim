//! Helpers shared by the SQS / Kinesis / DynamoDB stream poller paths:
//! Lambda-style FilterCriteria evaluation and DestinationConfig.OnFailure
//! routing (DLQ to SQS or SNS).

use std::collections::HashMap;
use std::sync::Arc;

use awsim_core::{RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::{info, warn};

/// Splits a record list into (kept, filtered_handles) according to
/// `filter_criteria` (the raw FilterCriteria JSON object stored on the
/// EventSourceMapping). Records with no matching filter are dropped; the
/// caller uses `extract_handle` to capture an opaque token (e.g. an SQS
/// receipt handle) that lets it acknowledge the dropped record on the source.
///
/// Pass `None` for `filter_criteria` to keep every record.
pub fn partition_by_filter<F>(
    records: &[Value],
    filter_criteria: Option<&Value>,
    extract_handle: F,
) -> (Vec<Value>, Vec<String>)
where
    F: Fn(&Value) -> Option<String>,
{
    let filters = match filter_criteria.and_then(|fc| fc.get("Filters")?.as_array()) {
        Some(f) if !f.is_empty() => f,
        _ => return (records.to_vec(), Vec::new()),
    };

    let patterns: Vec<Value> = filters
        .iter()
        .filter_map(|f| f.get("Pattern")?.as_str())
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect();

    if patterns.is_empty() {
        return (records.to_vec(), Vec::new());
    }

    let mut kept = Vec::new();
    let mut filtered = Vec::new();
    for rec in records {
        if patterns.iter().any(|p| matches_pattern(p, rec)) {
            kept.push(rec.clone());
        } else if let Some(h) = extract_handle(rec) {
            filtered.push(h);
        }
    }
    (kept, filtered)
}

/// Recursively match an EventBridge-style content pattern against a record.
///
/// Supported leaf forms: literal scalars, arrays (any-of), and the operator
/// objects `{"exists": bool}`, `{"prefix": "..."}`, `{"suffix": "..."}`,
/// `{"anything-but": [...]}`, `{"numeric": ["<", N, ...]}`. Unknown operators
/// behave as no-match. This is enough for the common Lambda FilterCriteria
/// idioms; complex `cidr` / `wildcard` matching is deliberately unimplemented.
fn matches_pattern(pattern: &Value, record: &Value) -> bool {
    match pattern {
        Value::Object(map) => map.iter().all(|(k, sub_pat)| {
            // {"exists": bool} at this level controls whether the key must be present.
            // Otherwise recurse into the same key on the record.
            let sub_record = record.get(k);
            match sub_record {
                Some(v) => matches_pattern(sub_pat, v),
                None => is_exists_false(sub_pat),
            }
        }),
        Value::Array(alternatives) => alternatives.iter().any(|alt| matches_leaf(alt, record)),
        // Literal scalar at non-leaf — only matches if the record is the same scalar.
        _ => pattern == record,
    }
}

fn matches_leaf(alt: &Value, value: &Value) -> bool {
    if alt == value {
        return true;
    }
    let Some(op_obj) = alt.as_object() else {
        return false;
    };
    if let Some(b) = op_obj.get("exists").and_then(|v| v.as_bool()) {
        return b;
    }
    if let Some(prefix) = op_obj.get("prefix").and_then(|v| v.as_str())
        && let Some(s) = value.as_str()
    {
        return s.starts_with(prefix);
    }
    if let Some(suffix) = op_obj.get("suffix").and_then(|v| v.as_str())
        && let Some(s) = value.as_str()
    {
        return s.ends_with(suffix);
    }
    if let Some(arr) = op_obj.get("anything-but").and_then(|v| v.as_array()) {
        return !arr.iter().any(|x| x == value);
    }
    if let Some(arr) = op_obj.get("numeric").and_then(|v| v.as_array())
        && let Some(n) = value.as_f64()
    {
        return numeric_match(arr, n);
    }
    false
}

fn is_exists_false(pat: &Value) -> bool {
    pat.as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("exists"))
        .and_then(|v| v.as_bool())
        == Some(false)
}

/// Evaluate the `["<", 1, ">", 2]` style numeric comparison list against `n`.
fn numeric_match(spec: &[Value], n: f64) -> bool {
    let mut i = 0;
    while i < spec.len() {
        let op = match spec[i].as_str() {
            Some(s) => s,
            None => return false,
        };
        let bound = match spec.get(i + 1).and_then(|v| v.as_f64()) {
            Some(b) => b,
            None => return false,
        };
        let ok = match op {
            "=" => n == bound,
            "!=" => n != bound,
            "<" => n < bound,
            "<=" => n <= bound,
            ">" => n > bound,
            ">=" => n >= bound,
            _ => return false,
        };
        if !ok {
            return false;
        }
        i += 2;
    }
    true
}

/// Forward a failed batch to a DestinationConfig.OnFailure ARN (SQS or SNS).
/// Best-effort: failures are logged but do not propagate.
pub async fn route_to_destination(
    services: &HashMap<String, Arc<dyn ServiceHandler>>,
    arn: &str,
    payload: &Value,
    account_id: &str,
    region: &str,
) {
    if arn.contains(":sqs:") {
        let Some(sqs) = services.get("sqs") else {
            return;
        };
        let parts: Vec<&str> = arn.split(':').collect();
        if parts.len() < 6 {
            return;
        }
        let queue_url = format!(
            "http://sqs.{}.localhost:4566/{}/{}",
            parts[3], parts[4], parts[5]
        );
        let ctx = RequestContext::new_with_account("sqs", region, account_id);
        let input = serde_json::json!({
            "QueueUrl": queue_url,
            "MessageBody": payload.to_string(),
        });
        match sqs.handle("SendMessage", input, &ctx).await {
            Ok(_) => info!(dlq = arn, "ESM->DLQ: failed batch routed to SQS"),
            Err(e) => warn!(dlq = arn, error = %e.message, "ESM->DLQ: SQS send failed"),
        }
    } else if arn.contains(":sns:") {
        let Some(sns) = services.get("sns") else {
            return;
        };
        let ctx = RequestContext::new_with_account("sns", region, account_id);
        let input = serde_json::json!({
            "TopicArn": arn,
            "Message": payload.to_string(),
        });
        match sns.handle("Publish", input, &ctx).await {
            Ok(_) => info!(dlq = arn, "ESM->DLQ: failed batch published to SNS"),
            Err(e) => warn!(dlq = arn, error = %e.message, "ESM->DLQ: SNS publish failed"),
        }
    } else {
        warn!(dlq = arn, "ESM->DLQ: unsupported destination ARN");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn equality_pattern_matches() {
        let pattern = json!({ "body": { "type": ["new"] } });
        let record = json!({ "body": { "type": "new", "id": 1 } });
        assert!(matches_pattern(&pattern, &record));
    }

    #[test]
    fn equality_pattern_rejects_other_value() {
        let pattern = json!({ "body": { "type": ["new"] } });
        let record = json!({ "body": { "type": "old" } });
        assert!(!matches_pattern(&pattern, &record));
    }

    #[test]
    fn prefix_operator() {
        let pattern = json!({ "body": { "id": [{ "prefix": "ord_" }] } });
        assert!(matches_pattern(
            &pattern,
            &json!({ "body": { "id": "ord_42" } })
        ));
        assert!(!matches_pattern(
            &pattern,
            &json!({ "body": { "id": "usr_42" } })
        ));
    }

    #[test]
    fn exists_false_matches_missing_key() {
        let pattern = json!({ "body": { "deleted": [{ "exists": false }] } });
        assert!(matches_pattern(&pattern, &json!({ "body": {} })));
        assert!(!matches_pattern(
            &pattern,
            &json!({ "body": { "deleted": true } })
        ));
    }

    #[test]
    fn numeric_range() {
        let pattern = json!({ "body": { "amount": [{ "numeric": [">=", 10, "<", 100] }] } });
        assert!(matches_pattern(
            &pattern,
            &json!({ "body": { "amount": 50 } })
        ));
        assert!(!matches_pattern(
            &pattern,
            &json!({ "body": { "amount": 200 } })
        ));
    }

    #[test]
    fn partition_keeps_when_no_filter() {
        let recs = vec![json!({ "body": "a" }), json!({ "body": "b" })];
        let (kept, filt) = partition_by_filter(&recs, None, |_| Some("h".to_string()));
        assert_eq!(kept.len(), 2);
        assert!(filt.is_empty());
    }

    #[test]
    fn partition_drops_non_matching() {
        let fc = json!({
            "Filters": [
                { "Pattern": "{\"body\": [\"keep\"]}" }
            ]
        });
        let recs = vec![
            json!({ "body": "keep", "rh": "1" }),
            json!({ "body": "drop", "rh": "2" }),
        ];
        let (kept, filt) = partition_by_filter(&recs, Some(&fc), |r| {
            r.get("rh").and_then(|v| v.as_str()).map(|s| s.to_string())
        });
        assert_eq!(kept.len(), 1);
        assert_eq!(filt, vec!["2".to_string()]);
    }
}
