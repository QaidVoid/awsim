pub mod listeners;
pub mod load_balancers;
pub mod metadata;
pub mod rules;
pub mod tags;
pub mod target_groups;

use serde_json::Value;

/// Extract a required string parameter from the input Value.
pub fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, awsim_core::AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::error::missing_parameter(key))
}

/// Extract an optional string from the input Value.
pub fn opt_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

/// Extract strings from a dotted-member list (e.g. `Subnets.member.1`, `Subnets.member.2`, …)
/// or a plain array.
pub fn extract_string_list(input: &Value, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = input.get(key) {
        match v {
            Value::Array(arr) => {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        out.push(s.to_string());
                    }
                }
            }
            Value::Object(map) => {
                // "member" sub-key, then numeric keys
                if let Some(Value::Object(members)) = map.get("member") {
                    let mut pairs: Vec<_> = members.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                    for (_, v) in pairs {
                        if let Some(s) = v.as_str() {
                            out.push(s.to_string());
                        }
                    }
                } else {
                    // direct numeric keys
                    let mut pairs: Vec<_> = map.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
                    for (_, v) in pairs {
                        if let Some(s) = v.as_str() {
                            out.push(s.to_string());
                        }
                    }
                }
            }
            Value::String(s) => out.push(s.clone()),
            _ => {}
        }
    }
    out
}
