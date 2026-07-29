//! AWS CBOR request and response bodies.
//!
//! Two dialects share this codec:
//!
//! * **Legacy AWS CBOR.** `Content-Type: application/x-amz-cbor-1.1` with
//!   an `X-Amz-Target` header. This is the JSON 1.0/1.1 dispatch shape
//!   with a different body encoding, and it is what the AWS SDK for Java
//!   v1 sends to DynamoDB and Kinesis by default.
//! * **Smithy rpcv2Cbor.** `smithy-protocol: rpc-v2-cbor`, routed by path
//!   at `/service/{ServiceName}/operation/{OperationName}`.
//!
//! Decoding produces exactly the `serde_json::Value` shape the JSON
//! parser produces for the equivalent payload, so the 60+ service
//! handlers stay protocol-agnostic and need no changes. The cost is that
//! binary values transit internally as base64 strings, which for a local
//! emulator is not a meaningful expense.

use base64::Engine;
use ciborium::value::Value as CborValue;
use serde_json::{Map, Value};

use crate::error::AwsError;

/// Decode a CBOR body into the JSON value shape handlers expect.
pub fn decode(bytes: &[u8]) -> Result<Value, AwsError> {
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let cbor: CborValue = ciborium::from_reader(bytes).map_err(|e| {
        AwsError::bad_request("SerializationException", format!("invalid CBOR: {e}"))
    })?;
    Ok(cbor_to_json(cbor))
}

/// Encode a handler's JSON output as CBOR.
pub fn encode(value: &Value) -> Result<Vec<u8>, AwsError> {
    let cbor = json_to_cbor(value);
    let mut out = Vec::new();
    ciborium::into_writer(&cbor, &mut out)
        .map_err(|e| AwsError::internal(format!("failed to encode CBOR response: {e}")))?;
    Ok(out)
}

/// CBOR to JSON.
///
/// Byte strings become base64, matching how the JSON protocols carry
/// binary. Tag 1 (epoch timestamp) unwraps to its numeric payload, which
/// is the representation the JSON protocols use for AWS timestamps.
fn cbor_to_json(value: CborValue) -> Value {
    match value {
        CborValue::Null => Value::Null,
        CborValue::Bool(b) => Value::Bool(b),
        CborValue::Text(s) => Value::String(s),
        CborValue::Bytes(b) => Value::String(base64::engine::general_purpose::STANDARD.encode(b)),
        CborValue::Integer(i) => {
            // i128 keeps 64-bit integers exact, which an f64 would not.
            let raw: i128 = i.into();
            if let Ok(n) = i64::try_from(raw) {
                Value::Number(n.into())
            } else if let Ok(n) = u64::try_from(raw) {
                Value::Number(n.into())
            } else {
                // Beyond 64 bits: keep it as a string rather than lose
                // precision silently.
                Value::String(raw.to_string())
            }
        }
        CborValue::Float(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        CborValue::Array(items) => Value::Array(items.into_iter().map(cbor_to_json).collect()),
        CborValue::Map(entries) => {
            let mut map = Map::with_capacity(entries.len());
            for (k, v) in entries {
                let key = match k {
                    CborValue::Text(s) => s,
                    CborValue::Integer(i) => {
                        let raw: i128 = i.into();
                        raw.to_string()
                    }
                    other => format!("{other:?}"),
                };
                map.insert(key, cbor_to_json(v));
            }
            Value::Object(map)
        }
        CborValue::Tag(tag, inner) => {
            // Tag 1 is epoch-based date/time. Every other tag is passed
            // through by value: the handlers only care about the payload.
            let _ = tag;
            cbor_to_json(*inner)
        }
        _ => Value::Null,
    }
}

/// JSON to CBOR.
///
/// DynamoDB's attribute-value envelope is the one place where a string
/// must go back out as a byte string: `B` and `BS` carry base64 in the
/// JSON protocols but are binary on the wire. The envelope is
/// structurally unambiguous, so it can be special-cased without needing
/// to consult the service's Smithy model.
fn json_to_cbor(value: &Value) -> CborValue {
    match value {
        Value::Null => CborValue::Null,
        Value::Bool(b) => CborValue::Bool(*b),
        Value::String(s) => CborValue::Text(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                CborValue::Integer(i.into())
            } else if let Some(u) = n.as_u64() {
                CborValue::Integer(u.into())
            } else {
                CborValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::Array(items) => CborValue::Array(items.iter().map(json_to_cbor).collect()),
        Value::Object(map) => {
            let mut entries = Vec::with_capacity(map.len());
            for (k, v) in map {
                let encoded = match (k.as_str(), v) {
                    ("B", Value::String(s)) => decode_b64(s),
                    ("BS", Value::Array(items)) => CborValue::Array(
                        items
                            .iter()
                            .map(|item| match item {
                                Value::String(s) => decode_b64(s),
                                other => json_to_cbor(other),
                            })
                            .collect(),
                    ),
                    _ => json_to_cbor(v),
                };
                entries.push((CborValue::Text(k.clone()), encoded));
            }
            CborValue::Map(entries)
        }
    }
}

/// Base64 back to a CBOR byte string, falling back to text when the
/// value is not valid base64 after all.
fn decode_b64(s: &str) -> CborValue {
    match base64::engine::general_purpose::STANDARD.decode(s) {
        Ok(bytes) => CborValue::Bytes(bytes),
        Err(_) => CborValue::Text(s.to_string()),
    }
}

/// Serialize an error the way a CBOR client expects.
///
/// Same `__type` discriminator the JSON protocols use, so a client that
/// branches on the error code behaves identically over either encoding.
pub fn encode_error(code: &str, message: &str) -> Vec<u8> {
    let mut map = Map::new();
    map.insert("__type".to_string(), Value::String(code.to_string()));
    map.insert("message".to_string(), Value::String(message.to_string()));
    encode(&Value::Object(map)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip(value: &Value) -> Value {
        decode(&encode(value).expect("encode")).expect("decode")
    }

    #[test]
    fn primitives_round_trip() {
        for v in [
            json!({"s": "hello"}),
            json!({"b": true}),
            json!({"n": 42}),
            json!({"neg": -7}),
            json!({"nil": null}),
        ] {
            assert_eq!(round_trip(&v), v, "round trip changed {v}");
        }
    }

    #[test]
    fn nested_structures_round_trip() {
        let v = json!({
            "outer": { "inner": [1, 2, {"deep": "value"}] },
            "list": ["a", "b"],
        });
        assert_eq!(round_trip(&v), v);
    }

    /// An f64 cannot hold this exactly, so a naive numeric path would
    /// silently corrupt it.
    #[test]
    fn large_integers_keep_their_exact_value() {
        let v = json!({ "n": 9007199254740993i64 });
        assert_eq!(round_trip(&v), v);
    }

    #[test]
    fn byte_strings_decode_to_base64() {
        // CBOR byte string 0x01 0x02 0x03 under key "b".
        let cbor = CborValue::Map(vec![(
            CborValue::Text("b".into()),
            CborValue::Bytes(vec![1, 2, 3]),
        )]);
        let mut buf = Vec::new();
        ciborium::into_writer(&cbor, &mut buf).unwrap();

        let decoded = decode(&buf).expect("decode");
        assert_eq!(decoded["b"], json!("AQID"));
    }

    /// The case that matters for DynamoDB: a `B` attribute must go back
    /// on the wire as binary, not as the base64 text we carry inside.
    #[test]
    fn dynamodb_binary_attribute_encodes_as_bytes() {
        let item = json!({ "Item": { "data": { "B": "AQID" } } });
        let encoded = encode(&item).expect("encode");
        let cbor: CborValue = ciborium::from_reader(&encoded[..]).expect("parse");

        fn find_bytes(v: &CborValue) -> Option<Vec<u8>> {
            match v {
                CborValue::Bytes(b) => Some(b.clone()),
                CborValue::Map(entries) => entries.iter().find_map(|(_, v)| find_bytes(v)),
                CborValue::Array(items) => items.iter().find_map(find_bytes),
                _ => None,
            }
        }
        assert_eq!(
            find_bytes(&cbor),
            Some(vec![1, 2, 3]),
            "B attribute should encode as a CBOR byte string"
        );
    }

    #[test]
    fn tagged_timestamps_unwrap_to_their_payload() {
        let cbor = CborValue::Map(vec![(
            CborValue::Text("when".into()),
            CborValue::Tag(1, Box::new(CborValue::Integer(1_700_000_000i64.into()))),
        )]);
        let mut buf = Vec::new();
        ciborium::into_writer(&cbor, &mut buf).unwrap();

        let decoded = decode(&buf).expect("decode");
        assert_eq!(decoded["when"], json!(1_700_000_000i64));
    }

    #[test]
    fn empty_body_decodes_to_an_empty_object() {
        assert_eq!(decode(&[]).expect("decode"), json!({}));
    }

    #[test]
    fn malformed_cbor_is_rejected() {
        let err = decode(&[0xff, 0xff, 0xff]).expect_err("should reject");
        assert_eq!(err.code, "SerializationException");
    }

    #[test]
    fn errors_carry_the_type_discriminator() {
        let bytes = encode_error("ResourceNotFoundException", "not found");
        let decoded = decode(&bytes).expect("decode");
        assert_eq!(decoded["__type"], json!("ResourceNotFoundException"));
        assert_eq!(decoded["message"], json!("not found"));
    }
}
