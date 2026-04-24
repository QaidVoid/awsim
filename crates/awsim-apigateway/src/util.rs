use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current time formatted as ISO 8601 (UTC).
pub fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    epoch_to_iso8601(secs)
}

/// Returns the current Unix epoch in seconds.
pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn epoch_to_iso8601(epoch: u64) -> String {
    let (year, month, day, hour, min, sec) = epoch_to_parts(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Format epoch as the Apache CLF time string used in API Gateway logs.
/// e.g., "21/Apr/2026:00:00:00 +0000"
pub fn epoch_to_clf(epoch: u64) -> String {
    let (year, month, day, hour, min, sec) = epoch_to_parts(epoch);
    let month_name = MONTH_NAMES[(month as usize).saturating_sub(1)];
    format!("{day:02}/{month_name}/{year:04}:{hour:02}:{min:02}:{sec:02} +0000")
}

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn epoch_to_parts(epoch: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = epoch % 60;
    let min_total = epoch / 60;
    let min = min_total % 60;
    let hour_total = min_total / 60;
    let hour = hour_total % 24;
    let days = hour_total / 24;

    let mut year = 1970u64;
    let mut remaining = days;

    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let months: [u64; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &dim in &months {
        if remaining < dim {
            break;
        }
        remaining -= dim;
        month += 1;
    }
    let day = remaining + 1;

    (year, month, day, hour, min, sec)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}
