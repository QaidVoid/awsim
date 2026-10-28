use serde_json::Value;

/// The `Filter.N.Name` / `Filter.N.Value.M` pairs of a Describe request.
pub(crate) fn parse_filters(input: &Value) -> Vec<(String, Vec<String>)> {
    let Some(filters) = input.get("Filter") else {
        return Vec::new();
    };
    let entries: Vec<&Value> = match filters {
        Value::Object(map) => map.values().collect(),
        Value::Array(arr) => arr.iter().collect(),
        _ => return Vec::new(),
    };
    entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.get("Name").and_then(Value::as_str)?.to_string();
            let values = match entry.get("Value") {
                Some(Value::String(s)) => vec![s.clone()],
                Some(Value::Object(map)) => map
                    .values()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                Some(Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                _ => Vec::new(),
            };
            Some((name, values))
        })
        .collect()
}

/// Whether a resource passes every filter. `field` answers the resource's
/// values for a filter name, or `None` for a filter it does not support,
/// which is ignored rather than matched against.
pub(crate) fn matches_all(
    filters: &[(String, Vec<String>)],
    field: impl Fn(&str) -> Option<Vec<String>>,
) -> bool {
    filters.iter().all(|(name, wanted)| match field(name) {
        Some(have) => have.iter().any(|h| wanted.iter().any(|w| glob(w, h))),
        None => true,
    })
}

/// EC2 filter values accept `*` and `?` wildcards.
fn glob(pattern: &str, value: &str) -> bool {
    fn go(p: &[u8], v: &[u8]) -> bool {
        match (p.first(), v.first()) {
            (None, None) => true,
            (Some(b'*'), _) => go(&p[1..], v) || (!v.is_empty() && go(p, &v[1..])),
            (Some(b'?'), Some(_)) => go(&p[1..], &v[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &v[1..]),
            _ => false,
        }
    }
    go(pattern.as_bytes(), value.as_bytes())
}

/// Values of the `tag:<key>` and `tag-key` filters for a tag map.
pub(crate) fn tag_field(
    tags: &std::collections::HashMap<String, String>,
    name: &str,
) -> Option<Vec<String>> {
    if let Some(key) = name.strip_prefix("tag:") {
        return Some(tags.get(key).cloned().into_iter().collect());
    }
    if name == "tag-key" {
        return Some(tags.keys().cloned().collect());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_array_and_object_shapes() {
        let input = json!({ "Filter": [
            { "Name": "tag:team", "Value": ["a", "b"] },
            { "Name": "instance-state-name", "Value": { "1": "running" } },
        ]});
        let filters = parse_filters(&input);
        assert_eq!(
            filters[0],
            ("tag:team".to_string(), vec!["a".into(), "b".into()])
        );
        assert_eq!(filters[1].1, vec!["running".to_string()]);
    }

    #[test]
    fn wildcards_match() {
        assert!(glob("app-*", "app-123"));
        assert!(glob("i-????", "i-abcd"));
        assert!(!glob("i-????", "i-abcde"));
    }
}
