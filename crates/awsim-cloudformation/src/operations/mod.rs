pub mod change_sets;
pub mod stacks;

use serde_json::Value;

/// Extract a required string parameter from the input Value.
pub fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, awsim_core::AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::error::missing_parameter(key))
}

/// Extract an optional string parameter from the input Value.
pub fn opt_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

/// Parse Parameters.member.N.{ParameterKey,ParameterValue} into a HashMap.
pub fn parse_parameters(input: &Value) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();

    let items = match input.get("Parameters") {
        Some(Value::Array(arr)) => arr.clone(),
        Some(Value::Object(obj)) => {
            // Could be member.1, member.2 or just raw object
            if let Some(Value::Array(arr)) = obj.get("member") {
                arr.clone()
            } else {
                // Try numbered keys
                let mut numbered: Vec<(usize, Value)> = obj
                    .iter()
                    .filter_map(|(k, v)| k.parse::<usize>().ok().map(|n| (n, v.clone())))
                    .collect();
                numbered.sort_by_key(|(n, _)| *n);
                numbered.into_iter().map(|(_, v)| v).collect()
            }
        }
        _ => return map,
    };

    for item in items {
        if let (Some(k), Some(v)) = (
            item.get("ParameterKey").and_then(|v| v.as_str()),
            item.get("ParameterValue").and_then(|v| v.as_str()),
        ) {
            map.insert(k.to_string(), v.to_string());
        }
    }

    map
}

/// Parse Tags.member.N.{Key,Value} into a HashMap.
pub fn parse_tags(input: &Value) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();

    let items = match input.get("Tags") {
        Some(Value::Array(arr)) => arr.clone(),
        Some(Value::Object(obj)) => {
            if let Some(Value::Array(arr)) = obj.get("member") {
                arr.clone()
            } else {
                let mut numbered: Vec<(usize, Value)> = obj
                    .iter()
                    .filter_map(|(k, v)| k.parse::<usize>().ok().map(|n| (n, v.clone())))
                    .collect();
                numbered.sort_by_key(|(n, _)| *n);
                numbered.into_iter().map(|(_, v)| v).collect()
            }
        }
        _ => return map,
    };

    for item in items {
        if let (Some(k), Some(v)) = (
            item.get("Key").and_then(|v| v.as_str()),
            item.get("Value").and_then(|v| v.as_str()),
        ) {
            map.insert(k.to_string(), v.to_string());
        }
    }

    map
}
