pub mod backup;
pub mod batch;
pub mod item;
pub mod kinesis_dest;
pub mod legacy;
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

/// Validate that every entry in `ExpressionAttributeValues` is a
/// well-formed AttributeValue. AWS rejects malformed entries (wrong
/// type wrapper, non-numeric `N`, non-string `S`, set type with empty
/// list) with `ValidationException`. Run this from any operation that
/// accepts `ExpressionAttributeValues` so the validation happens at
/// request parse time, not at expression evaluation.
pub fn validate_expr_attr_values(input: &Value) -> Result<(), awsim_core::AwsError> {
    let Some(obj) = input
        .get("ExpressionAttributeValues")
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    for (placeholder, value) in obj {
        if !placeholder.starts_with(':') {
            return Err(awsim_core::AwsError::bad_request(
                "ValidationException",
                format!(
                    "ExpressionAttributeValues placeholder `{placeholder}` must start with `:`."
                ),
            ));
        }
        validate_attribute_value(placeholder, value)?;
    }
    Ok(())
}

fn validate_attribute_value(placeholder: &str, value: &Value) -> Result<(), awsim_core::AwsError> {
    let obj = value.as_object().ok_or_else(|| {
        awsim_core::AwsError::bad_request(
            "ValidationException",
            format!("ExpressionAttributeValues `{placeholder}` must be an AttributeValue object."),
        )
    })?;
    if obj.len() != 1 {
        return Err(awsim_core::AwsError::bad_request(
            "ValidationException",
            format!(
                "ExpressionAttributeValues `{placeholder}` must specify exactly one type tag; \
                 got {}.",
                obj.len()
            ),
        ));
    }
    let (tag, inner) = obj.iter().next().unwrap();
    let invalid = |msg: &str| {
        awsim_core::AwsError::bad_request(
            "ValidationException",
            format!("ExpressionAttributeValues `{placeholder}` {msg}"),
        )
    };
    match tag.as_str() {
        "S" => {
            inner
                .as_str()
                .ok_or_else(|| invalid("type `S` requires a string value."))?;
        }
        "N" => {
            let s = inner
                .as_str()
                .ok_or_else(|| invalid("type `N` requires a stringified number."))?;
            s.parse::<f64>()
                .map_err(|_| invalid("type `N` value is not a valid number."))?;
        }
        "B" => {
            inner
                .as_str()
                .ok_or_else(|| invalid("type `B` requires a base64-encoded string."))?;
        }
        "BOOL" => {
            inner
                .as_bool()
                .ok_or_else(|| invalid("type `BOOL` requires true or false."))?;
        }
        "NULL" => {
            if inner.as_bool() != Some(true) {
                return Err(invalid("type `NULL` requires the literal value true."));
            }
        }
        "SS" | "NS" | "BS" => {
            let arr = inner
                .as_array()
                .ok_or_else(|| invalid(&format!("type `{tag}` requires an array.")))?;
            if arr.is_empty() {
                return Err(invalid(&format!(
                    "type `{tag}` set may not be empty; AWS requires at least one element."
                )));
            }
            for v in arr {
                let s = v
                    .as_str()
                    .ok_or_else(|| invalid(&format!("type `{tag}` entries must be strings.")))?;
                if tag == "NS" {
                    s.parse::<f64>()
                        .map_err(|_| invalid("type `NS` entries must be stringified numbers."))?;
                }
            }
        }
        "L" => {
            let arr = inner
                .as_array()
                .ok_or_else(|| invalid("type `L` requires an array."))?;
            for (i, v) in arr.iter().enumerate() {
                validate_attribute_value(&format!("{placeholder}.L[{i}]"), v)?;
            }
        }
        "M" => {
            let m = inner
                .as_object()
                .ok_or_else(|| invalid("type `M` requires an object."))?;
            for (k, v) in m {
                validate_attribute_value(&format!("{placeholder}.M.{k}"), v)?;
            }
        }
        other => {
            return Err(invalid(&format!(
                "unknown AttributeValue type `{other}`; expected one of \
                 S, N, B, BOOL, NULL, SS, NS, BS, L, M."
            )));
        }
    }
    Ok(())
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
///
/// `INDEXES` adds the AWS `Table` sub-object (base-table breakdown) plus
/// `LocalSecondaryIndexes` / `GlobalSecondaryIndexes` maps. When `index`
/// is `Some((name, is_gsi))` the matching map carries a per-index
/// capacity breakdown keyed by `name`; `is_gsi` routes it to the GSI map
/// (true) or the LSI map (false). When `index` is `None` both maps stay
/// empty (operations that touch only the base table).
pub fn build_consumed_capacity(
    input: &Value,
    table_name: &str,
    read_units: f64,
    write_units: f64,
    index: Option<(&str, bool)>,
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
    if mode == "INDEXES" {
        let mut table = serde_json::Map::new();
        table.insert("CapacityUnits".into(), Value::from(total));
        if read_units > 0.0 {
            table.insert("ReadCapacityUnits".into(), Value::from(read_units));
        }
        if write_units > 0.0 {
            table.insert("WriteCapacityUnits".into(), Value::from(write_units));
        }
        cc.insert("Table".into(), Value::Object(table));

        let mut lsi = serde_json::Map::new();
        let mut gsi = serde_json::Map::new();
        if let Some((name, is_gsi)) = index {
            let mut entry = serde_json::Map::new();
            entry.insert("CapacityUnits".into(), Value::from(total));
            if read_units > 0.0 {
                entry.insert("ReadCapacityUnits".into(), Value::from(read_units));
            }
            if write_units > 0.0 {
                entry.insert("WriteCapacityUnits".into(), Value::from(write_units));
            }
            let target = if is_gsi { &mut gsi } else { &mut lsi };
            target.insert(name.to_string(), Value::Object(entry));
        }
        cc.insert("LocalSecondaryIndexes".into(), Value::Object(lsi));
        cc.insert("GlobalSecondaryIndexes".into(), Value::Object(gsi));
    }
    Some(Value::Object(cc))
}

/// Build the `ItemCollectionMetrics` entry for a write that touched the item
/// whose key attributes are `key_source`.
///
/// AWS only emits this when the caller passes `ReturnItemCollectionMetrics=SIZE`
/// **and** the table has at least one local secondary index (item collections
/// only exist for LSIs); it returns `None` in every other case so callers can
/// conditionally insert the field. `key_source` may be the full item or just
/// the key map, since only the partition key is read from it.
pub fn item_collection_metrics(
    input: &Value,
    table: &crate::state::Table,
    key_source: &crate::state::DynamoItem,
) -> Option<Value> {
    if opt_str(input, "ReturnItemCollectionMetrics") != Some("SIZE") || table.lsi.is_empty() {
        return None;
    }
    let hash_key = table.hash_key()?;
    let hk_value = key_source.get(hash_key)?;
    Some(serde_json::json!({
        "ItemCollectionKey": { hash_key: hk_value },
        "SizeEstimateRangeGB": [0.0, 1.0],
    }))
}

/// Append one `ItemCollectionMetrics` entry to a per-table-name array,
/// creating the table's array on first use. Used by the batch and transact
/// write paths, which key metrics by table name.
pub fn push_item_collection(map: &mut serde_json::Map<String, Value>, table: &str, entry: Value) {
    map.entry(table.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("item collection bucket is an array")
        .push(entry);
}

#[cfg(test)]
mod expr_attr_value_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_well_formed_values() {
        let input = json!({
            "ExpressionAttributeValues": {
                ":s": { "S": "x" },
                ":n": { "N": "42" },
                ":b": { "B": "QUJD" },
                ":bool": { "BOOL": true },
                ":null": { "NULL": true },
                ":ss": { "SS": ["a", "b"] },
                ":ns": { "NS": ["1", "2.5"] },
                ":l": { "L": [{ "S": "x" }] },
                ":m": { "M": { "k": { "S": "v" } } },
            }
        });
        validate_expr_attr_values(&input).unwrap();
    }

    #[test]
    fn rejects_unknown_type_tag() {
        let input = json!({ "ExpressionAttributeValues": { ":x": { "Bogus": "y" } } });
        let err = validate_expr_attr_values(&input).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn rejects_n_with_non_numeric_string() {
        let input = json!({ "ExpressionAttributeValues": { ":x": { "N": "abc" } } });
        let err = validate_expr_attr_values(&input).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn rejects_empty_string_set() {
        let input = json!({ "ExpressionAttributeValues": { ":x": { "SS": [] } } });
        let err = validate_expr_attr_values(&input).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn rejects_placeholder_without_colon() {
        let input = json!({ "ExpressionAttributeValues": { "x": { "S": "v" } } });
        let err = validate_expr_attr_values(&input).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn rejects_two_type_tags_in_one_value() {
        let input = json!({
            "ExpressionAttributeValues": { ":x": { "S": "v", "N": "1" } }
        });
        let err = validate_expr_attr_values(&input).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }
}

#[cfg(test)]
mod consumed_capacity_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn returns_none_when_unset() {
        let cc = build_consumed_capacity(&json!({}), "t", 1.0, 0.0, None);
        assert!(cc.is_none());
    }

    #[test]
    fn total_mode_omits_table_and_index_subfields() {
        let cc = build_consumed_capacity(
            &json!({ "ReturnConsumedCapacity": "TOTAL" }),
            "orders",
            1.0,
            0.5,
            None,
        )
        .unwrap();
        assert_eq!(cc["TableName"], "orders");
        assert_eq!(cc["CapacityUnits"], 1.5);
        assert!(cc.get("Table").is_none());
        assert!(cc.get("LocalSecondaryIndexes").is_none());
        assert!(cc.get("GlobalSecondaryIndexes").is_none());
    }

    #[test]
    fn indexes_mode_emits_table_block_and_index_maps() {
        let cc = build_consumed_capacity(
            &json!({ "ReturnConsumedCapacity": "INDEXES" }),
            "orders",
            1.0,
            0.5,
            None,
        )
        .unwrap();
        assert_eq!(cc["TableName"], "orders");
        assert_eq!(cc["CapacityUnits"], 1.5);
        let table = cc["Table"].as_object().unwrap();
        assert_eq!(table["CapacityUnits"], 1.5);
        assert_eq!(table["ReadCapacityUnits"], 1.0);
        assert_eq!(table["WriteCapacityUnits"], 0.5);
        assert!(cc["LocalSecondaryIndexes"].is_object());
        assert!(cc["GlobalSecondaryIndexes"].is_object());
        // No index requested: both maps are present but empty.
        assert!(cc["LocalSecondaryIndexes"].as_object().unwrap().is_empty());
        assert!(cc["GlobalSecondaryIndexes"].as_object().unwrap().is_empty());
    }

    #[test]
    fn indexes_mode_routes_gsi_capacity_into_gsi_map() {
        let cc = build_consumed_capacity(
            &json!({ "ReturnConsumedCapacity": "INDEXES" }),
            "orders",
            2.0,
            0.0,
            Some(("byTag", true)),
        )
        .unwrap();
        // GSI breakdown lands under GlobalSecondaryIndexes keyed by name.
        let gsi = cc["GlobalSecondaryIndexes"]["byTag"].as_object().unwrap();
        assert_eq!(gsi["CapacityUnits"], 2.0);
        assert_eq!(gsi["ReadCapacityUnits"], 2.0);
        assert!(gsi.get("WriteCapacityUnits").is_none());
        // The LSI map stays empty for a GSI request.
        assert!(cc["LocalSecondaryIndexes"].as_object().unwrap().is_empty());
    }

    #[test]
    fn indexes_mode_routes_lsi_capacity_into_lsi_map() {
        let cc = build_consumed_capacity(
            &json!({ "ReturnConsumedCapacity": "INDEXES" }),
            "orders",
            3.0,
            0.0,
            Some(("byDate", false)),
        )
        .unwrap();
        let lsi = cc["LocalSecondaryIndexes"]["byDate"].as_object().unwrap();
        assert_eq!(lsi["CapacityUnits"], 3.0);
        assert!(cc["GlobalSecondaryIndexes"].as_object().unwrap().is_empty());
    }
}
