use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

/// A parsed Smithy model containing service info and all operations.
pub struct SmithyModel {
    #[allow(dead_code)]
    pub service_id: String,
    pub operations: Vec<OperationInfo>,
    #[allow(dead_code)]
    pub all_shapes: HashMap<String, Value>,
}

/// Information about a single operation extracted from the Smithy model.
pub struct OperationInfo {
    pub name: String,
    #[allow(dead_code)]
    pub input_shape: Option<String>,
    #[allow(dead_code)]
    pub output_shape: Option<String>,
    #[allow(dead_code)]
    pub required_input_fields: Vec<FieldInfo>,
}

/// Information about a required field in an input shape.
#[allow(dead_code)]
pub struct FieldInfo {
    pub name: String,
    pub field_type: FieldType,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum FieldType {
    String,
    Integer,
    Long,
    Boolean,
    Blob,
    Timestamp,
    List(Box<FieldType>),
    Map { key: Box<FieldType>, value: Box<FieldType> },
    Structure,
    Enum(Vec<String>),
    Unknown,
}

impl SmithyModel {
    /// Return all operation names.
    pub fn operations(&self) -> &[OperationInfo] {
        &self.operations
    }

    /// Return operation names as a set for quick lookup.
    pub fn operation_names(&self) -> HashSet<String> {
        self.operations.iter().map(|o| o.name.clone()).collect()
    }
}

/// Parse a Smithy JSON AST model file.
pub fn parse_model(path: &Path) -> SmithyModel {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read model {}: {e}", path.display()));
    let json: Value = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON {}: {e}", path.display()));

    let shapes = json["shapes"]
        .as_object()
        .expect("model has no shapes object");

    // Build a flat map of all shape definitions.
    let all_shapes: HashMap<String, Value> = shapes
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Find the service shape.
    let mut service_id = String::new();
    let mut operation_targets: Vec<String> = Vec::new();

    for (shape_id, shape) in shapes {
        if shape["type"].as_str() == Some("service") {
            service_id = shape_id.clone();
            if let Some(ops) = shape["operations"].as_array() {
                for op in ops {
                    if let Some(target) = op["target"].as_str() {
                        operation_targets.push(target.to_string());
                    }
                }
            }
            break;
        }
    }

    // Parse each operation.
    let mut operations = Vec::new();
    for op_id in &operation_targets {
        if let Some(op_shape) = all_shapes.get(op_id) {
            let name = local_name(op_id);
            let input_shape = op_shape["input"]["target"]
                .as_str()
                .map(|s| s.to_string());
            let output_shape = op_shape["output"]["target"]
                .as_str()
                .map(|s| s.to_string());

            let required_input_fields = if let Some(ref input_id) = input_shape {
                extract_required_fields(input_id, &all_shapes)
            } else {
                Vec::new()
            };

            operations.push(OperationInfo {
                name,
                input_shape,
                output_shape,
                required_input_fields,
            });
        }
    }

    // Sort operations alphabetically for stable output.
    operations.sort_by(|a, b| a.name.cmp(&b.name));

    SmithyModel {
        service_id,
        operations,
        all_shapes,
    }
}

/// Extract the local name from a fully-qualified Smithy shape ID.
/// e.g. "com.amazonaws.dynamodb#CreateTable" → "CreateTable"
fn local_name(shape_id: &str) -> String {
    shape_id
        .rsplit('#')
        .next()
        .unwrap_or(shape_id)
        .to_string()
}

/// Extract required fields from an input structure shape.
fn extract_required_fields(
    shape_id: &str,
    all_shapes: &HashMap<String, Value>,
) -> Vec<FieldInfo> {
    let shape = match all_shapes.get(shape_id) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let members = match shape["members"].as_object() {
        Some(m) => m,
        None => return Vec::new(),
    };

    let mut fields = Vec::new();
    for (field_name, field_def) in members {
        // A field is required if it has the smithy.api#required trait.
        let is_required = field_def["traits"]["smithy.api#required"].is_object()
            || field_def["traits"]["smithy.api#required"] == Value::Bool(true);
        // Also treat fields with default values as effectively optional.
        let has_default = field_def["traits"].get("smithy.api#default").is_some();

        if is_required && !has_default {
            let target = field_def["target"].as_str().unwrap_or("smithy.api#String");
            let field_type = resolve_type(target, all_shapes);
            fields.push(FieldInfo {
                name: field_name.clone(),
                field_type,
            });
        }
    }

    fields
}

/// Resolve a shape target to its FieldType.
fn resolve_type(shape_id: &str, all_shapes: &HashMap<String, Value>) -> FieldType {
    // Handle primitive smithy.api shapes.
    match shape_id {
        "smithy.api#String" => return FieldType::String,
        "smithy.api#Integer" => return FieldType::Integer,
        "smithy.api#Long" => return FieldType::Long,
        "smithy.api#Boolean" => return FieldType::Boolean,
        "smithy.api#Blob" => return FieldType::Blob,
        "smithy.api#Timestamp" => return FieldType::Timestamp,
        "smithy.api#Unit" => return FieldType::String,
        "smithy.api#Document" => return FieldType::String,
        _ => {}
    }

    if let Some(shape) = all_shapes.get(shape_id) {
        match shape["type"].as_str().unwrap_or("") {
            "string" => {
                // Could be an enum.
                if let Some(members) = shape["members"].as_object() {
                    let values: Vec<String> = members
                        .values()
                        .filter_map(|m| {
                            m["traits"]["smithy.api#enumValue"]
                                .as_str()
                                .map(|s| s.to_string())
                        })
                        .collect();
                    if !values.is_empty() {
                        return FieldType::Enum(values);
                    }
                }
                FieldType::String
            }
            "enum" => {
                let values: Vec<String> = shape["members"]
                    .as_object()
                    .map(|m| {
                        m.values()
                            .filter_map(|v| {
                                v["traits"]["smithy.api#enumValue"]
                                    .as_str()
                                    .map(|s| s.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !values.is_empty() {
                    FieldType::Enum(values)
                } else {
                    FieldType::String
                }
            }
            "integer" => FieldType::Integer,
            "long" => FieldType::Long,
            "boolean" => FieldType::Boolean,
            "blob" => FieldType::Blob,
            "timestamp" => FieldType::Timestamp,
            "list" => {
                let member_target = shape["member"]["target"]
                    .as_str()
                    .unwrap_or("smithy.api#String");
                FieldType::List(Box::new(resolve_type(member_target, all_shapes)))
            }
            "map" => {
                let key_target = shape["key"]["target"]
                    .as_str()
                    .unwrap_or("smithy.api#String");
                let val_target = shape["value"]["target"]
                    .as_str()
                    .unwrap_or("smithy.api#String");
                FieldType::Map {
                    key: Box::new(resolve_type(key_target, all_shapes)),
                    value: Box::new(resolve_type(val_target, all_shapes)),
                }
            }
            "structure" => FieldType::Structure,
            "union" => FieldType::Structure,
            _ => FieldType::Unknown,
        }
    } else {
        FieldType::Unknown
    }
}
