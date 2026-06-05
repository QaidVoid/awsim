use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::missing_parameter;

/// Extract a required string parameter from the input Value.
pub fn require_str<'a>(
    input: &'a serde_json::Value,
    key: &str,
) -> Result<&'a str, awsim_core::AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| missing_parameter(key))
}

/// Extract an optional string parameter from the input Value.
pub fn opt_str<'a>(input: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

/// Extract an optional u64 from the input Value.
pub fn opt_u64(input: &serde_json::Value, key: &str) -> Option<u64> {
    input.get(key).and_then(|v| v.as_u64())
}

/// Extract an optional bool from the input Value.
pub fn opt_bool(input: &serde_json::Value, key: &str) -> Option<bool> {
    input.get(key).and_then(|v| v.as_bool())
}

/// Current UTC timestamp in ISO 8601 format.
pub fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, min, s) = unix_to_ymd_hms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{min:02}:{s:02}.000+0000")
}

/// Parse an ISO 8601 timestamp produced by [`now_iso8601`]
/// (`YYYY-MM-DDTHH:MM:SS.fff+0000`) into UNIX epoch seconds. The restJson1
/// protocol serializes timestamp members as epoch-seconds numbers, so stored
/// ISO strings are converted at the response boundary. Returns 0 if the input
/// is not in the expected shape.
pub fn iso8601_to_epoch(s: &str) -> u64 {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return 0;
    }
    let num = |a: usize, b: usize| -> Option<u64> { s.get(a..b)?.parse().ok() };
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(sec)) = (
        num(0, 4),
        num(5, 7),
        num(8, 10),
        num(11, 13),
        num(14, 16),
        num(17, 19),
    ) else {
        return 0;
    };
    if !(1..=12).contains(&mo) {
        return 0;
    }
    let mut days = 0u64;
    for yr in 1970..y {
        days += if is_leap(yr) { 366 } else { 365 };
    }
    let months: [u64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for m in &months[..(mo as usize - 1)] {
        days += m;
    }
    days += d.saturating_sub(1);
    days * 86_400 + h * 3_600 + mi * 60 + sec
}

fn unix_to_ymd_hms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let days = hours / 24;
    let (y, doy) = days_to_year(days);
    let (mo, d) = doy_to_month_day(doy, is_leap(y));
    (y, mo, d, h, min, s)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn days_to_year(mut days: u64) -> (u64, u64) {
    let mut y = 1970u64;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if days < dy {
            return (y, days);
        }
        days -= dy;
        y += 1;
    }
}

fn doy_to_month_day(doy: u64, leap: bool) -> (u64, u64) {
    let months: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut rem = doy;
    for (i, &days) in months.iter().enumerate() {
        if rem < days {
            return ((i + 1) as u64, rem + 1);
        }
        rem -= days;
    }
    (12, 31)
}

/// Generate a new UUID v4 string.
pub fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// Decode base64 zip bytes and compute sha256 + size.
/// Returns (decoded_bytes, sha256_base64, size).
pub fn decode_zip(b64: &str) -> Result<(Vec<u8>, String, u64), awsim_core::AwsError> {
    let bytes = BASE64.decode(b64).map_err(|e| {
        awsim_core::AwsError::bad_request(
            "InvalidParameterValueException",
            format!("Invalid base64 ZipFile: {e}"),
        )
    })?;
    let hash = sha256_base64(&bytes);
    let size = bytes.len() as u64;
    Ok((bytes, hash, size))
}

/// Lambda managed runtimes accepted by CreateFunction / UpdateFunctionConfiguration
/// as of 2026. Excludes deprecated runtimes that AWS has fully retired —
/// callers passing those receive InvalidParameterValueException.
pub const VALID_RUNTIMES: &[&str] = &[
    // Node.js
    "nodejs18.x",
    "nodejs20.x",
    "nodejs22.x",
    // Python
    "python3.10",
    "python3.11",
    "python3.12",
    "python3.13",
    // Java
    "java11",
    "java17",
    "java21",
    // .NET
    "dotnet6",
    "dotnet8",
    // Ruby
    "ruby3.2",
    "ruby3.3",
    // Custom runtimes
    "provided.al2",
    "provided.al2023",
];

/// Validate a Runtime parameter against the AWS managed-runtime allow-list.
pub fn validate_runtime(runtime: &str) -> Result<(), awsim_core::AwsError> {
    if VALID_RUNTIMES.contains(&runtime) {
        return Ok(());
    }
    Err(awsim_core::AwsError::bad_request(
        "InvalidParameterValueException",
        format!(
            "Value {runtime} at 'runtime' failed to satisfy constraint: \
             Member must satisfy enum value set: [{}]",
            VALID_RUNTIMES.join(", ")
        ),
    ))
}

/// Validate a Handler parameter. AWS's Smithy constrains it to non-empty
/// strings of up to 128 characters with no whitespace. Format-checking
/// against the runtime's specific shape (e.g. `module.function` for
/// Python) is a runtime-time concern and not enforced here.
pub fn validate_handler(handler: &str) -> Result<(), awsim_core::AwsError> {
    if handler.is_empty() {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidParameterValueException",
            "Handler must not be empty",
        ));
    }
    if handler.len() > 128 {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidParameterValueException",
            "Handler must be at most 128 characters",
        ));
    }
    if handler.chars().any(char::is_whitespace) {
        return Err(awsim_core::AwsError::bad_request(
            "InvalidParameterValueException",
            "Handler must not contain whitespace",
        ));
    }
    Ok(())
}

/// Compute SHA-256 of bytes and return as base64.
pub fn sha256_base64(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    BASE64.encode(hasher.finalize())
}

/// Validate a Lambda function qualifier (the `?Qualifier=` query param
/// or the `:Qualifier` suffix on a function ARN).
///
/// AWS accepts three shapes:
/// - `$LATEST` — the unpublished current revision.
/// - A decimal version number (1..u64::MAX).
/// - An alias name matching `[a-zA-Z][a-zA-Z0-9-_]*`, 1..=128 characters.
///
/// Anything else is rejected with `InvalidParameterValueException` so
/// SDK callers see the same diagnostic locally that they would in
/// production.
pub fn validate_qualifier(qualifier: &str) -> Result<(), awsim_core::AwsError> {
    if qualifier == "$LATEST" {
        return Ok(());
    }
    if qualifier.chars().all(|c| c.is_ascii_digit()) && !qualifier.is_empty() {
        return Ok(());
    }
    if (1..=128).contains(&qualifier.len())
        && let Some(first) = qualifier.chars().next()
        && first.is_ascii_alphabetic()
        && qualifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Ok(());
    }
    Err(awsim_core::AwsError::bad_request(
        "InvalidParameterValueException",
        format!(
            "Qualifier `{qualifier}` is not valid. Must be $LATEST, a numeric version, \
             or an alias matching [a-zA-Z][a-zA-Z0-9-_]{{0,127}}."
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_to_epoch_unix_zero() {
        assert_eq!(iso8601_to_epoch("1970-01-01T00:00:00.000+0000"), 0);
    }

    #[test]
    fn iso8601_to_epoch_known() {
        // 2024-01-01T00:00:00 UTC = 1704067200
        assert_eq!(
            iso8601_to_epoch("2024-01-01T00:00:00.000+0000"),
            1_704_067_200
        );
    }

    #[test]
    fn iso8601_to_epoch_round_trips_now() {
        let s = now_iso8601();
        let epoch = iso8601_to_epoch(&s);
        assert!(epoch > 1_700_000_000);
    }

    #[test]
    fn iso8601_to_epoch_rejects_garbage() {
        assert_eq!(iso8601_to_epoch(""), 0);
        assert_eq!(iso8601_to_epoch("not-a-date"), 0);
    }

    #[test]
    fn validate_qualifier_accepts_dollar_latest() {
        validate_qualifier("$LATEST").unwrap();
    }

    #[test]
    fn validate_qualifier_accepts_numeric_version() {
        validate_qualifier("1").unwrap();
        validate_qualifier("42").unwrap();
    }

    #[test]
    fn validate_qualifier_accepts_alias_name() {
        validate_qualifier("prod").unwrap();
        validate_qualifier("blue-green_1").unwrap();
    }

    #[test]
    fn validate_qualifier_rejects_leading_digit_or_dash() {
        assert!(validate_qualifier("-prod").is_err());
        // Numeric is a separate accepted form; "1prod" is neither numeric
        // nor a valid alias (must start with a letter).
        assert!(validate_qualifier("1prod").is_err());
    }

    #[test]
    fn validate_qualifier_rejects_invalid_chars() {
        assert!(validate_qualifier("has space").is_err());
        assert!(validate_qualifier("dot.name").is_err());
        assert!(validate_qualifier("").is_err());
    }

    #[test]
    fn validate_qualifier_rejects_over_128_chars() {
        let long: String = "a".repeat(129);
        assert!(validate_qualifier(&long).is_err());
    }
}
