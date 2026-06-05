use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_types::region::Region;

#[derive(Debug)]
pub enum OpResult {
    Pass(String),
    Fail(String, String),
    NotImplemented(String),
    Skipped(String),
}

impl OpResult {
    pub fn op_name(&self) -> &str {
        match self {
            OpResult::Pass(n)
            | OpResult::Fail(n, _)
            | OpResult::NotImplemented(n)
            | OpResult::Skipped(n) => n,
        }
    }

    pub fn is_pass(&self) -> bool {
        matches!(self, OpResult::Pass(_))
    }

    pub fn is_fail(&self) -> bool {
        matches!(self, OpResult::Fail(_, _))
    }

    #[allow(dead_code)]
    pub fn is_not_implemented(&self) -> bool {
        matches!(self, OpResult::NotImplemented(_))
    }
}

pub struct ServiceResult {
    pub service: String,
    pub total: usize,
    pub implemented: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<OpResult>,
}

pub async fn make_config(endpoint: &str) -> aws_config::SdkConfig {
    make_config_with_key(endpoint, "test", "test").await
}

/// Like [`make_config`] but with an explicit access key, for auth-gating tests
/// that need to act as a specific principal (admin bypass vs. a low-privilege
/// IAM user vs. an unknown key).
pub async fn make_config_with_key(
    endpoint: &str,
    access_key: &str,
    secret_key: &str,
) -> aws_config::SdkConfig {
    let creds = Credentials::new(access_key, secret_key, None, None, "conformance");
    aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(creds)
        .endpoint_url(endpoint)
        .load()
        .await
}

pub fn categorise(op: &str, result: Result<(), String>, verbose: bool) -> OpResult {
    match result {
        Ok(_) => {
            if verbose {
                println!("  PASS {op}");
            }
            OpResult::Pass(op.to_string())
        }
        Err(e) => {
            if e.contains("NotImplemented") || e.contains("UnknownOperationException") {
                if verbose {
                    println!("  SKIP {op}: not implemented");
                }
                OpResult::NotImplemented(op.to_string())
            } else if is_deserialization_error(&e) {
                if verbose {
                    println!("  FAIL {op}: {e}");
                }
                OpResult::Fail(op.to_string(), e)
            } else {
                if verbose {
                    println!("  PASS {op} (service error: {})", truncate(&e, 120));
                }
                OpResult::Pass(op.to_string())
            }
        }
    }
}

pub fn is_deserialization_error(err: &str) -> bool {
    // An `Unhandled` error that still carries a parsed `code: Some(..)` is a
    // valid service error whose code just isn't in the model (e.g.
    // AccessDenied, MissingParameter) — not a deserialization failure. Only
    // flag `Unhandled` when no code was extracted, i.e. the response couldn't
    // be parsed into a recognizable AWS error at all.
    let unhandled_without_code =
        err.contains("Unhandled(Unhandled") && !err.contains("code: Some(");
    err.contains("ResponseDeserializationError")
        || unhandled_without_code
        || (err.contains("failed to deserialize") && !err.contains("ServiceError"))
        || err.contains("InvalidXml")
        || err.contains("DecodeError")
}

pub fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}

pub fn sdk_err_to_string<E: std::fmt::Debug>(e: E) -> String {
    format!("{e:?}")
}

/// Assert the call fails with a specific AWS error code.
///
/// `categorise` passes on any service error, so it cannot catch "awsim
/// accepted a request real AWS rejects". This asserts the rejection and its
/// code, turning an AWS-verified rule into a regression guard. `SdkError`
/// forwards `ProvideErrorMetadata`, so the operation `Result` passes straight in.
pub fn expect_err_code<T, E>(op: &str, r: Result<T, E>, want_code: &str, verbose: bool) -> OpResult
where
    E: aws_smithy_types::error::metadata::ProvideErrorMetadata,
{
    match r {
        Ok(_) => OpResult::Fail(
            op.to_string(),
            format!("expected error {want_code}, got success"),
        ),
        Err(e) => match e.code() {
            Some(code) if code == want_code => {
                if verbose {
                    println!("  PASS {op} (rejected with {code})");
                }
                OpResult::Pass(op.to_string())
            }
            other => OpResult::Fail(
                op.to_string(),
                format!("expected error {want_code}, got {other:?}"),
            ),
        },
    }
}

/// Assert the call succeeds and its decoded output satisfies `check`. Use for
/// round-trips and value checks the envelope-only smoke test can't see.
pub fn expect_ok<T, E>(
    op: &str,
    r: Result<T, E>,
    check: impl FnOnce(&T) -> Result<(), String>,
    verbose: bool,
) -> OpResult
where
    E: std::fmt::Debug,
{
    match r {
        Err(e) => OpResult::Fail(op.to_string(), sdk_err_to_string(e)),
        Ok(v) => match check(&v) {
            Ok(()) => {
                if verbose {
                    println!("  PASS {op}");
                }
                OpResult::Pass(op.to_string())
            }
            Err(msg) => OpResult::Fail(op.to_string(), msg),
        },
    }
}

#[macro_export]
macro_rules! chk {
    ($op:expr, $result:expr, $verbose:expr) => {
        $crate::runner::common::categorise(
            $op,
            $result
                .map(|_| ())
                .map_err(|e| $crate::runner::common::sdk_err_to_string(e)),
            $verbose,
        )
    };
}
