use awsim_core::AwsError;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ids::now_iso8601,
    state::{CloudFrontState, Invalidation},
};

fn not_found_dist(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchDistribution",
        format!("The specified distribution does not exist: {id}"),
    )
}

fn not_found_inv(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchInvalidation",
        format!("The specified invalidation does not exist: {id}"),
    )
}

fn invalidation_to_value(inv: &Invalidation) -> Value {
    let path_qty = inv.paths.len();
    let path_items: Vec<Value> = inv.paths.iter().map(|p| Value::String(p.clone())).collect();

    json!({
        "Id": inv.id,
        "Status": inv.status,
        "CreateTime": inv.create_time,
        "InvalidationBatch": {
            "CallerReference": inv.caller_reference,
            "Paths": {
                "Quantity": path_qty,
                "Items": { "Path": path_items }
            }
        }
    })
}

/// POST /2020-05-31/distribution/{DistributionId}/invalidation
pub fn create_invalidation(
    state: &CloudFrontState,
    dist_id: &str,
    input: &Value,
) -> Result<Value, AwsError> {
    if !state.distributions.contains_key(dist_id) {
        return Err(not_found_dist(dist_id));
    }

    let batch = input.get("InvalidationBatch").unwrap_or(input);
    let caller_reference = batch
        .get("CallerReference")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Extract paths
    let paths: Vec<String> = if let Some(paths_val) = batch.get("Paths") {
        let items = paths_val
            .get("Items")
            .and_then(|v| v.get("Path"))
            .or_else(|| paths_val.get("Items"))
            .unwrap_or(paths_val);

        match items {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            Value::String(s) => vec![s.clone()],
            _ => vec![],
        }
    } else {
        vec![]
    };

    let id = Uuid::new_v4().to_string();
    let inv = Invalidation {
        id: id.clone(),
        distribution_id: dist_id.to_string(),
        status: "Completed".to_string(),
        create_time: now_iso8601(),
        paths,
        caller_reference,
    };

    let result = invalidation_to_value(&inv);
    state.invalidations.insert(id.clone(), inv);

    Ok(json!({
        "Invalidation": result,
        "Location": format!("https://cloudfront.amazonaws.com/2020-05-31/distribution/{dist_id}/invalidation/{id}"),
    }))
}

/// GET /2020-05-31/distribution/{DistributionId}/invalidation/{Id}
pub fn get_invalidation(
    state: &CloudFrontState,
    dist_id: &str,
    inv_id: &str,
) -> Result<Value, AwsError> {
    if !state.distributions.contains_key(dist_id) {
        return Err(not_found_dist(dist_id));
    }

    let inv = state
        .invalidations
        .get(inv_id)
        .ok_or_else(|| not_found_inv(inv_id))?;

    Ok(json!({ "Invalidation": invalidation_to_value(&inv) }))
}

/// GET /2020-05-31/distribution/{DistributionId}/invalidation
pub fn list_invalidations(
    state: &CloudFrontState,
    dist_id: &str,
) -> Result<Value, AwsError> {
    if !state.distributions.contains_key(dist_id) {
        return Err(not_found_dist(dist_id));
    }

    let items: Vec<Value> = state
        .invalidations
        .iter()
        .filter(|e| e.value().distribution_id == dist_id)
        .map(|e| invalidation_to_value(e.value()))
        .collect();

    let qty = items.len();

    Ok(json!({
        "InvalidationList": {
            "Marker": "",
            "MaxItems": 100,
            "IsTruncated": false,
            "Quantity": qty,
            "Items": { "InvalidationSummary": items }
        }
    }))
}
