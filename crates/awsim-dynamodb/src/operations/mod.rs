pub mod backup;
pub mod batch;
pub mod item;
pub mod kinesis_dest;
pub mod partiql;
pub mod query;
pub mod resource_policy;
pub mod streams;
pub mod table;
pub mod transact;

use serde_json::Value;

/// Extract an optional string from input JSON.
pub fn opt_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

/// Extract a required string from input JSON.
pub fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, awsim_core::AwsError> {
    input.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        awsim_core::AwsError::bad_request("ValidationException", format!("{key} is required"))
    })
}

/// Build an empty ExpressionAttributeNames map if not present.
pub fn get_expr_attr_names(input: &Value) -> std::collections::HashMap<String, String> {
    input
        .get("ExpressionAttributeNames")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Build an ExpressionAttributeValues map.
pub fn get_expr_attr_values(input: &Value) -> serde_json::Map<String, Value> {
    input
        .get("ExpressionAttributeValues")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default()
}

/// Reject a request that supplies both `ProjectionExpression` and the
/// legacy `AttributesToGet`.
///
/// AWS treats the two as mutually exclusive: supplying both returns
/// `ValidationException`. Call this from any read path that accepts
/// `ProjectionExpression` (GetItem, BatchGetItem, Query, Scan) before
/// reading the value to keep callers from picking one and silently
/// dropping the other.
pub fn reject_attrs_to_get_with_projection(
    input: &Value,
    projection_expr: Option<&str>,
) -> Result<(), awsim_core::AwsError> {
    if projection_expr.is_some() && input.get("AttributesToGet").is_some() {
        return Err(awsim_core::AwsError::bad_request(
            "ValidationException",
            "Cannot specify both AttributesToGet and ProjectionExpression. \
             Use ProjectionExpression instead - AttributesToGet is a legacy parameter.",
        ));
    }
    Ok(())
}

/// Translate a byte count to read capacity units. AWS rounds up by 4 KiB
/// for strongly consistent reads and halves the result for eventually
/// consistent (the default). Transactional reads consume 2× the
/// strongly-consistent value.
pub fn read_capacity_units(bytes: usize, consistent: bool, transactional: bool) -> f64 {
    let blocks = bytes.div_ceil(4 * 1024).max(1) as f64;
    let mult = if transactional {
        2.0
    } else if consistent {
        1.0
    } else {
        0.5
    };
    blocks * mult
}

/// Translate a byte count to write capacity units (1 WCU per 1 KiB,
/// rounded up). Transactional writes consume 2× the standard cost.
pub fn write_capacity_units(bytes: usize, transactional: bool) -> f64 {
    let blocks = bytes.div_ceil(1024).max(1) as f64;
    let mult = if transactional { 2.0 } else { 1.0 };
    blocks * mult
}

/// Build a `ConsumedCapacity` JSON object when the caller passed
/// `ReturnConsumedCapacity` of `TOTAL` or `INDEXES`. Returns `None` for
/// `NONE` (or absent), matching the AWS contract that the field is only
/// present when explicitly requested.
pub fn build_consumed_capacity(
    input: &Value,
    table_name: &str,
    read_units: f64,
    write_units: f64,
) -> Option<Value> {
    let mode = opt_str(input, "ReturnConsumedCapacity").unwrap_or("NONE");
    if mode == "NONE" {
        return None;
    }
    let total = read_units + write_units;
    let mut cc = serde_json::Map::new();
    cc.insert("TableName".into(), Value::String(table_name.to_string()));
    cc.insert("CapacityUnits".into(), Value::from(total));
    if read_units > 0.0 {
        cc.insert("ReadCapacityUnits".into(), Value::from(read_units));
    }
    if write_units > 0.0 {
        cc.insert("WriteCapacityUnits".into(), Value::from(write_units));
    }
    Some(Value::Object(cc))
}
