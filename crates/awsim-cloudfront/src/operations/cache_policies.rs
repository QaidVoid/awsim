use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ids::{new_etag, now_iso8601},
    state::{CachePolicy, CloudFrontState},
};

fn not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchCachePolicy",
        format!("The specified cache policy does not exist: {id}"),
    )
}

fn policy_to_value(p: &CachePolicy) -> Value {
    json!({
        "Id": p.id,
        "LastModifiedTime": p.created_at,
        "CachePolicyConfig": {
            "Name": p.name,
            "Comment": p.comment,
            "DefaultTTL": p.default_ttl,
            "MaxTTL": p.max_ttl,
            "MinTTL": p.min_ttl,
        }
    })
}

/// POST /2020-05-31/cache-policy
pub fn create_cache_policy(state: &CloudFrontState, input: &Value) -> Result<Value, AwsError> {
    let config = input.get("CachePolicyConfig").unwrap_or(input);

    let name = config
        .get("Name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("InvalidArgument", "Name is required"))?
        .to_string();

    // Reject duplicate names
    if state.cache_policies.iter().any(|e| e.value().name == name) {
        return Err(AwsError::conflict(
            "CachePolicyAlreadyExists",
            format!("A cache policy with name '{name}' already exists"),
        ));
    }

    let comment = config
        .get("Comment")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let default_ttl = config
        .get("DefaultTTL")
        .and_then(|v| v.as_u64())
        .unwrap_or(86400);
    let max_ttl = config
        .get("MaxTTL")
        .and_then(|v| v.as_u64())
        .unwrap_or(31536000);
    let min_ttl = config
        .get("MinTTL")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);

    let id = Uuid::new_v4().to_string();
    let etag = new_etag();

    let policy = CachePolicy {
        id: id.clone(),
        name,
        comment,
        default_ttl,
        max_ttl,
        min_ttl,
        created_at: now_iso8601(),
        etag: etag.clone(),
    };

    let result = policy_to_value(&policy);
    state.cache_policies.insert(id, policy);

    Ok(json!({
        "CachePolicy": result,
        "ETag": etag,
    }))
}

/// GET /2020-05-31/cache-policy/{Id}
pub fn get_cache_policy(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    let policy = state
        .cache_policies
        .get(id)
        .ok_or_else(|| not_found(id))?;

    let etag = policy.etag.clone();
    let result = policy_to_value(&policy);

    Ok(json!({
        "CachePolicy": result,
        "ETag": etag,
    }))
}

/// DELETE /2020-05-31/cache-policy/{Id}
pub fn delete_cache_policy(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    if state.cache_policies.remove(id).is_none() {
        return Err(not_found(id));
    }
    Ok(json!({}))
}

/// GET /2020-05-31/cache-policy (returns both stored and one fixed managed policy)
pub fn list_cache_policies(state: &CloudFrontState) -> Result<Value, AwsError> {
    let mut items: Vec<Value> = vec![
        // Built-in managed policy
        json!({
            "Type": "managed",
            "CachePolicy": {
                "Id": "658327ea-f89d-4fab-a63d-7e88639e58f6",
                "LastModifiedTime": "2021-05-10T00:00:00Z",
                "CachePolicyConfig": {
                    "Name": "CachingOptimized",
                    "DefaultTTL": 86400,
                    "MaxTTL": 31536000,
                    "MinTTL": 1,
                    "Comment": "Optimized for caching",
                }
            }
        }),
    ];

    for e in state.cache_policies.iter() {
        items.push(json!({
            "Type": "custom",
            "CachePolicy": policy_to_value(e.value())
        }));
    }

    let qty = items.len();

    Ok(json!({
        "CachePolicyList": {
            "MaxItems": 100,
            "Quantity": qty,
            "Items": { "CachePolicySummary": items }
        }
    }))
}
