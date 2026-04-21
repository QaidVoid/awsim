use uuid::Uuid;

/// Generate a CloudFront distribution ID: `E` + 13 uppercase alphanumeric chars.
pub fn new_distribution_id() -> String {
    let bytes = Uuid::new_v4();
    let hex: String = bytes
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<String>()
        .chars()
        .take(13)
        .collect();
    format!("E{hex}")
}

/// Generate an OAC ID (same format as distribution ID).
pub fn new_oac_id() -> String {
    new_distribution_id()
}

/// Distribution ARN (CloudFront is global — no region in ARN).
pub fn distribution_arn(account: &str, id: &str) -> String {
    format!("arn:aws:cloudfront::{account}:distribution/{id}")
}

/// Domain name for a distribution.
pub fn distribution_domain(id: &str) -> String {
    format!("{id}.cloudfront.localhost")
}

/// Generate a random ETag-like string.
pub fn new_etag() -> String {
    let bytes = Uuid::new_v4();
    bytes
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<String>()
        .chars()
        .take(13)
        .collect()
}

/// Current UTC timestamp in ISO 8601 format.
pub fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
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
