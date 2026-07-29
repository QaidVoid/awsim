use serde_json::{Value, json};

pub mod permissions;
pub mod platform;
pub mod publish;
pub mod sms;
pub mod subscriptions;
pub mod tags;
pub mod topics;

/// Render a string map the way the query protocol serializes one.
///
/// SNS attribute maps go on the wire as repeated
/// `<entry><key>..</key><value>..</value></entry>` elements. Returning a
/// plain object instead produced `<DisplayName>..</DisplayName>`, which
/// botocore refuses to parse, so no SDK client could read topic,
/// subscription, or endpoint attributes at all.
///
/// `serde_json::Map` iterates in insertion order, so sort by key to keep
/// repeated calls returning the same document.
pub fn attribute_entries(attributes: &serde_json::Map<String, Value>) -> Value {
    let mut keys: Vec<&String> = attributes.keys().collect();
    keys.sort_unstable();
    json!({
        "entry": keys
            .into_iter()
            .map(|k| json!({ "key": k, "value": attributes[k] }))
            .collect::<Vec<_>>(),
    })
}
