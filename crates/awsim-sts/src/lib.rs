use awsim_core::{AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::{Value, json};
use tracing::debug;

pub struct StsService;

impl StsService {
    pub fn new() -> Self {
        Self
    }

    fn get_caller_identity(&self, ctx: &RequestContext) -> Result<Value, AwsError> {
        debug!(account_id = %ctx.account_id, "GetCallerIdentity");
        Ok(json!({
            "Account": ctx.account_id,
            "Arn": format!("arn:aws:iam::{}:root", ctx.account_id),
            "UserId": "AIDEXAMPLESTSUSERID",
        }))
    }

    fn assume_role(&self, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
        let role_arn = input["RoleArn"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleArn is required"))?;

        let session_name = input["RoleSessionName"]
            .as_str()
            .ok_or_else(|| AwsError::validation("RoleSessionName is required"))?;

        let duration = input["DurationSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| input["DurationSeconds"].as_u64())
            .unwrap_or(3600);

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

        let session_name = role_arn
            .split('/')
            .last()
            .unwrap_or("SAMLSession");

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
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
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

    format!(
        "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z"
    )
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
    let role_id_suffix = uuid::Uuid::new_v4().simple().to_string()[..20]
        .to_uppercase();
    let assumed_role_id = format!("AROA{role_id_suffix}:{session_name}");

    // Derive the assumed-role ARN from the role ARN.
    // role ARN format: arn:aws:iam::ACCOUNT:role/ROLE-NAME
    let role_name = role_arn.split('/').last().unwrap_or("unknown-role");
    let assumed_role_arn = format!(
        "arn:aws:sts::{account_id}:assumed-role/{role_name}/{session_name}"
    );

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
    fn test_get_caller_identity() {
        let svc = StsService::new();
        let ctx = make_ctx();
        let result = svc.get_caller_identity(&ctx).unwrap();
        assert_eq!(result["Account"], "000000000000");
        assert!(result["Arn"].as_str().unwrap().contains("arn:aws:iam::"));
        assert!(result["UserId"].as_str().is_some());
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
        assert!(aru["Arn"].as_str().unwrap().contains("assumed-role/MyRole/test-session"));
        assert!(aru["AssumedRoleId"].as_str().unwrap().contains("test-session"));
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
        assert!(aru["Arn"].as_str().unwrap().contains("assumed-role/SAMLRole"));
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

        fn noop_clone(_: *const ()) -> RawWaker { noop_raw_waker() }
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
