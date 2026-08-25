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
/// range key) is missing or non-scalar. The caller should surface
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

/// Validate a caller-supplied `Key` map, then extract its storage keys.
///
/// This is the entry point every operation taking a `Key` should use.
/// [`extract_pk_sk`] alone will happily coerce a mistyped key and then
/// simply miss, which surfaces a schema error as "item not found".
pub fn resolve_key(table: &Table, key: &DynamoItem) -> Result<(String, String), String> {
    validate_key_against_schema(table, key)?;
    extract_pk_sk(table, key)
        .ok_or_else(|| "The provided key element does not match the schema".to_string())
}

/// Validate a caller-supplied `Key` map against the table's key schema.
///
/// DynamoDB rejects a key whose element types disagree with the declared
/// `AttributeDefinitions`, whose required elements are missing, or which
/// carries attributes that are not key elements. Returning an empty result
/// instead would be worse than a wrong error: a type mismatch would read
/// as "item not found", so a test asserting absence passes locally and the
/// same code misbehaves against real AWS.
pub fn validate_key_against_schema(table: &Table, key: &DynamoItem) -> Result<(), String> {
    for element in &table.key_schema {
        let name = &element.attribute_name;
        let Some(supplied) = key.get(name) else {
            return Err(format!(
                "The provided key element does not match the schema: missing key `{name}`"
            ));
        };

        let Some(supplied_type) = attribute_type_tag(supplied) else {
            return Err(format!(
                "The provided key element does not match the schema: key `{name}` has no value"
            ));
        };

        if let Some(declared) = table
            .attribute_definitions
            .iter()
            .find(|d| &d.attribute_name == name)
            && declared.attribute_type != supplied_type
        {
            return Err(format!(
                "The provided key element does not match the schema: key `{name}` \
                 is declared as type {} but was supplied as type {supplied_type}",
                declared.attribute_type
            ));
        }
    }

    // AWS rejects a Key map carrying anything beyond the key elements.
    for name in key.keys() {
        if !table.key_schema.iter().any(|e| &e.attribute_name == name) {
            return Err(format!(
                "The provided key element does not match the schema: \
                 `{name}` is not a key attribute"
            ));
        }
    }

    Ok(())
}

/// The type tag of an AttributeValue: `S`, `N`, `B`, and so on.
fn attribute_type_tag(value: &Value) -> Option<&str> {
    value.as_object()?.keys().next().map(|s| s.as_str())
}

/// Convert a key AttributeValue into the string stored in its key
/// column.
///
/// Key columns are TEXT and SQLite compares TEXT byte-wise, so each type
/// has to be stored in a form whose byte order is DynamoDB's order:
///
/// * `S` is stored as-is. DynamoDB compares strings by UTF-8 bytes,
///   which is what SQLite already does.
/// * `N` goes through [`crate::numkey::encode`], because raw decimal
///   text sorts `"10"` before `"9"`.
/// * `B` is stored as hex. DynamoDB compares binary as unsigned bytes,
///   and the base64 the wire format uses does not preserve that order:
///   its alphabet runs `A-Za-z0-9+/`, so byte `0x00` encodes to `'A'`
///   while `0xF8` encodes to `'+'`, which sorts earlier. Hex digits are
///   monotonic in ASCII, and hex is fixed-width per byte, so it
///   preserves both the ordering and the shorter-is-a-prefix rule.
///
/// Every storage key must flow through here. Writes, point lookups, and
/// `ExclusiveStartKey` cursors all compare against these columns, so if
/// any one of them derived the string differently it would look up a
/// key that does not exist.
pub fn storage_key(value: &Value) -> Option<String> {
    if let Some(n) = value.get("N").and_then(Value::as_str) {
        // Unencodable input is something the validator should already
        // have rejected. Store it verbatim rather than inventing a key:
        // worse ordering, but never a lost item.
        return Some(crate::numkey::encode(n).unwrap_or_else(|| n.to_string()));
    }
    if let Some(b) = value.get("B").and_then(Value::as_str) {
        return Some(binary_to_hex(b).unwrap_or_else(|| b.to_string()));
    }
    extract_scalar_str(value).map(str::to_string)
}

/// Decode a base64 binary value and re-encode it as lowercase hex.
///
/// Returns `None` for input that is not valid base64, so the caller can
/// fall back to storing it verbatim.
pub(crate) fn binary_to_hex(base64_value: &str) -> Option<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_value)
        .ok()?;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    Some(out)
}

fn key_value(schema: &[KeySchemaElement], item: &DynamoItem, key_type: &str) -> Option<String> {
    let attr = schema.iter().find(|k| k.key_type == key_type)?;
    let raw = item.get(&attr.attribute_name)?;
    storage_key(raw)
}

fn gsi_key_pair(idx: &GlobalSecondaryIndex, item: &DynamoItem) -> (Option<String>, Option<String>) {
    let mut pk = None;
    let mut sk = None;
    let mut has_range = false;
    for ke in &idx.key_schema {
        let val = item.get(&ke.attribute_name).and_then(storage_key);
        match ke.key_type.as_str() {
            "HASH" => pk = val,
            "RANGE" => {
                has_range = true;
                sk = val;
            }
            _ => {}
        }
    }
    // An index with a composite key materialises an item only when BOTH key
    // attributes are present. Half a key is not a sparse entry, it is no
    // entry: the item is invisible to that index. Storing the hash alone
    // would surface it in index reads and hand back a LastEvaluatedKey with
    // no sort key in it.
    if pk.is_none() || (has_range && sk.is_none()) {
        return (None, None);
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

/// Inverse of `item_to_storage_value`. Turn a `serde_json::Value`
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
    use std::collections::VecDeque;

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
            stream_records: VecDeque::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: Default::default(),
            deletion_protection_enabled: false,
            sse: Default::default(),
            read_capacity_units: 0,
            write_capacity_units: 0,
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
    fn partial_gsi_key_does_not_materialise() {
        // GSI1 is (g1pk, g1sk). An item carrying only the hash half is not
        // in the index at all, so neither column is stored.
        let table = make_table();
        for partial in [
            json!({"pk": {"S": "x"}, "sk": {"S": "y"}, "g1pk": {"S": "tenant-a"}}),
            json!({"pk": {"S": "x"}, "sk": {"S": "y"}, "g1sk": {"S": "2024-01-01"}}),
        ] {
            let keys = extract_item_keys(&table, &dyn_item(partial)).expect("keys");
            assert!(
                keys.gsi[0].0.is_none() && keys.gsi[0].1.is_none(),
                "half a composite index key must not materialise"
            );
        }
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

#[cfg(test)]
mod storage_key_tests {
    use super::*;
    use base64::Engine;
    use serde_json::json;

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// DynamoDB compares binary as unsigned bytes. Base64 does not
    /// preserve that order, which is the whole reason for hex.
    #[test]
    fn binary_keys_sort_by_byte_value() {
        let ordered: Vec<Vec<u8>> = vec![
            vec![0x00],
            vec![0x00, 0x01],
            vec![0x01],
            vec![0x7f],
            vec![0x80],
            vec![0xf8],
            vec![0xff],
            vec![0xff, 0x00],
        ];
        let encoded: Vec<String> = ordered
            .iter()
            .map(|b| storage_key(&json!({ "B": b64(b) })).unwrap())
            .collect();
        let mut sorted = encoded.clone();
        sorted.sort();
        assert_eq!(sorted, encoded, "hex encoding must preserve byte order");

        // The raw base64 really would have got it wrong.
        let raw: Vec<String> = ordered.iter().map(|b| b64(b)).collect();
        let mut raw_sorted = raw.clone();
        raw_sorted.sort();
        assert_ne!(raw_sorted, raw, "base64 order differs, as expected");
    }

    #[test]
    fn shorter_binary_sorts_before_its_extension() {
        let short = storage_key(&json!({ "B": b64(&[0x41]) })).unwrap();
        let long = storage_key(&json!({ "B": b64(&[0x41, 0x00]) })).unwrap();
        assert!(short < long);
    }

    #[test]
    fn string_keys_are_stored_verbatim() {
        assert_eq!(
            storage_key(&json!({ "S": "item#1" })).unwrap(),
            "item#1".to_string()
        );
    }

    #[test]
    fn numeric_keys_are_encoded_not_verbatim() {
        let ten = storage_key(&json!({ "N": "10" })).unwrap();
        let nine = storage_key(&json!({ "N": "9" })).unwrap();
        assert!(nine < ten, "9 must sort before 10");
        assert_ne!(ten, "10");
    }

    /// Malformed input must still produce a key so the item is
    /// reachable, even if its ordering is not meaningful.
    #[test]
    fn undecodable_binary_falls_back_to_verbatim() {
        assert_eq!(
            storage_key(&json!({ "B": "not!valid!base64" })).unwrap(),
            "not!valid!base64".to_string()
        );
    }
}
