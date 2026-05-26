/// CloudFormation template parsing and intrinsic function resolution.
///
/// Supports:
/// - JSON and YAML template formats
/// - Intrinsic functions: Ref, Fn::GetAtt, Fn::Sub, Fn::Join, Fn::Select, Fn::If
/// - Conditions
/// - DependsOn ordering
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::error::invalid_template;
use awsim_core::AwsError;

/// A parsed and validated CloudFormation template.
#[derive(Debug, Clone)]
pub struct ParsedTemplate {
    pub description: Option<String>,
    /// Resolved resource definitions, in dependency order.
    pub resources: Vec<ResourceDef>,
    /// Condition name -> resolved bool
    pub conditions: HashMap<String, bool>,
    /// Parameter definitions from the template
    pub parameters: Vec<ParameterDef>,
}

#[derive(Debug, Clone)]
pub struct ResourceDef {
    pub logical_id: String,
    pub resource_type: String,
    pub properties: Value,
    pub depends_on: Vec<String>,
    pub condition: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParameterDef {
    pub name: String,
    pub param_type: String,
    pub default: Option<String>,
    pub description: Option<String>,
}

/// Parse a template body (JSON or YAML) and return the raw Value.
/// Validate the `<vendor>::<service>::<resource>` shape that CFN uses
/// for all resource types. Real CFN looks the type up against a
/// registry; we accept any well-formed name under the documented
/// vendor prefixes (`AWS`, `Custom`, `Alexa`).
fn is_valid_resource_type(t: &str) -> bool {
    // Custom resources are `Custom::<Name>` (single double-colon).
    if let Some(rest) = t.strip_prefix("Custom::") {
        return !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
    }
    let parts: Vec<&str> = t.split("::").collect();
    if parts.len() != 3 {
        return false;
    }
    if !matches!(parts[0], "AWS" | "Alexa") {
        return false;
    }
    parts[1..]
        .iter()
        .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric()))
}

pub fn parse_template_body(body: &str) -> Result<Value, AwsError> {
    let trimmed = body.trim();

    if trimmed.starts_with('{') {
        serde_json::from_str(trimmed)
            .map_err(|e| invalid_template(format!("Invalid JSON template: {e}")))
    } else {
        use saphyr::{LoadableYamlNode, Yaml};
        let docs = Yaml::load_from_str(trimmed)
            .map_err(|e| invalid_template(format!("Invalid YAML template: {e}")))?;
        let doc = docs
            .into_iter()
            .next()
            .ok_or_else(|| invalid_template("Empty YAML template"))?;
        Ok(yaml_to_json(&doc))
    }
}

fn yaml_to_json(yaml: &saphyr::Yaml) -> Value {
    use saphyr::Yaml;
    match yaml {
        Yaml::Value(scalar) => scalar_to_json(scalar),
        Yaml::Sequence(seq) => Value::Array(seq.iter().map(yaml_to_json).collect()),
        Yaml::Mapping(map) => {
            let mut obj = Map::new();
            for (k, v) in map {
                let key = match k {
                    Yaml::Value(saphyr::Scalar::String(s)) => s.to_string(),
                    Yaml::Value(saphyr::Scalar::Integer(i)) => i.to_string(),
                    Yaml::Value(saphyr::Scalar::Boolean(b)) => b.to_string(),
                    _ => continue,
                };
                obj.insert(key, yaml_to_json(v));
            }
            Value::Object(obj)
        }
        Yaml::Tagged(_, inner) => yaml_to_json(inner),
        Yaml::Alias(_) | Yaml::BadValue | Yaml::Representation(_, _, _) => Value::Null,
    }
}

fn scalar_to_json(scalar: &saphyr::Scalar) -> Value {
    use saphyr::Scalar;
    match scalar {
        Scalar::Null => Value::Null,
        Scalar::Boolean(b) => Value::Bool(*b),
        Scalar::Integer(i) => Value::Number((*i).into()),
        Scalar::FloatingPoint(f) => serde_json::Number::from_f64(f.into_inner())
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Scalar::String(s) => Value::String(s.to_string()),
    }
}

/// Validate and parse a CloudFormation template.
pub fn validate_and_parse(
    body: &str,
    supplied_params: &HashMap<String, String>,
) -> Result<ParsedTemplate, AwsError> {
    let template = parse_template_body(body)?;

    let description = template
        .get("Description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Parse parameters
    let parameter_defs = parse_parameter_defs(&template);

    // Build effective parameter map: defaults + supplied values
    let mut params: HashMap<String, Value> = HashMap::new();
    for pd in &parameter_defs {
        if let Some(supplied) = supplied_params.get(&pd.name) {
            params.insert(pd.name.clone(), Value::String(supplied.clone()));
        } else if let Some(default) = &pd.default {
            params.insert(pd.name.clone(), Value::String(default.clone()));
        }
    }

    // Parse and evaluate conditions
    let conditions = evaluate_conditions(&template, &params);

    // Parse resources
    let resources_raw = template
        .get("Resources")
        .and_then(|v| v.as_object())
        .ok_or_else(|| invalid_template("Template must contain a 'Resources' section"))?;

    if resources_raw.is_empty() {
        return Err(invalid_template(
            "Template must contain at least one resource",
        ));
    }

    let mut resource_defs: Vec<ResourceDef> = Vec::new();
    for (logical_id, resource) in resources_raw {
        let resource_type = resource
            .get("Type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                invalid_template(format!("Resource '{logical_id}' must have a 'Type' field"))
            })?
            .to_string();

        // CFN resource type names follow `<vendor>::<service>::<resource>`
        // where vendor is `AWS`, `Custom`, or `Alexa`. Anything outside
        // that shape is a malformed template and AWS surfaces it as
        // `ValidationError` (with the offending logical id). Catching it
        // here is cheap and avoids letting nonsense flow through to the
        // resource-provisioning event sink.
        if !is_valid_resource_type(&resource_type) {
            return Err(invalid_template(format!(
                "Resource '{logical_id}' has unknown resource type '{resource_type}'. \
                 Expected `AWS::<Service>::<Resource>`, `Custom::<Name>`, or `Alexa::<Service>::<Resource>`."
            )));
        }

        let properties = resource
            .get("Properties")
            .cloned()
            .unwrap_or(Value::Object(Map::new()));

        let depends_on: Vec<String> = match resource.get("DependsOn") {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        };

        let condition = resource
            .get("Condition")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        resource_defs.push(ResourceDef {
            logical_id: logical_id.clone(),
            resource_type,
            properties,
            depends_on,
            condition,
        });
    }

    // Topological sort by DependsOn
    let sorted = topological_sort(resource_defs)
        .map_err(|e| invalid_template(format!("Dependency error: {e}")))?;

    Ok(ParsedTemplate {
        description,
        resources: sorted,
        conditions,
        parameters: parameter_defs,
    })
}

fn parse_parameter_defs(template: &Value) -> Vec<ParameterDef> {
    let mut defs = Vec::new();

    if let Some(params_obj) = template.get("Parameters").and_then(|v| v.as_object()) {
        for (name, param) in params_obj {
            let param_type = param
                .get("Type")
                .and_then(|v| v.as_str())
                .unwrap_or("String")
                .to_string();

            let default = param.get("Default").and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            });

            let description = param
                .get("Description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            defs.push(ParameterDef {
                name: name.clone(),
                param_type,
                default,
                description,
            });
        }
    }

    defs
}

fn evaluate_conditions(template: &Value, params: &HashMap<String, Value>) -> HashMap<String, bool> {
    let mut resolved: HashMap<String, bool> = HashMap::new();

    if let Some(conditions_obj) = template.get("Conditions").and_then(|v| v.as_object()) {
        for (name, condition) in conditions_obj {
            let result = eval_condition_value(condition, params, &resolved);
            resolved.insert(name.clone(), result);
        }
    }

    resolved
}

fn eval_condition_value(
    val: &Value,
    params: &HashMap<String, Value>,
    conditions: &HashMap<String, bool>,
) -> bool {
    match val {
        Value::Object(map) => {
            if let Some(eq_args) = map.get("Fn::Equals") {
                if let Value::Array(arr) = eq_args
                    && arr.len() == 2
                {
                    let a = resolve_value(&arr[0], params, conditions, &HashMap::new());
                    let b = resolve_value(&arr[1], params, conditions, &HashMap::new());
                    return a == b;
                }
                return false;
            }
            if let Some(not_arg) = map.get("Fn::Not") {
                if let Value::Array(arr) = not_arg
                    && let Some(first) = arr.first()
                {
                    return !eval_condition_value(first, params, conditions);
                }
                return false;
            }
            if let Some(and_args) = map.get("Fn::And") {
                if let Value::Array(arr) = and_args {
                    return arr
                        .iter()
                        .all(|a| eval_condition_value(a, params, conditions));
                }
                return false;
            }
            if let Some(or_args) = map.get("Fn::Or") {
                if let Value::Array(arr) = or_args {
                    return arr
                        .iter()
                        .any(|a| eval_condition_value(a, params, conditions));
                }
                return false;
            }
            false
        }
        Value::Bool(b) => *b,
        _ => false,
    }
}

/// Resolve intrinsic functions in a Value against parameters and pseudo-parameters.
pub fn resolve_value(
    val: &Value,
    params: &HashMap<String, Value>,
    conditions: &HashMap<String, bool>,
    resources: &HashMap<String, Value>,
) -> Value {
    match val {
        Value::Object(map) => {
            // Check for intrinsic function keys
            if let Some(ref_val) = map.get("Ref")
                && let Some(s) = ref_val.as_str()
            {
                return resolve_ref(s, params, resources);
            }
            if let Some(get_att) = map.get("Fn::GetAtt") {
                return resolve_get_att(get_att, resources);
            }
            if let Some(sub_val) = map.get("Fn::Sub") {
                return resolve_sub(sub_val, params, conditions, resources);
            }
            if let Some(join_val) = map.get("Fn::Join") {
                return resolve_join(join_val, params, conditions, resources);
            }
            if let Some(select_val) = map.get("Fn::Select") {
                return resolve_select(select_val, params, conditions, resources);
            }
            if let Some(if_val) = map.get("Fn::If") {
                return resolve_if(if_val, params, conditions, resources);
            }
            if let Some(b64_val) = map.get("Fn::Base64") {
                return resolve_base64(b64_val, params, conditions, resources);
            }
            if let Some(azs_val) = map.get("Fn::GetAZs") {
                return resolve_get_azs(azs_val, params, conditions, resources);
            }

            // Regular object: resolve all values recursively.
            let resolved_map: Map<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), resolve_value(v, params, conditions, resources)))
                .collect();
            Value::Object(resolved_map)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| resolve_value(v, params, conditions, resources))
                .collect(),
        ),
        // Primitives pass through as-is
        _ => val.clone(),
    }
}

fn resolve_ref(
    name: &str,
    params: &HashMap<String, Value>,
    resources: &HashMap<String, Value>,
) -> Value {
    // Check pseudo-parameters first
    match name {
        "AWS::AccountId" => return Value::String("000000000000".to_string()),
        "AWS::Region" => return Value::String("us-east-1".to_string()),
        "AWS::StackId" => {
            return Value::String(
                "arn:aws:cloudformation:us-east-1:000000000000:stack/stack/unknown".to_string(),
            );
        }
        "AWS::StackName" => return Value::String("unknown-stack".to_string()),
        "AWS::NoValue" => return Value::Null,
        _ => {}
    }

    // Check parameters
    if let Some(v) = params.get(name) {
        return v.clone();
    }

    // Check resource physical IDs
    if let Some(res) = resources.get(name) {
        if let Some(phys_id) = res.get("PhysicalResourceId") {
            return phys_id.clone();
        }
        return res.clone();
    }

    // Unknown ref — return as-is string
    Value::String(name.to_string())
}

fn resolve_get_att(val: &Value, resources: &HashMap<String, Value>) -> Value {
    // Fn::GetAtt: [LogicalId, AttributeName] or "LogicalId.AttributeName"
    match val {
        Value::Array(arr) if arr.len() == 2 => {
            let logical_id = arr[0].as_str().unwrap_or("");
            let attr = arr[1].as_str().unwrap_or("");
            if let Some(res) = resources.get(logical_id)
                && let Some(v) = res.get(attr)
            {
                return v.clone();
            }
            Value::String(format!("{logical_id}.{attr}"))
        }
        Value::String(s) => {
            if let Some((logical_id, attr)) = s.split_once('.')
                && let Some(res) = resources.get(logical_id)
                && let Some(v) = res.get(attr)
            {
                return v.clone();
            }
            Value::String(s.clone())
        }
        _ => Value::Null,
    }
}

fn resolve_sub(
    val: &Value,
    params: &HashMap<String, Value>,
    conditions: &HashMap<String, bool>,
    resources: &HashMap<String, Value>,
) -> Value {
    let (template_str, extra_vars) = match val {
        Value::String(s) => (s.as_str(), HashMap::new()),
        Value::Array(arr) if arr.len() == 2 => {
            let s = arr[0].as_str().unwrap_or("");
            let mut extra = HashMap::new();
            if let Some(Value::Object(map)) = arr.get(1) {
                for (k, v) in map {
                    let resolved = resolve_value(v, params, conditions, resources);
                    extra.insert(k.clone(), resolved.as_str().unwrap_or("").to_string());
                }
            }
            (s, extra)
        }
        _ => return val.clone(),
    };

    // Substitute ${VarName} patterns
    let mut i = 0;
    let bytes = template_str.as_bytes();
    let mut out = String::new();
    while i < bytes.len() {
        if bytes[i] == b'$'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'{'
            && let Some(end) = template_str[i + 2..].find('}')
        {
            let var_name = &template_str[i + 2..i + 2 + end];
            let replacement = if let Some(v) = extra_vars.get(var_name) {
                v.clone()
            } else if let Some(v) = params.get(var_name) {
                v.as_str().unwrap_or("").to_string()
            } else if let Some(res) = resources.get(var_name) {
                res.get("PhysicalResourceId")
                    .and_then(|v| v.as_str())
                    .unwrap_or(var_name)
                    .to_string()
            } else {
                // Pseudo-parameters
                match var_name {
                    "AWS::AccountId" => "000000000000".to_string(),
                    "AWS::Region" => "us-east-1".to_string(),
                    _ => var_name.to_string(),
                }
            };
            out.push_str(&replacement);
            i += 2 + end + 1; // skip past the closing `}`
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }

    Value::String(out)
}

fn resolve_join(
    val: &Value,
    params: &HashMap<String, Value>,
    conditions: &HashMap<String, bool>,
    resources: &HashMap<String, Value>,
) -> Value {
    if let Value::Array(arr) = val
        && arr.len() == 2
    {
        let delimiter = arr[0].as_str().unwrap_or("");
        let resolved = resolve_value(&arr[1], params, conditions, resources);
        let items: Vec<String> = match &resolved {
            Value::Array(items) => items
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
            _ => return Value::String(String::new()),
        };
        return Value::String(items.join(delimiter));
    }
    Value::Null
}

fn resolve_select(
    val: &Value,
    params: &HashMap<String, Value>,
    conditions: &HashMap<String, bool>,
    resources: &HashMap<String, Value>,
) -> Value {
    if let Value::Array(arr) = val
        && arr.len() == 2
    {
        let idx = arr[0].as_u64().unwrap_or(0) as usize;
        let resolved = resolve_value(&arr[1], params, conditions, resources);
        if let Value::Array(items) = resolved
            && let Some(item) = items.get(idx)
        {
            return item.clone();
        }
    }
    Value::Null
}

fn resolve_if(
    val: &Value,
    params: &HashMap<String, Value>,
    conditions: &HashMap<String, bool>,
    resources: &HashMap<String, Value>,
) -> Value {
    if let Value::Array(arr) = val
        && arr.len() == 3
    {
        let condition_name = arr[0].as_str().unwrap_or("");
        let is_true = conditions.get(condition_name).copied().unwrap_or(false);
        let branch = if is_true { &arr[1] } else { &arr[2] };
        return resolve_value(branch, params, conditions, resources);
    }
    Value::Null
}

/// `Fn::Base64`: resolve the inner value and return its UTF-8
/// bytes base64-encoded as a string. CloudFormation uses this for
/// EC2 UserData and inline launch-config blobs; without it,
/// `Fn::Base64: !Sub <...>` produced an empty map and any AMI
/// that consumed UserData booted with no user data set.
fn resolve_base64(
    val: &Value,
    params: &HashMap<String, Value>,
    conditions: &HashMap<String, bool>,
    resources: &HashMap<String, Value>,
) -> Value {
    use base64::Engine as _;
    let inner = resolve_value(val, params, conditions, resources);
    let raw = match &inner {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
    Value::String(encoded)
}

/// `Fn::GetAZs`: return the list of availability zones for the given
/// region. Real AWS varies the count per region; awsim is a single-
/// node emulator without real AZ topology, so we return three
/// synthesized zones (`<region>a`, `<region>b`, `<region>c`).
/// An empty string is shorthand for the calling region; CloudFormation
/// uses `Fn::GetAZs: ""` idiomatically inside `Fn::Select`.
fn resolve_get_azs(
    val: &Value,
    params: &HashMap<String, Value>,
    conditions: &HashMap<String, bool>,
    resources: &HashMap<String, Value>,
) -> Value {
    let inner = resolve_value(val, params, conditions, resources);
    let region = match &inner {
        Value::String(s) if !s.is_empty() => s.clone(),
        _ => "us-east-1".to_string(),
    };
    Value::Array(vec![
        Value::String(format!("{region}a")),
        Value::String(format!("{region}b")),
        Value::String(format!("{region}c")),
    ])
}

/// Topological sort of resources by DependsOn.
fn topological_sort(resources: Vec<ResourceDef>) -> Result<Vec<ResourceDef>, String> {
    let mut name_to_idx: HashMap<String, usize> = HashMap::new();
    for (i, r) in resources.iter().enumerate() {
        name_to_idx.insert(r.logical_id.clone(), i);
    }

    let n = resources.len();
    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, r) in resources.iter().enumerate() {
        for dep in &r.depends_on {
            if let Some(&j) = name_to_idx.get(dep) {
                adj[j].push(i);
                in_degree[i] += 1;
            } else {
                return Err(format!("Unknown DependsOn target '{dep}'"));
            }
        }
    }

    // Kahn's algorithm
    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut order: Vec<usize> = Vec::with_capacity(n);

    while let Some(node) = queue.first().copied() {
        queue.remove(0);
        order.push(node);
        for &next in &adj[node] {
            in_degree[next] -= 1;
            if in_degree[next] == 0 {
                queue.push(next);
            }
        }
    }

    if order.len() != n {
        return Err("Circular dependency detected in resources".to_string());
    }

    let mut result: Vec<Option<ResourceDef>> = resources.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|i| result[i].take().unwrap())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_resource_type_format() {
        assert!(is_valid_resource_type("AWS::S3::Bucket"));
        assert!(is_valid_resource_type("AWS::EC2::VPC"));
        assert!(is_valid_resource_type("Alexa::ASK::Skill"));
        assert!(is_valid_resource_type("Custom::MyResource"));
        assert!(!is_valid_resource_type(""));
        assert!(!is_valid_resource_type("S3::Bucket"));
        assert!(!is_valid_resource_type("AWS::"));
        assert!(!is_valid_resource_type("AWS::S3"));
        assert!(!is_valid_resource_type("Bogus::Service::Thing"));
        assert!(!is_valid_resource_type("AWS::S3::Bucket::Extra"));
    }

    #[test]
    fn rejects_unknown_resource_type_in_template() {
        let body = r#"{
            "Resources": {
                "X": { "Type": "Bogus::Service::Thing" }
            }
        }"#;
        let err = validate_and_parse(body, &HashMap::new()).unwrap_err();
        assert!(err.message.contains("unknown resource type"));
    }

    #[test]
    fn test_parse_json_template() {
        let body = r#"{
            "AWSTemplateFormatVersion": "2010-09-09",
            "Description": "Test template",
            "Resources": {
                "MyBucket": {
                    "Type": "AWS::S3::Bucket"
                }
            }
        }"#;

        let result = validate_and_parse(body, &HashMap::new());
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.description, Some("Test template".to_string()));
        assert_eq!(parsed.resources.len(), 1);
        assert_eq!(parsed.resources[0].logical_id, "MyBucket");
        assert_eq!(parsed.resources[0].resource_type, "AWS::S3::Bucket");
    }

    #[test]
    fn test_depends_on_ordering() {
        let body = r#"{
            "Resources": {
                "ResourceB": {
                    "Type": "AWS::S3::Bucket",
                    "DependsOn": "ResourceA"
                },
                "ResourceA": {
                    "Type": "AWS::IAM::Role"
                }
            }
        }"#;

        let result = validate_and_parse(body, &HashMap::new());
        assert!(result.is_ok());
        let parsed = result.unwrap();
        // ResourceA must come before ResourceB
        let a_pos = parsed
            .resources
            .iter()
            .position(|r| r.logical_id == "ResourceA")
            .unwrap();
        let b_pos = parsed
            .resources
            .iter()
            .position(|r| r.logical_id == "ResourceB")
            .unwrap();
        assert!(a_pos < b_pos, "ResourceA should precede ResourceB");
    }

    #[test]
    fn test_ref_resolution() {
        let mut params = HashMap::new();
        params.insert("MyParam".to_string(), Value::String("my-value".to_string()));
        let val = json!({ "Ref": "MyParam" });
        let resolved = resolve_value(&val, &params, &HashMap::new(), &HashMap::new());
        assert_eq!(resolved, Value::String("my-value".to_string()));
    }

    #[test]
    fn test_fn_join() {
        let val = json!({ "Fn::Join": ["-", ["a", "b", "c"]] });
        let resolved = resolve_value(&val, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(resolved, Value::String("a-b-c".to_string()));
    }

    #[test]
    fn test_parse_yaml_template() {
        let body = r#"
AWSTemplateFormatVersion: "2010-09-09"
Description: YAML test template
Parameters:
  BucketName:
    Type: String
    Default: my-bucket
Resources:
  MyBucket:
    Type: AWS::S3::Bucket
    Properties:
      BucketName: !Ref BucketName
"#;
        let result = validate_and_parse(body, &HashMap::new()).unwrap();
        assert_eq!(result.description, Some("YAML test template".to_string()));
        assert_eq!(result.resources.len(), 1);
        assert_eq!(result.resources[0].logical_id, "MyBucket");
        assert_eq!(result.resources[0].resource_type, "AWS::S3::Bucket");
        assert_eq!(result.parameters.len(), 1);
        assert_eq!(result.parameters[0].name, "BucketName");
        assert_eq!(result.parameters[0].default, Some("my-bucket".to_string()));
    }
}

#[cfg(test)]
mod base64_intrinsic_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fn_base64_encodes_static_string() {
        let val = json!({ "Fn::Base64": "hello world" });
        let got = resolve_value(&val, &HashMap::new(), &HashMap::new(), &HashMap::new());
        // base64("hello world") == "aGVsbG8gd29ybGQ="
        assert_eq!(got, json!("aGVsbG8gd29ybGQ="));
    }

    #[test]
    fn fn_base64_encodes_nested_intrinsic_result() {
        let val = json!({
            "Fn::Base64": {
                "Fn::Sub": "#!/bin/bash\necho ${Name}"
            }
        });
        let mut params = HashMap::new();
        params.insert("Name".to_string(), json!("alice"));
        let got = resolve_value(&val, &params, &HashMap::new(), &HashMap::new());
        // base64("#!/bin/bash\necho alice") == "IyEvYmluL2Jhc2gKZWNobyBhbGljZQ=="
        assert_eq!(got, json!("IyEvYmluL2Jhc2gKZWNobyBhbGljZQ=="));
    }

    #[test]
    fn fn_base64_handles_empty_string() {
        let val = json!({ "Fn::Base64": "" });
        let got = resolve_value(&val, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(got, json!(""));
    }

    #[test]
    fn fn_get_azs_with_explicit_region() {
        let val = json!({ "Fn::GetAZs": "us-west-2" });
        let got = resolve_value(&val, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(got, json!(["us-west-2a", "us-west-2b", "us-west-2c"]));
    }

    #[test]
    fn fn_get_azs_with_empty_region_defaults() {
        let val = json!({ "Fn::GetAZs": "" });
        let got = resolve_value(&val, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(got, json!(["us-east-1a", "us-east-1b", "us-east-1c"]));
    }

    #[test]
    fn fn_get_azs_inside_fn_select_picks_first_zone() {
        let val = json!({ "Fn::Select": [0, { "Fn::GetAZs": "eu-central-1" }] });
        let got = resolve_value(&val, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(got, json!("eu-central-1a"));
    }
}
