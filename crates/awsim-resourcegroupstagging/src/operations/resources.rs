use std::collections::BTreeMap;

use awsim_core::{AwsError, RequestContext};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::TaggingState;

/// Per-call paging cursor — opaque to the caller, base64-encoded JSON
/// for us. We carry the last ARN that was returned so the next call
/// resumes by skipping everything `<= last_arn`. Encoding the ARN
/// (rather than an integer offset) makes pagination stable across
/// inserts/deletes that happen between calls — if a resource is
/// added before the cursor, the next page still picks up where the
/// previous one left off.
#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    /// Last ARN delivered on the previous page. Empty string when
    /// starting fresh.
    last_arn: String,
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

    let page_size = match input.get("ResourcesPerPage").and_then(Value::as_i64) {
        Some(n) if !(1..=100).contains(&n) => {
            return Err(AwsError::validation(format!(
                "ResourcesPerPage `{n}` must be in 1..=100."
            )));
        }
        Some(n) => n as usize,
        None => 50,
    };

    let last_arn = decode_cursor(input.get("PaginationToken"))?;

    // Collect into a BTreeMap so iteration is in ARN order; cloning
    // entries also drops the per-shard DashMap reads before we start
    // the (potentially long) page walk, satisfying the "per-call
    // read lock only" requirement.
    let snapshot: BTreeMap<String, BTreeMap<String, String>> = state
        .resources
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect();

    // Resume strictly after `last_arn`. An empty marker (`""` < every
    // real ARN) means "start from the first key". Filtering happens
    // *after* the cursor advance so a tag/type filter that excludes
    // everything on the current page still advances the cursor.
    let mut page: Vec<(String, BTreeMap<String, String>)> = Vec::with_capacity(page_size);
    let mut walked_arn: Option<String> = None;
    for (arn, tags) in snapshot.range::<String, _>((
        std::ops::Bound::Excluded(&last_arn),
        std::ops::Bound::Unbounded,
    )) {
        walked_arn = Some(arn.clone());
        if !matches_type_filters(arn, &type_filters) || !matches_tag_filters(tags, &tag_filters) {
            continue;
        }
        page.push((arn.clone(), tags.clone()));
        if page.len() >= page_size {
            break;
        }
    }

    let mappings: Vec<Value> = page
        .iter()
        .map(|(arn, tags)| {
            json!({
                "ResourceARN": arn,
                "Tags": tags
                    .iter()
                    .map(|(k, v)| json!({"Key": k, "Value": v}))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    // Hand back a cursor only when there is more to walk after the
    // last ARN we touched (filtered or otherwise). When `walked_arn`
    // is None we exhausted the entire map without seeing a single
    // entry — there is no next page either way.
    let next_token = match walked_arn {
        Some(last)
            if snapshot
                .range::<String, _>((std::ops::Bound::Excluded(&last), std::ops::Bound::Unbounded))
                .next()
                .is_some() =>
        {
            encode_cursor(&last)
        }
        _ => String::new(),
    };

    Ok(json!({
        "PaginationToken": next_token,
        "ResourceTagMappingList": mappings,
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

fn encode_cursor(last_arn: &str) -> String {
    let payload = serde_json::to_vec(&Cursor {
        last_arn: last_arn.to_string(),
    })
    .unwrap_or_default();
    base64::engine::general_purpose::STANDARD.encode(payload)
}

fn decode_cursor(value: Option<&Value>) -> Result<String, AwsError> {
    let Some(s) = value.and_then(Value::as_str) else {
        return Ok(String::new());
    };
    if s.is_empty() {
        return Ok(String::new());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|_| AwsError::validation("PaginationToken is not valid base64"))?;
    let cursor: Cursor = serde_json::from_slice(&bytes)
        .map_err(|_| AwsError::validation("PaginationToken is malformed"))?;
    Ok(cursor.last_arn)
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
    fn pagination_survives_inserts_between_pages() {
        let state = populated();
        // Page 1: two entries.
        let page1 = get_resources(&state, &json!({ "ResourcesPerPage": 2 }), &ctx()).unwrap();
        let arns1: Vec<String> = page1["ResourceTagMappingList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["ResourceARN"].as_str().unwrap().to_string())
            .collect();
        let token = page1["PaginationToken"].as_str().unwrap().to_string();
        assert!(!token.is_empty());

        // Inject a new resource *before* the cursor (ARN-wise) so an
        // offset-based cursor would skip it onto page 2 and produce a
        // duplicate / miss. A real AWS marker-based cursor doesn't
        // double-count it.
        state
            .resources
            .insert("arn:aws:s3:::aaa-injected".into(), BTreeMap::new());

        let page2 = get_resources(
            &state,
            &json!({ "ResourcesPerPage": 5, "PaginationToken": token }),
            &ctx(),
        )
        .unwrap();
        let arns2: Vec<String> = page2["ResourceTagMappingList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["ResourceARN"].as_str().unwrap().to_string())
            .collect();

        // No ARN appears on both pages.
        for arn in &arns2 {
            assert!(
                !arns1.contains(arn),
                "{arn} appeared on both pages — cursor is not stable",
            );
        }
        // The injected ARN sorts before the page-1 head, so it must
        // NOT show up on page 2 — that's the whole point of the
        // marker-based design.
        assert!(
            !arns2.contains(&"arn:aws:s3:::aaa-injected".to_string()),
            "page 2 should not surface an entry that sorts before page 1's head",
        );
    }

    #[test]
    fn rejects_resources_per_page_out_of_range() {
        let state = populated();
        for bad in [0i64, 101, 1000] {
            let err =
                get_resources(&state, &json!({ "ResourcesPerPage": bad }), &ctx()).unwrap_err();
            assert_eq!(err.code, "ValidationException", "input {bad}");
        }
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
