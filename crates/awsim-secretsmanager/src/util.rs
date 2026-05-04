use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Return the current time as a Unix epoch float (seconds + fraction).
///
/// The AWS JSON 1.1 protocol serialises timestamps as JSON numbers
/// (seconds since epoch), so this value should be embedded directly in
/// `serde_json::json!` macros as a number, not as a quoted string.
pub fn now_epoch_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Generate a new random UUID version ID.
pub fn new_version_id() -> String {
    Uuid::new_v4().to_string()
}

/// Validate a `ClientRequestToken` per AWS rules: 32–64 characters, ASCII
/// letters/digits/`-`/`_`. Returns the token as `String` on success.
pub fn validate_client_request_token(token: &str) -> Result<String, awsim_core::AwsError> {
    let len = token.chars().count();
    if !(32..=64).contains(&len) {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidParameterException",
            "ClientRequestToken must be between 32 and 64 characters",
        ));
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidParameterException",
            "ClientRequestToken may only contain letters, digits, '-', and '_'",
        ));
    }
    Ok(token.to_string())
}

/// Generate 6 random alphanumeric characters for ARN suffix.
pub fn random_suffix(len: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut bytes = Vec::new();
    while bytes.len() < len {
        bytes.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    bytes[..len]
        .iter()
        .map(|b| CHARS[(*b as usize) % CHARS.len()] as char)
        .collect()
}

/// Generate a random password for `GetRandomPassword`.
pub fn random_password(
    length: usize,
    exclude_uppercase: bool,
    exclude_lowercase: bool,
    exclude_numbers: bool,
    exclude_punctuation: bool,
) -> String {
    let mut charset: Vec<u8> = Vec::new();
    if !exclude_uppercase {
        charset.extend_from_slice(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    }
    if !exclude_lowercase {
        charset.extend_from_slice(b"abcdefghijklmnopqrstuvwxyz");
    }
    if !exclude_numbers {
        charset.extend_from_slice(b"0123456789");
    }
    if !exclude_punctuation {
        charset.extend_from_slice(b"!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~");
    }
    if charset.is_empty() {
        charset.extend_from_slice(b"abcdefghijklmnopqrstuvwxyz");
    }
    let mut raw: Vec<u8> = Vec::new();
    while raw.len() < length {
        raw.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    raw[..length]
        .iter()
        .map(|b| charset[(*b as usize) % charset.len()] as char)
        .collect()
}
