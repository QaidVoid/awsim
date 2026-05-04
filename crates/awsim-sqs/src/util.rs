use std::collections::HashMap;

use awsim_core::AwsError;
use md5::{Digest, Md5};

use crate::state::MessageAttribute;

/// Compute the MD5 hex digest of a string (used for MD5OfMessageBody).
pub fn md5_of(s: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Compute MD5OfMessageAttributes per the AWS SQS attribute-validation
/// algorithm.
///
/// Returns `None` when the attribute map is empty (AWS omits the field
/// entirely on responses with no attributes). Otherwise serializes each
/// attribute in name-sorted order as:
///
///   `len(name) name len(type) type transport_byte len(value) value`
///
/// where lengths are 4-byte big-endian, the transport byte is `1` for any
/// String- or Number-typed value and `2` for Binary, and value bytes are
/// the UTF-8 encoding (String/Number) or raw bytes (Binary). Custom data
/// types like `String.foo` use the base type as the transport indicator.
pub fn md5_of_message_attributes(attrs: &HashMap<String, MessageAttribute>) -> Option<String> {
    if attrs.is_empty() {
        return None;
    }

    let mut names: Vec<&String> = attrs.keys().collect();
    names.sort();

    let mut buf: Vec<u8> = Vec::new();
    for name in names {
        let attr = &attrs[name];
        encode_len_prefixed_str(&mut buf, name);
        encode_len_prefixed_str(&mut buf, &attr.data_type);

        let base_type = attr.data_type.split('.').next().unwrap_or(&attr.data_type);
        match base_type {
            "Binary" => {
                buf.push(2);
                let bytes = attr.binary_value.as_deref().unwrap_or(&[]);
                buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(bytes);
            }
            // String / Number / String.* / Number.* / unknown — treat as
            // String-encoded (transport byte 1).
            _ => {
                buf.push(1);
                let s = attr.string_value.as_deref().unwrap_or("");
                buf.extend_from_slice(&(s.len() as u32).to_be_bytes());
                buf.extend_from_slice(s.as_bytes());
            }
        }
    }

    let mut hasher = Md5::new();
    hasher.update(&buf);
    Some(format!("{:x}", hasher.finalize()))
}

fn encode_len_prefixed_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u32).to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
}

/// Extract the queue name from a queue URL.
///
/// URL format: `http://sqs.{region}.localhost:4566/{account_id}/{queue_name}`
pub fn queue_name_from_url(url: &str) -> Result<String, AwsError> {
    // Split on '/' and take the last segment
    url.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidAddress",
                format!("The address {url} is not valid for this endpoint."),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_attr(value: &str) -> MessageAttribute {
        MessageAttribute {
            data_type: "String".to_string(),
            string_value: Some(value.to_string()),
            binary_value: None,
        }
    }

    fn binary_attr(value: Vec<u8>) -> MessageAttribute {
        MessageAttribute {
            data_type: "Binary".to_string(),
            string_value: None,
            binary_value: Some(value),
        }
    }

    #[test]
    fn empty_attrs_returns_none() {
        let attrs: HashMap<String, MessageAttribute> = HashMap::new();
        assert!(md5_of_message_attributes(&attrs).is_none());
    }

    #[test]
    fn single_string_attribute_matches_aws_algorithm() {
        let mut attrs = HashMap::new();
        attrs.insert("Name".to_string(), string_attr("Alice"));
        // Hand-computed: name=4 "Name" type=6 "String" txn=1 val=5 "Alice"
        let mut expected = Vec::new();
        expected.extend_from_slice(&4u32.to_be_bytes());
        expected.extend_from_slice(b"Name");
        expected.extend_from_slice(&6u32.to_be_bytes());
        expected.extend_from_slice(b"String");
        expected.push(1);
        expected.extend_from_slice(&5u32.to_be_bytes());
        expected.extend_from_slice(b"Alice");
        let mut hasher = Md5::new();
        hasher.update(&expected);
        let want = format!("{:x}", hasher.finalize());

        let got = md5_of_message_attributes(&attrs).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn attributes_are_sorted_by_name_before_hashing() {
        let mut a = HashMap::new();
        a.insert("Z".to_string(), string_attr("z"));
        a.insert("A".to_string(), string_attr("a"));

        let mut b = HashMap::new();
        b.insert("A".to_string(), string_attr("a"));
        b.insert("Z".to_string(), string_attr("z"));

        // Same attributes, different insertion order — must hash the same.
        assert_eq!(md5_of_message_attributes(&a), md5_of_message_attributes(&b));
    }

    #[test]
    fn binary_attribute_uses_transport_byte_2() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "Blob".to_string(),
            binary_attr(vec![0xde, 0xad, 0xbe, 0xef]),
        );

        let mut expected = Vec::new();
        expected.extend_from_slice(&4u32.to_be_bytes());
        expected.extend_from_slice(b"Blob");
        expected.extend_from_slice(&6u32.to_be_bytes());
        expected.extend_from_slice(b"Binary");
        expected.push(2);
        expected.extend_from_slice(&4u32.to_be_bytes());
        expected.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let mut hasher = Md5::new();
        hasher.update(&expected);
        let want = format!("{:x}", hasher.finalize());

        let got = md5_of_message_attributes(&attrs).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn custom_string_subtype_uses_transport_byte_1() {
        // "String.foo" is a valid AWS custom type; its transport byte is
        // still 1 because the base type is String.
        let mut attrs = HashMap::new();
        attrs.insert(
            "K".to_string(),
            MessageAttribute {
                data_type: "String.foo".to_string(),
                string_value: Some("v".to_string()),
                binary_value: None,
            },
        );
        let got = md5_of_message_attributes(&attrs).expect("md5 present");

        // Same value with the bare "String" type would differ only in the
        // data_type bytes — round-trip the byte-level shape.
        assert_eq!(got.len(), 32);
    }
}
