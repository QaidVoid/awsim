//! Extract AWS error definitions from Smithy JSON AST models.
//!
//! Every error in a Smithy model is a structure shape carrying the
//! `smithy.api#error` trait. The on-wire `Code` element (Query/EC2
//! protocols) can be overridden by `aws.protocols#awsQueryError.code`;
//! JSON protocols use the shape name (with the `Exception` suffix) for
//! the `__type` field. HTTP status comes from either
//! `smithy.api#httpError` or the Query trait's `httpResponseCode`.

use std::path::Path;

use serde_json::Value;

/// One error shape as defined in a Smithy model.
#[derive(Debug, Clone)]
pub struct SmithyError {
    /// AWS sdkId of the service the error belongs to (e.g. `"IAM"`).
    pub service_sdk_id: String,
    /// Local Smithy shape name (e.g. `"NoSuchEntityException"`).
    pub shape_name: String,
    /// On-wire error code AWS actually emits.
    ///
    /// For Query/EC2 protocols this is the `awsQueryError.code` override
    /// (typically the shape name with the `Exception` suffix stripped).
    /// For JSON protocols this is the shape name unchanged.
    pub wire_code: String,
    /// `awsQueryError.code` if present, regardless of the service's
    /// protocol. Useful for `awsQueryCompatible` JSON services where the
    /// SDK reads the Query code from the `x-amzn-query-error` header
    /// alongside the JSON body `__type`.
    pub query_error_code: Option<String>,
    /// HTTP status code AWS returns when raising this error.
    pub http_status: u16,
    /// `"client"` or `"server"`, mirroring `smithy.api#error`.
    pub error_kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Client,
    Server,
}

/// Wire protocol of an AWS service, as derived from the service shape's
/// `aws.protocols#*` traits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwsProtocol {
    AwsJson1_0,
    AwsJson1_1,
    AwsQuery,
    Ec2Query,
    RestJson1,
    RestXml,
    /// Newer SDKs negotiate Smithy RPC v2 CBOR for DynamoDB on some
    /// endpoints; for error-shape purposes it behaves like awsJson1_0.
    SmithyRpcV2Cbor,
}

/// Parse a Smithy AST model file and return all error definitions for the
/// service.
pub fn load_errors(model_path: &Path) -> Vec<SmithyError> {
    let content = std::fs::read_to_string(model_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", model_path.display()));
    let json: Value = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("parse {}: {e}", model_path.display()));
    let shapes = json["shapes"].as_object().expect("model.shapes missing");

    let (service_sdk_id, protocol) = service_info(shapes);

    let mut errors = Vec::new();
    for (shape_id, shape) in shapes {
        if shape["type"].as_str() != Some("structure") {
            continue;
        }
        let traits = match shape["traits"].as_object() {
            Some(t) => t,
            None => continue,
        };
        let kind = match traits.get("smithy.api#error").and_then(|v| v.as_str()) {
            Some("client") => ErrorKind::Client,
            Some("server") => ErrorKind::Server,
            _ => continue,
        };

        let shape_name = local_name(shape_id);
        let query_override = traits
            .get("aws.protocols#awsQueryError")
            .and_then(|q| q.as_object());
        let query_error_code = query_override
            .and_then(|q| q.get("code"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        let wire_code = match (protocol, query_override) {
            (AwsProtocol::AwsQuery | AwsProtocol::Ec2Query, Some(q)) => q
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or(&shape_name)
                .to_string(),
            _ => shape_name.clone(),
        };

        let http_status = traits
            .get("smithy.api#httpError")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                // awsQueryError.httpResponseCode only applies to Query,
                // EC2, and awsQueryCompatible JSON protocols; pure JSON
                // services that still carry the trait (legacy Query
                // migrations like SSM) ignore the response code.
                if matches!(protocol, AwsProtocol::AwsQuery | AwsProtocol::Ec2Query) {
                    query_override
                        .and_then(|q| q.get("httpResponseCode"))
                        .and_then(|v| v.as_u64())
                } else {
                    None
                }
            })
            .unwrap_or(match kind {
                ErrorKind::Client => 400,
                ErrorKind::Server => 500,
            }) as u16;

        errors.push(SmithyError {
            service_sdk_id: service_sdk_id.clone(),
            shape_name,
            wire_code,
            query_error_code,
            http_status,
            error_kind: kind,
        });
    }

    errors.sort_by(|a, b| a.shape_name.cmp(&b.shape_name));
    errors
}

fn service_info(shapes: &serde_json::Map<String, Value>) -> (String, AwsProtocol) {
    for shape in shapes.values() {
        if shape["type"].as_str() != Some("service") {
            continue;
        }
        let traits = shape["traits"].as_object().cloned().unwrap_or_default();
        let sdk_id = traits
            .get("aws.api#service")
            .and_then(|s| s.get("sdkId"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let protocol = if traits.contains_key("aws.protocols#awsJson1_0") {
            AwsProtocol::AwsJson1_0
        } else if traits.contains_key("aws.protocols#awsJson1_1") {
            AwsProtocol::AwsJson1_1
        } else if traits.contains_key("aws.protocols#awsQuery") {
            AwsProtocol::AwsQuery
        } else if traits.contains_key("aws.protocols#ec2Query") {
            AwsProtocol::Ec2Query
        } else if traits.contains_key("aws.protocols#restJson1") {
            AwsProtocol::RestJson1
        } else if traits.contains_key("aws.protocols#restXml") {
            AwsProtocol::RestXml
        } else if traits.contains_key("smithy.protocols#rpcv2Cbor") {
            AwsProtocol::SmithyRpcV2Cbor
        } else {
            AwsProtocol::AwsJson1_1
        };
        return (sdk_id, protocol);
    }
    panic!("model has no service shape");
}

fn local_name(shape_id: &str) -> String {
    shape_id.rsplit('#').next().unwrap_or(shape_id).to_string()
}

/// Look up a single error shape by name within a list produced by
/// [`load_errors`].
pub fn find<'a>(errors: &'a [SmithyError], shape_name: &str) -> Option<&'a SmithyError> {
    errors.iter().find(|e| e.shape_name == shape_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models_dir() -> std::path::PathBuf {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        crate_dir
            .ancestors()
            .nth(2)
            .expect("workspace root above crates/awsim-conformance")
            .join("models")
    }

    #[test]
    fn iam_no_such_entity_uses_query_override() {
        let errors = load_errors(&models_dir().join("iam.json"));
        let nse = find(&errors, "NoSuchEntityException").expect("NoSuchEntityException present");
        assert_eq!(nse.wire_code, "NoSuchEntity");
        assert_eq!(nse.http_status, 404);
        assert_eq!(nse.error_kind, ErrorKind::Client);
    }

    #[test]
    fn iam_limit_exceeded_is_409() {
        let errors = load_errors(&models_dir().join("iam.json"));
        let limit =
            find(&errors, "LimitExceededException").expect("LimitExceededException present");
        assert_eq!(limit.wire_code, "LimitExceeded");
        assert_eq!(limit.http_status, 409);
    }

    #[test]
    fn dynamodb_keeps_full_shape_name_on_wire() {
        let errors = load_errors(&models_dir().join("dynamodb.json"));
        let rne = find(&errors, "ResourceNotFoundException").expect("ResourceNotFoundException");
        // JSON protocol: no Query override, so __type carries the full
        // shape name.
        assert_eq!(rne.wire_code, "ResourceNotFoundException");
    }
}
