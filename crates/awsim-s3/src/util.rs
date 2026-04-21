use md5::{Digest, Md5};

/// Compute an MD5 ETag for the given data (wrapped in quotes as S3 does).
pub fn compute_etag(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("\"{:x}\"", result)
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
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
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
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
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
