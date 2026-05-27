use std::collections::BTreeMap;

use awsim_core::{AwsError, RequestContext};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::TaggingState;

/// Per-call paging cursor — opaque to the caller, base64-encoded JSON for us.
#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    /// Number of results already returned.
    skipped: usize,
}

/// `GetResources` — list tagged resources, optionally filtered by tag.
///
/// Supported inputs:
///   * `TagFilters: [{ Key, Values? }]` — match resources whose tags include
///     the key, and (when provided) one of the given values.
///   * `ResourceTypeFilters: [String]` — match resources whose ARN service
///     segment matches any of the given strings.
///   * `ResourcesPerPage` — caller-supplied page size, clamped to 100.
///   * `PaginationToken` — opaque cursor returned by a previous call.
///
/// `IncludeComplianceDetails` and `ExcludeCompliantResources` are accepted but
/// not enforced — the emulator has no compliance signal.
pub fn get_resources(
    state: &TaggingState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let tag_filters = parse_tag_filters(input.get("TagFilters"))?;
    // ResourceTypeFilters: AWS matches case-sensitively against the
    // canonical service / `service:resource-type` form. Trim only —
    // do not lowercase, because callers that pass `ec2:Instance`
    // expect a different match set than `ec2:instance`.
    let type_filters: Vec<String> = input
        .get("ResourceTypeFilters")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let page_size = input
        .get("ResourcesPerPage")
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 100) as usize)
        .unwrap_or(50);

    let skip = decode_cursor(input.get("PaginationToken"))?;

    // Stable ordering — sort ARNs alphabetically so cursors are deterministic.
    let mut all: Vec<(String, BTreeMap<String, String>)> = state
        .resources
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));

    let matching: Vec<(String, BTreeMap<String, String>)> = all
        .into_iter()
        .filter(|(arn, tags)| {
            matches_type_filters(arn, &type_filters) && matches_tag_filters(tags, &tag_filters)
        })
        .collect();

    let total = matching.len();
    let page: Vec<Value> = matching
        .into_iter()
        .skip(skip)
        .take(page_size)
        .map(|(arn, tags)| {
            json!({
                "ResourceARN": arn,
                "Tags": tags
                    .into_iter()
                    .map(|(k, v)| json!({"Key": k, "Value": v}))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    let returned = page.len();
    let next_token = if skip + returned < total {
        encode_cursor(skip + returned)
    } else {
        String::new()
    };

    Ok(json!({
        "PaginationToken": next_token,
        "ResourceTagMappingList": page,
    }))
}

#[derive(Debug, Default)]
struct TagFilter {
    key: String,
    values: Vec<String>,
}

fn parse_tag_filters(value: Option<&Value>) -> Result<Vec<TagFilter>, AwsError> {
    let Some(arr) = value.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(arr.len());
    for f in arr {
        let Some(key) = f.get("Key").and_then(Value::as_str) else {
            continue;
        };
        let values: Vec<String> = f
            .get("Values")
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .filter_map(|x| x.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        // AWS caps TagFilter.Values at 256 per filter and treats
        // missing/empty `Values` as "match any tag with this key" —
        // the latter is encoded by leaving the vector empty so
        // `matches_tag_filters` short-circuits.
        if values.len() > 256 {
            return Err(AwsError::validation(format!(
                "TagFilter.Values for key `{}` has {} entries; the maximum is 256.",
                key,
                values.len()
            )));
        }
        out.push(TagFilter {
            key: key.to_string(),
            values,
        });
    }
    Ok(out)
}

fn matches_tag_filters(tags: &BTreeMap<String, String>, filters: &[TagFilter]) -> bool {
    filters.iter().all(|f| match tags.get(&f.key) {
        None => false,
        Some(v) => f.values.is_empty() || f.values.iter().any(|wanted| wanted == v),
    })
}

/// `ResourceTypeFilters` are documented as `service` or
/// `service:resourceType`, matched **case-sensitively** against the
/// ARN's canonical service (segment 2) and resource type (the head
/// of segment 5 before any `/` or `:`).
fn matches_type_filters(arn: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() < 6 {
        return false;
    }
    let service = parts[2];
    let resource_segment = parts[5];
    let resource_type = resource_segment.split(['/', ':']).next().unwrap_or("");

    filters.iter().any(|f| match f.split_once(':') {
        Some((svc, rt)) => svc == service && rt == resource_type,
        None => f == service,
    })
}

fn encode_cursor(skipped: usize) -> String {
    let payload = serde_json::to_vec(&Cursor { skipped }).unwrap_or_default();
    base64::engine::general_purpose::STANDARD.encode(payload)
}

fn decode_cursor(value: Option<&Value>) -> Result<usize, AwsError> {
    let Some(s) = value.and_then(Value::as_str) else {
        return Ok(0);
    };
    if s.is_empty() {
        return Ok(0);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|_| AwsError::validation("PaginationToken is not valid base64"))?;
    let cursor: Cursor = serde_json::from_slice(&bytes)
        .map_err(|_| AwsError::validation("PaginationToken is malformed"))?;
    Ok(cursor.skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("tagging", "us-east-1")
    }

    fn populated() -> TaggingState {
        let s = TaggingState::default();
        s.resources.insert(
            "arn:aws:s3:::bucket-a".into(),
            BTreeMap::from([
                ("Env".into(), "prod".into()),
                ("Team".into(), "core".into()),
            ]),
        );
        s.resources.insert(
            "arn:aws:s3:::bucket-b".into(),
            BTreeMap::from([("Env".into(), "dev".into())]),
        );
        s.resources.insert(
            "arn:aws:dynamodb:us-east-1:000000000000:table/users".into(),
            BTreeMap::from([("Team".into(), "auth".into())]),
        );
        s
    }

    #[test]
    fn filters_by_resource_type() {
        let state = populated();
        let resp =
            get_resources(&state, &json!({ "ResourceTypeFilters": ["s3"] }), &ctx()).unwrap();
        let arns: Vec<String> = resp["ResourceTagMappingList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["ResourceARN"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            arns,
            vec![
                "arn:aws:s3:::bucket-a".to_string(),
                "arn:aws:s3:::bucket-b".to_string()
            ]
        );
    }

    #[test]
    fn filters_by_tag_key_and_value() {
        let state = populated();
        let resp = get_resources(
            &state,
            &json!({
                "TagFilters": [{ "Key": "Env", "Values": ["prod"] }]
            }),
            &ctx(),
        )
        .unwrap();
        let arns: Vec<String> = resp["ResourceTagMappingList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["ResourceARN"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(arns, vec!["arn:aws:s3:::bucket-a".to_string()]);
    }

    #[test]
    fn paginates_and_returns_cursor() {
        let state = populated();
        let page1 = get_resources(&state, &json!({ "ResourcesPerPage": 2 }), &ctx()).unwrap();
        assert_eq!(page1["ResourceTagMappingList"].as_array().unwrap().len(), 2);
        let token = page1["PaginationToken"].as_str().unwrap();
        assert!(!token.is_empty());

        let page2 = get_resources(
            &state,
            &json!({ "ResourcesPerPage": 2, "PaginationToken": token }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(page2["ResourceTagMappingList"].as_array().unwrap().len(), 1);
        assert_eq!(page2["PaginationToken"].as_str().unwrap(), "");
    }

    #[test]
    fn resource_type_filters_are_case_sensitive() {
        let state = populated();
        // `S3` (uppercase) must not match `s3` ARNs — real AWS is
        // case-sensitive on canonical service names.
        let resp =
            get_resources(&state, &json!({ "ResourceTypeFilters": ["S3"] }), &ctx()).unwrap();
        assert!(
            resp["ResourceTagMappingList"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn tag_filter_values_capped_at_256() {
        let state = populated();
        let too_many: Vec<String> = (0..257).map(|i| format!("v{i}")).collect();
        let err = get_resources(
            &state,
            &json!({ "TagFilters": [{ "Key": "Env", "Values": too_many }] }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("256"), "{}", err.message);
    }

    #[test]
    fn tag_filter_with_empty_values_matches_any_value_for_key() {
        let state = populated();
        // No Values means "any value for the key" — bucket-a has
        // `Env=prod`, bucket-b has `Env=dev`; both match.
        let resp = get_resources(
            &state,
            &json!({ "TagFilters": [{ "Key": "Env", "Values": [] }] }),
            &ctx(),
        )
        .unwrap();
        let arns: Vec<String> = resp["ResourceTagMappingList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["ResourceARN"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            arns,
            vec![
                "arn:aws:s3:::bucket-a".to_string(),
                "arn:aws:s3:::bucket-b".to_string(),
            ]
        );
    }
}
