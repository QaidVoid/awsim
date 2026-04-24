use axum::http::{HeaderMap, StatusCode};
use bytes::Bytes;
use serde_json::Value;

use crate::error::AwsError;

use super::ParsedRequest;

/// Parse an awsQuery request.
///
/// Form body contains `Action=OperationName&Version=...&Param1=value1&...`
/// Complex types use dot-notation: `Tags.member.1.Key=Name&Tags.member.1.Value=foo`
pub fn parse_request(body: &Bytes) -> Result<ParsedRequest, AwsError> {
    let body_str = std::str::from_utf8(body)
        .map_err(|_| AwsError::bad_request("InvalidRequest", "Request body is not valid UTF-8"))?;

    let params: Vec<(String, String)> = serde_urlencoded::from_str(body_str)
        .map_err(|e| AwsError::bad_request("InvalidRequest", format!("Invalid form body: {e}")))?;

    let operation = params
        .iter()
        .find(|(k, _)| k == "Action")
        .map(|(_, v)| v.clone())
        .ok_or_else(|| AwsError::bad_request("MissingAction", "Missing 'Action' parameter"))?;

    // Convert flat dot-notation params into structured JSON
    let input = flatten_to_json(&params);

    Ok(ParsedRequest { operation, input })
}

/// Convert flat query params with dot-notation into a JSON value.
///
/// Example input:
///   `Tags.member.1.Key=Name`, `Tags.member.1.Value=foo`
/// Output:
///   `{"Tags": {"member": [{"Key": "Name", "Value": "foo"}]}}`
fn flatten_to_json(params: &[(String, String)]) -> Value {
    let mut map = serde_json::Map::new();
    for (key, value) in params {
        if key == "Action" || key == "Version" {
            continue;
        }
        set_nested(&mut map, key, value);
    }
    Value::Object(map)
}

fn set_nested(map: &mut serde_json::Map<String, Value>, key: &str, value: &str) {
    let parts: Vec<&str> = key.split('.').collect();
    set_nested_recursive(map, &parts, value);
}

fn set_nested_recursive(map: &mut serde_json::Map<String, Value>, parts: &[&str], value: &str) {
    if parts.is_empty() {
        return;
    }
    if parts.len() == 1 {
        map.insert(parts[0].to_string(), Value::String(value.to_string()));
        return;
    }

    let key = parts[0];
    let rest = &parts[1..];

    // Check if next part is a number (array index)
    if let Some(next) = rest.first() {
        if next.parse::<usize>().is_ok() {
            // This is an array member pattern like "Tags.member.1.Key"
            let entry = map
                .entry(key.to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Value::Array(arr) = entry {
                let idx: usize = next.parse::<usize>().unwrap() - 1; // 1-based → 0-based
                while arr.len() <= idx {
                    arr.push(Value::Object(serde_json::Map::new()));
                }
                if rest.len() > 1 {
                    if let Value::Object(ref mut inner) = arr[idx] {
                        set_nested_recursive(inner, &rest[1..], value);
                    }
                } else {
                    arr[idx] = Value::String(value.to_string());
                }
            }
            return;
        }
    }

    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Value::Object(inner) = entry {
        set_nested_recursive(inner, rest, value);
    }
}

/// Serialize a successful awsQuery XML response.
///
/// Format:
/// ```xml
/// <{Action}Response xmlns="...">
///   <{Action}Result>
///     {serialized fields}
///   </{Action}Result>
///   <ResponseMetadata>
///     <RequestId>{request_id}</RequestId>
///   </ResponseMetadata>
/// </{Action}Response>
/// ```
pub fn serialize_response(
    operation: &str,
    output: &Value,
    request_id: &str,
) -> (StatusCode, HeaderMap, Bytes) {
    let result_xml = json_to_xml_fields(output);

    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <{operation}Response xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\n\
         <{operation}Result>\n\
         {result_xml}\
         </{operation}Result>\n\
         <ResponseMetadata>\n\
         <RequestId>{request_id}</RequestId>\n\
         </ResponseMetadata>\n\
         </{operation}Response>"
    );

    let mut headers = HeaderMap::new();
    headers.insert("content-type", "text/xml".parse().unwrap());
    headers.insert("x-amzn-requestid", request_id.parse().unwrap());
    (StatusCode::OK, headers, Bytes::from(xml))
}

/// Serialize an awsQuery/XML error response.
pub fn serialize_error(error: &AwsError, request_id: &str) -> (StatusCode, HeaderMap, Bytes) {
    let error_type = match error.error_type {
        crate::error::ErrorType::Sender => "Sender",
        crate::error::ErrorType::Receiver => "Receiver",
    };

    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <ErrorResponse xmlns=\"http://iam.amazonaws.com/doc/2010-05-08/\">\n\
         <Error>\n\
         <Type>{error_type}</Type>\n\
         <Code>{code}</Code>\n\
         <Message>{message}</Message>\n\
         </Error>\n\
         <RequestId>{request_id}</RequestId>\n\
         </ErrorResponse>",
        code = error.code,
        message = error.message,
    );

    let mut headers = HeaderMap::new();
    headers.insert("content-type", "text/xml".parse().unwrap());
    headers.insert("x-amzn-requestid", request_id.parse().unwrap());
    (error.status, headers, Bytes::from(xml))
}

/// Convert a JSON Value to XML elements.
pub fn json_to_xml_fields(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut xml = String::new();
            for (key, val) in map {
                match val {
                    Value::Object(_) => {
                        xml.push_str(&format!("<{key}>\n{}</{key}>\n", json_to_xml_fields(val)));
                    }
                    Value::Array(arr) => {
                        for item in arr {
                            xml.push_str(&format!(
                                "<{key}>\n{}</{key}>\n",
                                json_to_xml_fields(item)
                            ));
                        }
                    }
                    Value::String(s) => {
                        xml.push_str(&format!("<{key}>{s}</{key}>\n"));
                    }
                    Value::Number(n) => {
                        xml.push_str(&format!("<{key}>{n}</{key}>\n"));
                    }
                    Value::Bool(b) => {
                        xml.push_str(&format!("<{key}>{b}</{key}>\n"));
                    }
                    Value::Null => {
                        xml.push_str(&format!("<{key}/>\n"));
                    }
                }
            }
            xml
        }
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_query() {
        let body = Bytes::from("Action=GetCallerIdentity&Version=2011-06-15");
        let result = parse_request(&body).unwrap();
        assert_eq!(result.operation, "GetCallerIdentity");
        assert_eq!(result.input, Value::Object(serde_json::Map::new()));
    }

    #[test]
    fn test_parse_query_with_params() {
        let body = Bytes::from("Action=CreateUser&UserName=testuser&Path=/engineering/");
        let result = parse_request(&body).unwrap();
        assert_eq!(result.operation, "CreateUser");
        assert_eq!(result.input["UserName"], "testuser");
        assert_eq!(result.input["Path"], "/engineering/");
    }

    #[test]
    fn test_flatten_dot_notation() {
        let params = vec![
            ("Action".to_string(), "TagResource".to_string()),
            ("Tags.member.1.Key".to_string(), "Env".to_string()),
            ("Tags.member.1.Value".to_string(), "prod".to_string()),
            ("Tags.member.2.Key".to_string(), "Team".to_string()),
            ("Tags.member.2.Value".to_string(), "eng".to_string()),
        ];
        let result = flatten_to_json(&params);
        let tags = result["Tags"]["member"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0]["Key"], "Env");
        assert_eq!(tags[0]["Value"], "prod");
        assert_eq!(tags[1]["Key"], "Team");
        assert_eq!(tags[1]["Value"], "eng");
    }
}
