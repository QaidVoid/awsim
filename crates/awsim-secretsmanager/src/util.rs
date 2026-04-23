use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    secs_to_iso8601(secs)
}

pub fn secs_to_iso8601(secs: u64) -> String {
    let (y, mo, d, h, min, s) = unix_to_ymd_hms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
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
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
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
