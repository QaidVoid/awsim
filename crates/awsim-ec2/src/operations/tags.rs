use awsim_core::AwsError;
use serde_json::{Value, json};

use crate::state::Ec2State;

/// Parse the EC2 query-encoded tag list (Tags.member.N.Key / Tags.member.N.Value).
/// The core parser delivers them as an object like:
/// { "member": { "1": { "Key": "Name", "Value": "foo" }, ... } }
fn parse_tag_list(input: &Value, field: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let tags_val = match input.get(field) {
        Some(v) => v,
        None => return result,
    };

    // Array form
    if let Some(arr) = tags_val.as_array() {
        for t in arr {
            if let (Some(k), Some(v)) = (t["Key"].as_str(), t["Value"].as_str()) {
                result.push((k.to_string(), v.to_string()));
            }
        }
        return result;
    }

    // Object form with "member" sub-key (ec2Query dot-notation)
    let member = match tags_val.get("member") {
        Some(m) => m,
        None => tags_val,
    };
    if let Some(obj) = member.as_object() {
        for (_, v) in obj {
            if let (Some(k), Some(val)) = (v["Key"].as_str(), v["Value"].as_str()) {
                result.push((k.to_string(), val.to_string()));
            }
        }
    }
    result
}

/// Parse ResourceId list from ec2Query input.
fn parse_resource_ids(input: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    let val = match input.get("ResourceId") {
        Some(v) => v,
        None => return ids,
    };
    match val {
        Value::String(s) => ids.push(s.clone()),
        Value::Array(arr) => {
            for v in arr {
                if let Some(s) = v.as_str() {
                    ids.push(s.to_string());
                }
            }
        }
        Value::Object(map) => {
            // Handles ResourceId.1, ResourceId.2 etc.
            for (_, v) in map {
                if let Some(s) = v.as_str() {
                    ids.push(s.to_string());
                }
            }
        }
        _ => {}
    }
    ids
}

// ---------------------------------------------------------------------------
// CreateTags
// ---------------------------------------------------------------------------

pub fn create_tags(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let resource_ids = parse_resource_ids(input);
    let tags = parse_tag_list(input, "Tag");

    for resource_id in &resource_ids {
        let mut entry = state
            .resource_tags
            .entry(resource_id.clone())
            .or_default();
        for (k, v) in &tags {
            entry.insert(k.clone(), v.clone());
        }

        // Also update tags on the underlying resource directly if it exists
        apply_tags_to_resource(state, resource_id, &tags);
    }

    Ok(json!({}))
}

fn apply_tags_to_resource(state: &Ec2State, resource_id: &str, tags: &[(String, String)]) {
    if let Some(mut vpc) = state.vpcs.get_mut(resource_id) {
        for (k, v) in tags {
            vpc.tags.insert(k.clone(), v.clone());
        }
        return;
    }
    if let Some(mut subnet) = state.subnets.get_mut(resource_id) {
        for (k, v) in tags {
            subnet.tags.insert(k.clone(), v.clone());
        }
        return;
    }
    if let Some(mut sg) = state.security_groups.get_mut(resource_id) {
        for (k, v) in tags {
            sg.tags.insert(k.clone(), v.clone());
        }
        return;
    }
    if let Some(mut igw) = state.internet_gateways.get_mut(resource_id) {
        for (k, v) in tags {
            igw.tags.insert(k.clone(), v.clone());
        }
        return;
    }
    if let Some(mut rt) = state.route_tables.get_mut(resource_id) {
        for (k, v) in tags {
            rt.tags.insert(k.clone(), v.clone());
        }
        return;
    }
    if let Some(mut inst) = state.instances.get_mut(resource_id) {
        for (k, v) in tags {
            inst.tags.insert(k.clone(), v.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// DeleteTags
// ---------------------------------------------------------------------------

pub fn delete_tags(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let resource_ids = parse_resource_ids(input);
    let tags_to_delete = parse_tag_list(input, "Tag");
    let keys: Vec<&str> = tags_to_delete.iter().map(|(k, _)| k.as_str()).collect();

    for resource_id in &resource_ids {
        if let Some(mut entry) = state.resource_tags.get_mut(resource_id.as_str()) {
            for k in &keys {
                entry.remove(*k);
            }
        }
        remove_tags_from_resource(state, resource_id, &keys);
    }

    Ok(json!({}))
}

fn remove_tags_from_resource(state: &Ec2State, resource_id: &str, keys: &[&str]) {
    if let Some(mut vpc) = state.vpcs.get_mut(resource_id) {
        for k in keys {
            vpc.tags.remove(*k);
        }
        return;
    }
    if let Some(mut subnet) = state.subnets.get_mut(resource_id) {
        for k in keys {
            subnet.tags.remove(*k);
        }
        return;
    }
    if let Some(mut sg) = state.security_groups.get_mut(resource_id) {
        for k in keys {
            sg.tags.remove(*k);
        }
        return;
    }
    if let Some(mut igw) = state.internet_gateways.get_mut(resource_id) {
        for k in keys {
            igw.tags.remove(*k);
        }
        return;
    }
    if let Some(mut rt) = state.route_tables.get_mut(resource_id) {
        for k in keys {
            rt.tags.remove(*k);
        }
        return;
    }
    if let Some(mut inst) = state.instances.get_mut(resource_id) {
        for k in keys {
            inst.tags.remove(*k);
        }
    }
}

// ---------------------------------------------------------------------------
// DescribeTags
// ---------------------------------------------------------------------------

pub fn describe_tags(state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    let mut items: Vec<Value> = Vec::new();

    for entry in state.resource_tags.iter() {
        let resource_id = entry.key();
        for (k, v) in entry.value() {
            items.push(json!({
                "key": k,
                "value": v,
                "resourceId": resource_id,
                "resourceType": resource_type_for(resource_id),
            }));
        }
    }

    Ok(json!({ "tagSet": { "item": items } }))
}

fn resource_type_for(resource_id: &str) -> &'static str {
    if resource_id.starts_with("vpc-") {
        "vpc"
    } else if resource_id.starts_with("subnet-") {
        "subnet"
    } else if resource_id.starts_with("sg-") {
        "security-group"
    } else if resource_id.starts_with("igw-") {
        "internet-gateway"
    } else if resource_id.starts_with("rtb-") {
        "route-table"
    } else if resource_id.starts_with("i-") {
        "instance"
    } else {
        "resource"
    }
}
