use uuid::Uuid;

/// Generate an EC2 resource ID: `{prefix}-{8 hex chars}`, e.g. `vpc-1a2b3c4d`.
pub fn new_ec2_id(prefix: &str) -> String {
    let bytes = Uuid::new_v4();
    let hex: String = bytes.as_bytes()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{prefix}-{hex}")
}

/// Current UTC timestamp in ISO 8601 format (reused from IAM pattern).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ec2_id_format() {
        let id = new_ec2_id("vpc");
        assert!(
            id.starts_with("vpc-"),
            "vpc id should start with vpc-: {id}"
        );
        assert_eq!(id.len(), 12); // "vpc-" (4) + 8 hex = 12
    }

    #[test]
    fn sg_id_format() {
        let id = new_ec2_id("sg");
        assert!(id.starts_with("sg-"), "sg id should start with sg-: {id}");
        assert_eq!(id.len(), 11); // "sg-" (3) + 8 hex = 11
    }
}
