use uuid::Uuid;

/// Generate a random 16-hex-char suffix.
pub fn random_hex(len: usize) -> String {
    let bytes = Uuid::new_v4();
    bytes
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .chars()
        .take(len)
        .collect()
}

/// Load Balancer ARN.
/// Format: `arn:aws:elasticloadbalancing:{region}:{account}:loadbalancer/{lb_type}/{name}/{random}`
pub fn lb_arn(region: &str, account: &str, lb_type: &str, name: &str) -> String {
    // lb_type "application" -> "app", "network" -> "net"
    let type_prefix = match lb_type {
        "network" => "net",
        _ => "app",
    };
    let rand = random_hex(16);
    format!(
        "arn:aws:elasticloadbalancing:{region}:{account}:loadbalancer/{type_prefix}/{name}/{rand}"
    )
}

/// Target Group ARN.
pub fn tg_arn(region: &str, account: &str, name: &str) -> String {
    let rand = random_hex(16);
    format!("arn:aws:elasticloadbalancing:{region}:{account}:targetgroup/{name}/{rand}")
}

/// Listener ARN.
pub fn listener_arn(region: &str, account: &str, lb_name: &str, lb_rand: &str) -> String {
    let rand = random_hex(16);
    format!(
        "arn:aws:elasticloadbalancing:{region}:{account}:listener/app/{lb_name}/{lb_rand}/{rand}"
    )
}

/// Rule ARN.
pub fn rule_arn(
    region: &str,
    account: &str,
    lb_name: &str,
    lb_rand: &str,
    listener_rand: &str,
) -> String {
    let rand = random_hex(16);
    format!(
        "arn:aws:elasticloadbalancing:{region}:{account}:listener-rule/app/{lb_name}/{lb_rand}/{listener_rand}/{rand}"
    )
}

/// DNS name for a load balancer.
pub fn lb_dns_name(name: &str, region: &str) -> String {
    let rand = random_hex(8);
    format!("{name}-{rand}.{region}.elb.localhost")
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

/// Extract the random suffix from an ARN (last segment).
pub fn arn_suffix(arn: &str) -> &str {
    arn.rsplit('/').next().unwrap_or("")
}
