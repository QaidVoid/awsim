use std::time::{SystemTime, UNIX_EPOCH};

/// Current UNIX time in whole seconds since the epoch. Used as the
/// machine-readable creation stamp for resources (e.g. archives) where
/// the ISO 8601 `creation_time` string is display-only and must not be
/// parsed back. Returns 0 if the clock is somehow before the epoch.
pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format the current UTC time as an ISO 8601 string suitable for the
/// EventBridge timestamp fields (CreationTime, LastModifiedTime,
/// ReplayStartTime, …). Example: `2026-05-04T09:00:00.000Z`.
pub fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso8601(secs)
}

fn format_iso8601(secs: u64) -> String {
    let s = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let mut days = hours / 24;
    let mut y = 1970u64;
    loop {
        let leap = is_leap_year(y);
        let dy = if leap { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        y += 1;
    }
    let leap = is_leap_year(y);
    let months = if leap {
        [31u64, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 0usize;
    while days >= months[mo] {
        days -= months[mo];
        mo += 1;
    }
    let d = days + 1;
    format!("{y:04}-{:02}-{d:02}T{h:02}:{min:02}:{s:02}.000Z", mo + 1)
}

fn is_leap_year(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_unix_epoch_zero() {
        assert_eq!(format_iso8601(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn format_known_timestamp() {
        // 2024-01-01T00:00:00 UTC = 1704067200
        assert_eq!(format_iso8601(1_704_067_200), "2024-01-01T00:00:00.000Z");
    }

    #[test]
    fn format_round_trips_to_iso_shape() {
        let s = now_iso8601();
        assert_eq!(s.len(), 24);
        assert!(s.ends_with('Z'));
        assert!(s.chars().nth(4) == Some('-'));
        assert!(s.chars().nth(10) == Some('T'));
    }
}
