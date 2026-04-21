use awsim_core::AwsError;
use md5::{Digest, Md5};

/// Compute the MD5 hex digest of a string (used for MD5OfMessageBody).
pub fn md5_of(s: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract the queue name from a queue URL.
///
/// URL format: `http://sqs.{region}.localhost:4566/{account_id}/{queue_name}`
pub fn queue_name_from_url(url: &str) -> Result<String, AwsError> {
    // Split on '/' and take the last segment
    url.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvalidAddress",
                format!("The address {url} is not valid for this endpoint."),
            )
        })
}
