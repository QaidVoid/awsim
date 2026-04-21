use awsim_core::RequestContext;

/// Credentials extracted from an AWS SigV4 Authorization header.
#[derive(Debug, Clone)]
pub struct SigV4Credentials {
    pub access_key: String,
    pub region: String,
    pub service: String,
    pub date: String,
    pub signed_headers: Vec<String>,
    pub signature: String,
}

/// Parse the Authorization header to extract SigV4 credential components.
///
/// Format: `AWS4-HMAC-SHA256 Credential={access_key}/{date}/{region}/{service}/aws4_request,
///          SignedHeaders={headers}, Signature={sig}`
pub fn parse_authorization(header: &str) -> Option<SigV4Credentials> {
    let header = header.strip_prefix("AWS4-HMAC-SHA256 ")?;

    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;

    for part in header.split(", ") {
        if let Some(val) = part.strip_prefix("Credential=") {
            credential = Some(val);
        } else if let Some(val) = part.strip_prefix("SignedHeaders=") {
            signed_headers = Some(val);
        } else if let Some(val) = part.strip_prefix("Signature=") {
            signature = Some(val);
        }
    }

    let credential = credential?;
    let parts: Vec<&str> = credential.split('/').collect();
    if parts.len() != 5 {
        return None;
    }

    Some(SigV4Credentials {
        access_key: parts[0].to_string(),
        date: parts[1].to_string(),
        region: parts[2].to_string(),
        service: parts[3].to_string(),
        signed_headers: signed_headers?
            .split(';')
            .map(|s| s.to_string())
            .collect(),
        signature: signature?.to_string(),
    })
}

/// Build a RequestContext from parsed SigV4 credentials.
///
/// In bypass mode (default), we accept any credentials and just extract metadata.
pub fn build_request_context(
    creds: &SigV4Credentials,
    method: &str,
    uri: &str,
    default_account_id: &str,
) -> RequestContext {
    let mut ctx = RequestContext::new(&creds.service, &creds.region);
    ctx.account_id = default_account_id.to_string();
    ctx.access_key = Some(creds.access_key.clone());
    ctx.method = method.to_string();
    ctx.uri = uri.to_string();
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sigv4_authorization() {
        let header = "AWS4-HMAC-SHA256 \
            Credential=AKIAIOSFODNN7EXAMPLE/20230101/us-east-1/s3/aws4_request, \
            SignedHeaders=host;x-amz-date, \
            Signature=abcdef1234567890";

        let creds = parse_authorization(header).unwrap();
        assert_eq!(creds.access_key, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(creds.date, "20230101");
        assert_eq!(creds.region, "us-east-1");
        assert_eq!(creds.service, "s3");
        assert_eq!(creds.signed_headers, vec!["host", "x-amz-date"]);
        assert_eq!(creds.signature, "abcdef1234567890");
    }

    #[test]
    fn test_parse_invalid_header() {
        assert!(parse_authorization("Bearer token123").is_none());
        assert!(parse_authorization("").is_none());
    }

    #[test]
    fn test_build_request_context() {
        let creds = SigV4Credentials {
            access_key: "AKID".to_string(),
            region: "eu-west-1".to_string(),
            service: "dynamodb".to_string(),
            date: "20230101".to_string(),
            signed_headers: vec!["host".to_string()],
            signature: "sig".to_string(),
        };

        let ctx = build_request_context(&creds, "POST", "/", "123456789012");
        assert_eq!(ctx.account_id, "123456789012");
        assert_eq!(ctx.region, "eu-west-1");
        assert_eq!(ctx.service, "dynamodb");
        assert_eq!(ctx.access_key.unwrap(), "AKID");
    }
}
