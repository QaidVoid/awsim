pub mod connections;
pub mod crawlers;
pub mod databases;
pub mod extras;
pub mod jobs;
pub mod tables;
pub mod tags;

use serde_json::Value;

/// Convert a stored timestamp string into an awsJson1.1 timestamp value (a JSON
/// number of epoch seconds). Accepts epoch seconds (with optional fraction) or
/// ISO-8601; anything unparseable falls back to the raw string.
pub(crate) fn ts(stored: &str) -> Value {
    if let Ok(secs) = stored.parse::<i64>() {
        return Value::from(secs);
    }
    if let Ok(secs) = stored.parse::<f64>() {
        return Value::from(secs);
    }
    if let Some(secs) = iso8601_to_epoch(stored) {
        return Value::from(secs);
    }
    Value::from(stored)
}

/// Convert an optional stored timestamp into a JSON number, or null when absent.
pub(crate) fn ts_opt(stored: &Option<String>) -> Value {
    match stored {
        Some(s) => ts(s),
        None => Value::Null,
    }
}

/// Parse a minimal subset of ISO-8601 ("YYYY-MM-DDTHH:MM:SS[Z]") to epoch seconds.
fn iso8601_to_epoch(s: &str) -> Option<i64> {
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    let time = time.split('.').next().unwrap_or(time);
    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next().unwrap_or("0").parse().ok()?;

    // Days since 1970-01-01 using a civil-from-days algorithm.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn epoch_seconds_string_becomes_number() {
        assert_eq!(ts("1780668227"), json!(1780668227));
        assert!(ts("1780668227").is_number());
    }

    #[test]
    fn fractional_epoch_becomes_number() {
        assert_eq!(ts("1780668227.5"), json!(1780668227.5));
    }

    #[test]
    fn iso8601_becomes_epoch_number() {
        assert_eq!(ts("2024-01-01T00:00:00Z"), json!(1704067200));
    }

    #[test]
    fn ts_opt_none_is_null() {
        assert!(ts_opt(&None).is_null());
        assert_eq!(ts_opt(&Some("1780668227".to_string())), json!(1780668227));
    }
}
