use awsim_core::{AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::{Value, json};
use tracing::debug;

pub struct StsService;

impl StsService {
    pub fn new() -> Self {
        Self
    }

    fn get_caller_identity(&self, ctx: &RequestContext) -> Result<Value, AwsError> {
        debug!(
            account_id = %ctx.account_id,
            access_key = ?ctx.access_key,
            "GetCallerIdentity"
        );
        // Derive UserId and Arn from the SigV4-signed access key when one
        // was supplied: a sigil access key keeps the historical "root"
        // shape, otherwise we surface the access key itself as UserId so
        // tools that compare callers across requests get a stable, distinct
        // identifier per credential. Without an access key (anonymous /
        // unauthenticated test calls), we fall back to root.
        let (user_id, arn) = match ctx.access_key.as_deref() {
            Some(key) if !key.is_empty() => (
                key.to_string(),
                format!("arn:aws:iam::{}:user/{}", ctx.account_id, key),
            ),
            _ => (
                ctx.account_id.clone(),
                format!("arn:aws:iam::{}:root", ctx.account_id),
            ),
        };
        Ok(json!({
            "Account": ctx.account_id,
            "Arn": arn,
            "UserId": user_id,
        }))
    }

    fn assume_role(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let role_arn = input["RoleArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleArn is required"))?;

        let session_name = input["RoleSessionName"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleSessionName is required"))?;

        validate_role_arn(role_arn)?;
        validate_role_session_name(session_name)?;
        validate_session_tags(input)?;
        validate_session_policies(input)?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(3600);
        validate_assume_role_duration(duration)?;

        debug!(role_arn = %role_arn, session_name = %session_name, "AssumeRole");

        let (credentials, assumed_role_user) =
            generate_assumed_role_output(role_arn, session_name, &ctx.account_id, duration);

        Ok(json!({
            "Credentials": credentials,
            "AssumedRoleUser": assumed_role_user,
        }))
    }

    fn get_session_token(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(43200);

        debug!(account_id = %ctx.account_id, duration, "GetSessionToken");

        let credentials = generate_credentials(duration);
        Ok(json!({ "Credentials": credentials }))
    }

    fn assume_role_with_web_identity(
        &self,
        input: &Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        let role_arn = input["RoleArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleArn is required"))?;

        let session_name = input["RoleSessionName"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleSessionName is required"))?;

        // WebIdentityToken is required by AWS but we accept any value.
        let _token = input["WebIdentityToken"]
            .as_str()
            .ok_or_else(|| AwsError::validation("WebIdentityToken is required"))?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(3600);

        debug!(role_arn = %role_arn, session_name = %session_name, "AssumeRoleWithWebIdentity");

        let (credentials, assumed_role_user) =
            generate_assumed_role_output(role_arn, session_name, &ctx.account_id, duration);

        Ok(json!({
            "Credentials": credentials,
            "AssumedRoleUser": assumed_role_user,
            "SubjectFromWebIdentityToken": "web-identity-subject",
            "Audience": "sts.amazonaws.com",
            "Provider": "sts.amazonaws.com",
        }))
    }

    fn assume_root(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let target_principal = input["TargetPrincipal"]
            .as_str()
            .ok_or_else(|| AwsError::validation("TargetPrincipal is required"))?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(900);

        debug!(target_principal = %target_principal, "AssumeRoot");

        let credentials = generate_credentials(duration);
        let source_identity = format!("arn:aws:iam::{}:root", ctx.account_id);

        Ok(json!({
            "Credentials": credentials,
            "SourceIdentity": source_identity,
        }))
    }

    fn get_federation_token(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let name = input["Name"]
            .as_str()
            .ok_or_else(|| AwsError::validation("Name is required"))?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(43200);

        debug!(name = %name, duration, "GetFederationToken");

        let credentials = generate_credentials(duration);
        let federated_user_arn = format!("arn:aws:sts::{}:federated-user/{}", ctx.account_id, name);
        let federated_user_id = format!("{}:{}", ctx.account_id, name);

        Ok(json!({
            "Credentials": credentials,
            "FederatedUser": {
                "FederatedUserId": federated_user_id,
                "Arn": federated_user_arn,
            },
            "PackedPolicySize": 0,
        }))
    }

    fn decode_authorization_message(&self, input: &Value) -> Result<Value, AwsError> {
        let encoded = input["EncodedMessage"]
            .as_str()
            .ok_or_else(|| AwsError::validation("EncodedMessage is required"))?;

        if encoded.is_empty() {
            return Err(AwsError::bad_request(
                "InvalidAuthorizationMessageException",
                "EncodedMessage cannot be empty",
            ));
        }

        let decoded = json!({
            "allowed": false,
            "explicitDeny": false,
            "matchedStatements": {"items": []},
            "failures": {"items": []},
            "context": {
                "principal": {
                    "id": "AIDEXAMPLEAWSIM",
                    "arn": "arn:aws:iam::000000000000:user/awsim-user"
                },
                "action": "unknown:Action",
                "resource": "*",
                "conditions": {"items": []}
            }
        });

        Ok(json!({
            "DecodedMessage": decoded.to_string(),
        }))
    }

    fn get_access_key_info(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let key = input["AccessKeyId"]
            .as_str()
            .ok_or_else(|| AwsError::validation("AccessKeyId is required"))?;

        if key.len() < 16 {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                "AccessKeyId must be at least 16 characters",
            ));
        }

        Ok(json!({
            "Account": ctx.account_id,
        }))
    }

    fn assume_role_with_saml(
        &self,
        input: &Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        let role_arn = input["RoleArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleArn is required"))?;

        let _principal_arn = input["PrincipalArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("PrincipalArn is required"))?;

        // SAMLAssertion is required but we accept any value as a stub.
        let _saml_assertion = input["SAMLAssertion"]
            .as_str()
            .ok_or_else(|| AwsError::validation("SAMLAssertion is required"))?;

        let session_name = role_arn.split('/').next_back().unwrap_or("SAMLSession");

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(3600);

        debug!(role_arn = %role_arn, "AssumeRoleWithSAML");

        let (credentials, assumed_role_user) =
            generate_assumed_role_output(role_arn, session_name, &ctx.account_id, duration);

        Ok(json!({
            "Credentials": credentials,
            "AssumedRoleUser": assumed_role_user,
            "Issuer": "https://saml.example.com",
            "Audience": "sts.amazonaws.com",
            "NameQualifier": "awsim-saml",
            "SubjectType": "transient",
            "Subject": "saml-subject",
        }))
    }
}

impl Default for StsService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for StsService {
    fn service_name(&self) -> &str {
        "sts"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsQuery
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        match operation {
            "GetCallerIdentity" => self.get_caller_identity(ctx),
            "AssumeRole" => self.assume_role(&input, ctx),
            "GetSessionToken" => self.get_session_token(&input, ctx),
            "AssumeRoleWithWebIdentity" => self.assume_role_with_web_identity(&input, ctx),
            "AssumeRoleWithSAML" => self.assume_role_with_saml(&input, ctx),
            "GetFederationToken" => self.get_federation_token(&input, ctx),
            "DecodeAuthorizationMessage" => self.decode_authorization_message(&input),
            "GetAccessKeyInfo" => self.get_access_key_info(&input, ctx),
            "AssumeRoot" => self.assume_root(&input, ctx),
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}

// ---------------------------------------------------------------------------
// Input validation
// ---------------------------------------------------------------------------

/// Characters allowed in IAM resource names (e.g., role/user/session names)
/// per the published Smithy patterns: `[\w+=,.@-]`. `\w` here means
/// `[A-Za-z0-9_]`.
fn is_iam_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-')
}

/// Validate the AWS Role ARN shape:
/// `arn:aws(-{partition})?:iam::ACCOUNT_ID:role/PATH`
/// — 12-digit account id, role/path segments composed of `[\w+=,.@-]`
/// segments separated by `/`.
fn validate_role_arn(arn: &str) -> Result<(), AwsError> {
    fn invalid(arn: &str) -> AwsError {
        AwsError::validation(format!(
            "1 validation error detected: Value '{arn}' at 'roleArn' \
             failed to satisfy constraint: Member must satisfy regular \
             expression pattern: arn:aws(-[a-z]+)*:iam::\\d{{12}}:role/[\\w+=,.@-]+"
        ))
    }

    let rest = arn.strip_prefix("arn:aws").ok_or_else(|| invalid(arn))?;
    // Optional partition suffix like `-cn` or `-us-gov`.
    let rest = if let Some(stripped) = rest.strip_prefix(':') {
        stripped
    } else if let Some(after_dash) = rest.strip_prefix('-') {
        // Walk through the partition suffix until the next ':'.
        let colon = after_dash.find(':').ok_or_else(|| invalid(arn))?;
        let (partition, after) = after_dash.split_at(colon);
        if partition.is_empty()
            || !partition
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-')
        {
            return Err(invalid(arn));
        }
        // `after` starts with the ':' we found.
        &after[1..]
    } else {
        return Err(invalid(arn));
    };

    let rest = rest.strip_prefix("iam::").ok_or_else(|| invalid(arn))?;
    let (account, rest) = rest.split_once(':').ok_or_else(|| invalid(arn))?;
    if account.len() != 12 || !account.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid(arn));
    }
    let path = rest.strip_prefix("role/").ok_or_else(|| invalid(arn))?;
    if path.is_empty() {
        return Err(invalid(arn));
    }
    // Each '/'-separated segment must use only the IAM-name charset.
    for segment in path.split('/') {
        if segment.is_empty() || !segment.chars().all(is_iam_name_char) {
            return Err(invalid(arn));
        }
    }
    Ok(())
}

/// Validate `RoleSessionName` per Smithy: 2..=64 chars from `[\w+=,.@-]`.
fn validate_role_session_name(name: &str) -> Result<(), AwsError> {
    if !(2..=64).contains(&name.len()) || !name.chars().all(is_iam_name_char) {
        return Err(AwsError::validation(format!(
            "1 validation error detected: Value '{name}' at 'roleSessionName' \
             failed to satisfy constraint: Member must satisfy regular \
             expression pattern: [\\w+=,.@-]{{2,64}}"
        )));
    }
    Ok(())
}

/// Validate the optional session-policy inputs to AssumeRole. AWS:
///   - inline `Policy`: must parse as JSON; combined size with PolicyArns
///     bound to a packed limit (we use 2048 chars as a rough guard)
///   - `PolicyArns`: max 10 entries, each must be a valid policy ARN
///
/// Note: we don't yet apply session policies to credential evaluation —
/// the IAM enforcement engine is opt-in and AssumeRole credentials are
/// generated without consulting them. Validation here just prevents
/// callers from getting silent acceptance of bad input.
fn validate_session_policies(input: &Value) -> Result<(), AwsError> {
    if let Some(policy) = input.get("Policy").and_then(Value::as_str) {
        if policy.len() > 2048 {
            return Err(AwsError::validation(format!(
                "Session Policy must be at most 2048 characters; got {}",
                policy.len()
            )));
        }
        if !policy.is_empty() {
            serde_json::from_str::<Value>(policy)
                .map_err(|_| AwsError::validation("Session Policy must be valid JSON"))?;
        }
    }
    if let Some(arns) = input.get("PolicyArns").and_then(Value::as_array) {
        if arns.len() > 10 {
            return Err(AwsError::validation(format!(
                "PolicyArns supports up to 10 entries; got {}",
                arns.len()
            )));
        }
        for entry in arns {
            let arn = entry
                .get("arn")
                .or_else(|| entry.get("Arn"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AwsError::validation("Each PolicyArns entry must include an 'arn' field")
                })?;
            // Accept either an account-managed ARN (arn:aws:iam::{12 digits}:policy/...)
            // or an AWS-managed ARN (arn:aws:iam::aws:policy/...).
            let is_account_arn = arn.starts_with("arn:aws:iam::")
                && arn.split(':').nth(4).is_some_and(|seg| {
                    seg == "aws" || (seg.len() == 12 && seg.chars().all(|c| c.is_ascii_digit()))
                })
                && arn.contains(":policy/");
            if !is_account_arn {
                return Err(AwsError::validation(format!(
                    "PolicyArns entry '{arn}' is not a valid IAM policy ARN"
                )));
            }
        }
    }
    Ok(())
}

/// Validate the optional `Tags` and `TransitiveTagKeys` inputs to
/// AssumeRole. AWS rejects:
///   - more than 50 tags per session
///   - tag keys outside 1..=128 chars
///   - tag values over 256 chars
///   - duplicate tag keys (case-sensitive)
///   - TransitiveTagKeys that aren't subsets of the supplied tag keys
fn validate_session_tags(input: &Value) -> Result<(), AwsError> {
    let tags = match input.get("Tags").and_then(Value::as_array) {
        Some(arr) => arr,
        None => return Ok(()),
    };
    if tags.len() > 50 {
        return Err(AwsError::validation(format!(
            "Cannot have more than 50 session tags; got {}",
            tags.len()
        )));
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for tag in tags {
        let key = tag
            .get("Key")
            .and_then(Value::as_str)
            .ok_or_else(|| AwsError::validation("Each Tag entry must include Key and Value"))?;
        let value = tag
            .get("Value")
            .and_then(Value::as_str)
            .ok_or_else(|| AwsError::validation("Each Tag entry must include Key and Value"))?;
        if key.is_empty() || key.len() > 128 {
            return Err(AwsError::validation(format!(
                "Tag key length {} is outside 1..=128",
                key.len()
            )));
        }
        if value.len() > 256 {
            return Err(AwsError::validation(format!(
                "Tag value length {} exceeds 256",
                value.len()
            )));
        }
        if !seen.insert(key.to_string()) {
            return Err(AwsError::validation(format!(
                "Duplicate session tag key: {key}"
            )));
        }
    }

    if let Some(transitive) = input.get("TransitiveTagKeys").and_then(Value::as_array) {
        for k in transitive {
            let Some(s) = k.as_str() else {
                return Err(AwsError::validation(
                    "TransitiveTagKeys entries must be strings",
                ));
            };
            if !seen.contains(s) {
                return Err(AwsError::validation(format!(
                    "TransitiveTagKeys entry {s} is not present in Tags"
                )));
            }
        }
    }
    Ok(())
}

/// Validate the DurationSeconds parameter for AssumeRole. AWS allows
/// 900..=43200 seconds (the upper bound is role-specific in real AWS but
/// we apply the global max).
fn validate_assume_role_duration(seconds: u64) -> Result<(), AwsError> {
    if !(900..=43_200).contains(&seconds) {
        return Err(AwsError::validation(format!(
            "1 validation error detected: Value '{seconds}' at 'durationSeconds' \
             failed to satisfy constraint: Member must have value less than or equal to 43200 \
             and greater than or equal to 900"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Credential generation helpers
// ---------------------------------------------------------------------------

/// Generate a fake but realistically-shaped AWS access key ID.
/// Real access keys: ASIA... (temporary) or AKIA... (long-term).
/// Temporary STS credentials always start with ASIA.
fn fake_access_key_id() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    // Take 16 uppercase hex chars and prefix with ASIA
    let suffix: String = id[..16].to_uppercase();
    format!("ASIA{suffix}")
}

/// Generate a fake 40-character hex secret access key.
fn fake_secret_access_key() -> String {
    let u1 = uuid::Uuid::new_v4().simple().to_string();
    let u2 = uuid::Uuid::new_v4().simple().to_string();
    // Two UUIDs without hyphens give 64 hex chars; take first 40.
    format!("{u1}{u2}")[..40].to_string()
}

/// Generate a fake session token (base64-ish long string).
fn fake_session_token() -> String {
    let parts: Vec<String> = (0..4)
        .map(|_| uuid::Uuid::new_v4().simple().to_string())
        .collect();
    // Produce a long opaque string that looks like a real session token.
    format!(
        "FwoGZXIvYXdzEAwaDAwsim{}//////////wEaD{}Aw{}Q{}",
        parts[0], parts[1], parts[2], parts[3]
    )
}

/// ISO 8601 expiration timestamp (now + duration_seconds).
fn expiration_timestamp(duration_seconds: u64) -> String {
    // Use a simple offset calculation without an external time crate.
    // We know today is 2026-04-21; generate a static offset for testing parity.
    // In practice, for a local emulator, the exact wall-clock time is acceptable.
    // We use std::time::SystemTime to get UNIX epoch seconds.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expiry = now + duration_seconds;
    unix_to_iso8601(expiry)
}

/// Convert a UNIX timestamp to an ISO 8601 UTC string.
/// Implements a minimal conversion without external crates.
fn unix_to_iso8601(secs: u64) -> String {
    // Days since epoch
    let mut remaining = secs;
    let seconds = remaining % 60;
    remaining /= 60;
    let minutes = remaining % 60;
    remaining /= 60;
    let hours = remaining % 24;
    remaining /= 24;

    // Gregorian calendar calculation from days since 1970-01-01
    let (year, month, day) = days_to_ymd(remaining);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since 1970-01-01 to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Generate a `Credentials` JSON object.
fn generate_credentials(duration_seconds: u64) -> Value {
    json!({
        "AccessKeyId": fake_access_key_id(),
        "SecretAccessKey": fake_secret_access_key(),
        "SessionToken": fake_session_token(),
        "Expiration": expiration_timestamp(duration_seconds),
    })
}

/// Generate credentials + `AssumedRoleUser` for role assumption operations.
fn generate_assumed_role_output(
    role_arn: &str,
    session_name: &str,
    account_id: &str,
    duration_seconds: u64,
) -> (Value, Value) {
    let role_id_suffix = uuid::Uuid::new_v4().simple().to_string()[..20].to_uppercase();
    let assumed_role_id = format!("AROA{role_id_suffix}:{session_name}");

    // Derive the assumed-role ARN from the role ARN.
    // role ARN format: arn:aws:iam::ACCOUNT:role/ROLE-NAME
    let role_name = role_arn.split('/').next_back().unwrap_or("unknown-role");
    let assumed_role_arn =
        format!("arn:aws:sts::{account_id}:assumed-role/{role_name}/{session_name}");

    let credentials = generate_credentials(duration_seconds);
    let assumed_role_user = json!({
        "AssumedRoleId": assumed_role_id,
        "Arn": assumed_role_arn,
    });

    (credentials, assumed_role_user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use awsim_core::RequestContext;

    fn make_ctx() -> RequestContext {
        RequestContext::new("sts", "us-east-1")
    }

    #[test]
    fn test_fake_access_key_id_format() {
        let key = fake_access_key_id();
        assert!(key.starts_with("ASIA"), "must start with ASIA: {key}");
        assert_eq!(key.len(), 20, "must be 20 chars: {key}");
    }

    #[test]
    fn test_fake_secret_access_key_length() {
        let secret = fake_secret_access_key();
        assert_eq!(secret.len(), 40, "must be 40 chars: {secret}");
    }

    #[test]
    fn test_get_caller_identity_anonymous_returns_root() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let result = svc.get_caller_identity(&ctx).unwrap();
        assert_eq!(result["Account"], "000000000000");
        // Anonymous/unauthenticated → root shape; UserId equals account id.
        assert_eq!(result["UserId"], json!("000000000000"));
        assert_eq!(
            result["Arn"].as_str().unwrap(),
            "arn:aws:iam::000000000000:root"
        );
    }

    #[test]
    fn test_get_caller_identity_uses_access_key_when_present() {
        let svc = StsService::new();
        let mut ctx = make_ctx();
        ctx.access_key = Some("AKIATESTAKID000000".to_string());
        let result = svc.get_caller_identity(&ctx).unwrap();
        assert_eq!(result["UserId"], json!("AKIATESTAKID000000"));
        assert_eq!(
            result["Arn"].as_str().unwrap(),
            "arn:aws:iam::000000000000:user/AKIATESTAKID000000"
        );
    }

    #[test]
    fn test_assume_role_missing_role_arn() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({ "RoleSessionName": "my-session" });
        let err = svc.assume_role(&input, &ctx).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_missing_session_name() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({ "RoleArn": "arn:aws:iam::000000000000:role/MyRole" });
        let err = svc.assume_role(&input, &ctx).unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_success() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({
            "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
            "RoleSessionName": "test-session",
        });
        let result = svc.assume_role(&input, &ctx).unwrap();
        let creds = &result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
        assert_eq!(creds["SecretAccessKey"].as_str().unwrap().len(), 40);
        assert!(!creds["SessionToken"].as_str().unwrap().is_empty());
        assert!(!creds["Expiration"].as_str().unwrap().is_empty());

        let aru = &result["AssumedRoleUser"];
        assert!(
            aru["Arn"]
                .as_str()
                .unwrap()
                .contains("assumed-role/MyRole/test-session")
        );
        assert!(
            aru["AssumedRoleId"]
                .as_str()
                .unwrap()
                .contains("test-session")
        );
    }

    #[test]
    fn test_assume_role_rejects_malformed_arn() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "not-an-arn",
                    "RoleSessionName": "session",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("roleArn"));
    }

    #[test]
    fn test_assume_role_rejects_session_name_too_short() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "x",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("roleSessionName"));
    }

    #[test]
    fn test_assume_role_rejects_session_name_with_invalid_chars() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "bad name with spaces",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_accepts_partition_arn() {
        let svc = StsService::new();
        let ctx = make_ctx();
        svc.assume_role(
            &json!({
                "RoleArn": "arn:aws-us-gov:iam::000000000000:role/MyRole",
                "RoleSessionName": "session",
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn test_assume_role_accepts_role_with_path() {
        let svc = StsService::new();
        let ctx = make_ctx();
        svc.assume_role(
            &json!({
                "RoleArn": "arn:aws:iam::000000000000:role/team/dev/MyRole",
                "RoleSessionName": "session",
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn test_assume_role_rejects_invalid_policy_json() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "Policy": "{not-json",
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_rejects_too_many_policy_arns() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let arns: Vec<Value> = (0..11)
            .map(|i| json!({ "arn": format!("arn:aws:iam::aws:policy/P{i}") }))
            .collect();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "PolicyArns": arns,
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_rejects_malformed_policy_arn() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "PolicyArns": [{ "arn": "not-an-arn" }],
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_accepts_valid_session_policies() {
        let svc = StsService::new();
        let ctx = make_ctx();
        svc.assume_role(
            &json!({
                "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                "RoleSessionName": "session",
                "Policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
                "PolicyArns": [
                    { "arn": "arn:aws:iam::aws:policy/AdministratorAccess" },
                    { "arn": "arn:aws:iam::000000000000:policy/Custom" },
                ],
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn test_assume_role_rejects_duplicate_tag_keys() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "Tags": [
                        { "Key": "team", "Value": "infra" },
                        { "Key": "team", "Value": "data" },
                    ],
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("Duplicate"));
    }

    #[test]
    fn test_assume_role_rejects_transitive_tag_not_in_tags() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "Tags": [{ "Key": "team", "Value": "infra" }],
                    "TransitiveTagKeys": ["nonexistent"],
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn test_assume_role_accepts_well_formed_tags() {
        let svc = StsService::new();
        let ctx = make_ctx();
        svc.assume_role(
            &json!({
                "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                "RoleSessionName": "session",
                "Tags": [
                    { "Key": "team", "Value": "infra" },
                    { "Key": "env", "Value": "prod" },
                ],
                "TransitiveTagKeys": ["team"],
            }),
            &ctx,
        )
        .unwrap();
    }

    #[test]
    fn test_assume_role_rejects_duration_below_900() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let err = svc
            .assume_role(
                &json!({
                    "RoleArn": "arn:aws:iam::000000000000:role/MyRole",
                    "RoleSessionName": "session",
                    "DurationSeconds": 60u64,
                }),
                &ctx,
            )
            .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("durationSeconds"));
    }

    #[test]
    fn test_get_session_token_success() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({});
        let result = svc.get_session_token(&input, &ctx).unwrap();
        let creds = &result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
    }

    #[test]
    fn test_assume_role_with_web_identity_success() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({
            "RoleArn": "arn:aws:iam::000000000000:role/WebRole",
            "RoleSessionName": "web-session",
            "WebIdentityToken": "fake-oidc-token",
        });
        let result = svc.assume_role_with_web_identity(&input, &ctx).unwrap();
        let creds = &result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
    }

    #[test]
    fn test_assume_role_with_saml_success() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let input = json!({
            "RoleArn": "arn:aws:iam::000000000000:role/SAMLRole",
            "PrincipalArn": "arn:aws:iam::000000000000:saml-provider/MyProvider",
            "SAMLAssertion": "base64-encoded-saml-assertion",
        });
        let result = svc.assume_role_with_saml(&input, &ctx).unwrap();
        let creds = &result["Credentials"];
        assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
        let aru = &result["AssumedRoleUser"];
        assert!(
            aru["Arn"]
                .as_str()
                .unwrap()
                .contains("assumed-role/SAMLRole")
        );
    }

    #[test]
    fn test_unknown_operation() {
        // The handle() dispatch is synchronous for the unknown-operation branch,
        // so we can call the internal match directly via a blocking future executor.
        let svc = StsService::new();
        let ctx = make_ctx();
        // Drive the async fn to completion with a minimal single-threaded executor.
        let fut = svc.handle("NonExistentOp", json!({}), &ctx);
        let err = futures_executor_block_on(fut).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }

    /// Minimal blocking executor that drives a future to completion on the current thread.
    fn futures_executor_block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }

        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn test_expiration_timestamp_format() {
        let ts = expiration_timestamp(3600);
        // Must match YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'), "must end with Z: {ts}");
        assert_eq!(ts.len(), 20, "must be 20 chars: {ts}");
    }
}
