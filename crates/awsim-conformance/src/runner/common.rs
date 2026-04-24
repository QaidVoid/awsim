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
    let creds = Credentials::new("test", "test", None, None, "conformance");
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
    err.contains("ResponseDeserializationError")
        || err.contains("Unhandled(Unhandled { source: Error")
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
