use serde_json::Value;

pub mod extra;
pub mod health_checks;
pub mod more;
pub mod records;
pub mod tags;
pub mod zones;

/// Read a repeated XML element as a list.
///
/// The XML parser has no schema, so it cannot tell a one-element list
/// from a single struct: `<Changes><Change>..</Change></Changes>` comes
/// back as an object, and only becomes an array once a second element
/// appears. Reading it with `as_array` alone silently dropped every
/// single-item request, so a one-record `ChangeResourceRecordSets`
/// answered 200 and changed nothing.
pub fn xml_list(parent: Option<&Value>, element: &str) -> Vec<Value> {
    let Some(node) = parent.and_then(|p| p.get(element)) else {
        return Vec::new();
    };
    match node {
        Value::Array(items) => items.clone(),
        Value::Null => Vec::new(),
        single => vec![single.clone()],
    }
}
