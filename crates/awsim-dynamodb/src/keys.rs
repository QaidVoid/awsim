//! Key + GSI extraction shared by every item-storing operation.
//!
//! DynamoDB items live in two coordinate systems at once:
//!   * The user-facing names (whatever attributes the table's KeySchema
//!     and GSI KeySchemas point at).
//!   * The SQLite columns (`pk`, `sk`, `gsi{1..5}_pk`, `gsi{1..5}_sk`)
//!     that we project into at write time so range scans hit indexes.
//!
//! This module is the only place that knows how to map between them.

use serde_json::Value;

use crate::sqlite_store::MAX_GSI_SLOTS;
use crate::state::{DynamoItem, GlobalSecondaryIndex, KeySchemaElement, Table, extract_scalar_str};

/// Storage-level keys derived from a single item, ready to hand to
/// `SqliteStore::put_item`. `sk` is the empty string when the table
/// has no range key. Each GSI slot is `(None, None)` when the item
/// doesn't materialise into that index (sparse semantics).
pub struct ItemKeys {
    pub pk: String,
    pub sk: String,
    pub gsi: [(Option<String>, Option<String>); MAX_GSI_SLOTS],
}

/// Compute the storage key + every GSI key column for `item` against
/// `table`'s key schema. Returns `None` when the hash key (or required
/// range key) is missing or non-scalar — the caller should surface
/// that as a validation error to the SDK.
pub fn extract_item_keys(table: &Table, item: &DynamoItem) -> Option<ItemKeys> {
    let pk = key_value(&table.key_schema, item, "HASH")?;
    let sk = key_value(&table.key_schema, item, "RANGE").unwrap_or_default();

    let mut gsi: [(Option<String>, Option<String>); MAX_GSI_SLOTS] = Default::default();
    for (slot, idx) in table.gsi.iter().take(MAX_GSI_SLOTS).enumerate() {
        gsi[slot] = gsi_key_pair(idx, item);
    }

    Some(ItemKeys { pk, sk, gsi })
}

/// Compute just the (pk, sk) strings for a key map (used by `GetItem`,
/// `DeleteItem`, etc. where the caller hands us a Key map rather than
/// a full Item). Same return semantics as `extract_item_keys`.
pub fn extract_pk_sk(table: &Table, key: &DynamoItem) -> Option<(String, String)> {
    let pk = key_value(&table.key_schema, key, "HASH")?;
    let sk = key_value(&table.key_schema, key, "RANGE").unwrap_or_default();
    Some((pk, sk))
}

fn key_value(schema: &[KeySchemaElement], item: &DynamoItem, key_type: &str) -> Option<String> {
    let attr = schema.iter().find(|k| k.key_type == key_type)?;
    let raw = item.get(&attr.attribute_name)?;
    extract_scalar_str(raw).map(|s| s.to_string())
}

fn gsi_key_pair(idx: &GlobalSecondaryIndex, item: &DynamoItem) -> (Option<String>, Option<String>) {
    let mut pk = None;
    let mut sk = None;
    for ke in &idx.key_schema {
        let val = item
            .get(&ke.attribute_name)
            .and_then(extract_scalar_str)
            .map(|s| s.to_string());
        match ke.key_type.as_str() {
            "HASH" => pk = val,
            "RANGE" => sk = val,
            _ => {}
        }
    }
    (pk, sk)
}

/// Convert a `DynamoItem` (HashMap of typed AttributeValues) into a
/// `serde_json::Value` for storage in the `attrs_json` SQLite column.
/// The shape is the same as DynamoDB's wire format
/// (`{ "AttrName": { "S": "value" } }`), so we can round-trip cleanly.
pub fn item_to_storage_value(item: &DynamoItem) -> Value {
    let mut map = serde_json::Map::with_capacity(item.len());
    for (k, v) in item {
        map.insert(k.clone(), v.clone());
    }
    Value::Object(map)
}

/// Inverse of `item_to_storage_value` — turn a `serde_json::Value`
/// pulled out of `attrs_json` back into a `DynamoItem`.
pub fn storage_value_to_item(val: Value) -> Option<DynamoItem> {
    let Value::Object(map) = val else { return None };
    Some(map.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{KeySchemaElement, Projection};
    use serde_json::json;

    fn ks(name: &str, kt: &str) -> KeySchemaElement {
        KeySchemaElement {
            attribute_name: name.to_string(),
            key_type: kt.to_string(),
        }
    }

    fn make_table() -> Table {
        Table {
            name: "t".into(),
            arn: "arn".into(),
            key_schema: vec![ks("pk", "HASH"), ks("sk", "RANGE")],
            attribute_definitions: vec![],
            billing_mode: "PAY_PER_REQUEST".into(),
            status: "ACTIVE".into(),
            created_at: 0.0,
            gsi: vec![GlobalSecondaryIndex {
                index_name: "GSI1".into(),
                key_schema: vec![ks("g1pk", "HASH"), ks("g1sk", "RANGE")],
                projection: Projection {
                    projection_type: "ALL".into(),
                    non_key_attributes: vec![],
                },
                status: "ACTIVE".into(),
            }],
            lsi: vec![],
            stream_enabled: false,
            stream_arn: None,
            stream_view_type: None,
            stream_records: Vec::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: Default::default(),
            deletion_protection_enabled: false,
        }
    }

    fn dyn_item(json: serde_json::Value) -> DynamoItem {
        let serde_json::Value::Object(m) = json else {
            panic!("expected object")
        };
        m.into_iter().collect()
    }

    #[test]
    fn extracts_pk_sk_and_active_gsi() {
        let table = make_table();
        let item = dyn_item(json!({
            "pk":   {"S": "user-1"},
            "sk":   {"S": "profile"},
            "g1pk": {"S": "tenant-a"},
            "g1sk": {"S": "2024-01-01"},
        }));
        let keys = extract_item_keys(&table, &item).expect("keys");
        assert_eq!(keys.pk, "user-1");
        assert_eq!(keys.sk, "profile");
        assert_eq!(keys.gsi[0].0.as_deref(), Some("tenant-a"));
        assert_eq!(keys.gsi[0].1.as_deref(), Some("2024-01-01"));
        // Other GSI slots stay empty.
        for slot in &keys.gsi[1..] {
            assert!(slot.0.is_none() && slot.1.is_none());
        }
    }

    #[test]
    fn missing_gsi_attrs_yield_sparse_index() {
        let table = make_table();
        let item = dyn_item(json!({"pk": {"S": "x"}, "sk": {"S": "y"}}));
        let keys = extract_item_keys(&table, &item).expect("keys");
        assert!(keys.gsi[0].0.is_none() && keys.gsi[0].1.is_none());
    }

    #[test]
    fn missing_hash_key_returns_none() {
        let table = make_table();
        let item = dyn_item(json!({"sk": {"S": "alone"}}));
        assert!(extract_item_keys(&table, &item).is_none());
    }

    #[test]
    fn round_trips_storage_value() {
        let item = dyn_item(json!({"a": {"S": "1"}, "b": {"N": "2"}}));
        let stored = item_to_storage_value(&item);
        let back = storage_value_to_item(stored).expect("round-trip");
        assert_eq!(back.len(), 2);
        assert_eq!(back.get("a"), Some(&json!({"S": "1"})));
    }
}
