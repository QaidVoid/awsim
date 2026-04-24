use md5::{Digest, Md5};

/// Compute an MD5 ETag for the given data (wrapped in quotes as S3 does).
pub fn compute_etag(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("\"{:x}\"", result)
}

/// Return the current time as an ISO 8601 UTC string.
/// Example: `2026-04-21T14:48:01.000Z`
///
/// The S3 SDK expects ISO 8601 for `CreationDate` in `ListAllMyBucketsResult`.
pub fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso8601(secs)
}

/// Format a Unix timestamp as ISO 8601 (e.g. `2026-04-21T14:48:01.000Z`).
pub fn format_iso8601(secs: u64) -> String {
    let s = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let mut days = hours / 24;
    let mut y = 1970u64;
    loop {
        let dy = if is_leap_year(y) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        y += 1;
    }
    let months = if is_leap_year(y) {
        [31u64, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 0usize;
    loop {
        if days < months[mo] {
            break;
        }
        days -= months[mo];
        mo += 1;
    }
    let d = days + 1;
    format!("{y:04}-{:02}-{d:02}T{h:02}:{min:02}:{s:02}.000Z", mo + 1)
}

/// Convert an RFC 7231 date string to ISO 8601 via Unix epoch (best-effort).
///
/// S3 stores `LastModified` as RFC 7231 (e.g. `Tue, 21 Apr 2026 14:52:46 GMT`)
/// internally but the S3 XML API (ListObjectsV2 etc.) requires ISO 8601
/// (e.g. `2026-04-21T14:52:46.000Z`).  This function converts between the
/// two formats.  If parsing fails the string is returned unchanged.
pub fn rfc7231_to_iso8601(rfc7231: &str) -> String {
    // Parse: "<day-name>, <dd> <Mon> <yyyy> <HH>:<mm>:<ss> GMT"
    // Example: "Tue, 21 Apr 2026 14:52:46 GMT"
    let parts: Vec<&str> = rfc7231.splitn(2, ", ").collect();
    let rest = if parts.len() == 2 {
        parts[1]
    } else {
        return rfc7231.to_string();
    };

    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() < 4 {
        return rfc7231.to_string();
    }

    let day: u64 = tokens[0].parse().unwrap_or(1);
    let month = MONTH_NAMES
        .iter()
        .position(|&m| m == tokens[1])
        .map(|i| i as u64 + 1)
        .unwrap_or(1);
    let year: u64 = tokens[2].parse().unwrap_or(1970);
    let time_parts: Vec<&str> = tokens[3].split(':').collect();
    if time_parts.len() < 3 {
        return rfc7231.to_string();
    }
    let h: u64 = time_parts[0].parse().unwrap_or(0);
    let min: u64 = time_parts[1].parse().unwrap_or(0);
    let s: u64 = time_parts[2].parse().unwrap_or(0);

    format!("{year:04}-{month:02}-{day:02}T{h:02}:{min:02}:{s:02}.000Z")
}

/// Return the current time as an RFC 7231 date string.
/// Example: `Mon, 21 Apr 2026 12:00:00 GMT`
pub fn now_rfc7231() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    format_rfc7231(secs)
}

/// Format a Unix timestamp as RFC 7231.
pub fn format_rfc7231(secs: u64) -> String {
    // Days per month (non-leap year, then leap-year Jan is fine since we compute the actual year).
    const DAYS_IN_MONTH: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    const DAY_NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let sec = secs % 60;
    let min = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let mut days = secs / 86400;

    // Day of week (Unix epoch = Thursday = index 0)
    let day_of_week = DAY_NAMES[(days % 7) as usize];

    // Compute year, month, day from epoch days.
    let mut year = 1970u64;
    loop {
        let is_leap = is_leap_year(year);
        let days_in_year = if is_leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let is_leap = is_leap_year(year);
    let mut month = 0usize;
    loop {
        let days_in_month = if month == 1 && is_leap {
            29
        } else {
            DAYS_IN_MONTH[month]
        };
        if days < days_in_month {
            break;
        }
        days -= days_in_month;
        month += 1;
    }

    let day = days + 1;

    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        day_of_week, day, MONTH_NAMES[month], year, hour, min, sec
    )
}

fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_etag() {
        // MD5 of empty string
        let etag = compute_etag(b"");
        assert_eq!(etag, "\"d41d8cd98f00b204e9800998ecf8427e\"");
    }

    #[test]
    fn test_format_rfc7231_known() {
        // Unix epoch 0 = Thu, 01 Jan 1970 00:00:00 GMT
        let s = format_rfc7231(0);
        assert_eq!(s, "Thu, 01 Jan 1970 00:00:00 GMT");
    }
}
