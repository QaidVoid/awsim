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
/// Format: `arn:{partition}:elasticloadbalancing:{region}:{account}:loadbalancer/{lb_type}/{name}/{random}`
pub fn lb_arn(partition: &str, region: &str, account: &str, lb_type: &str, name: &str) -> String {
    // lb_type "application" -> "app", "network" -> "net"
    let type_prefix = match lb_type {
        "network" => "net",
        _ => "app",
    };
    let rand = random_hex(16);
    format!(
        "arn:{partition}:elasticloadbalancing:{region}:{account}:loadbalancer/{type_prefix}/{name}/{rand}"
    )
}

/// Target Group ARN.
pub fn tg_arn(partition: &str, region: &str, account: &str, name: &str) -> String {
    let rand = random_hex(16);
    format!("arn:{partition}:elasticloadbalancing:{region}:{account}:targetgroup/{name}/{rand}")
}

/// Listener ARN.
pub fn listener_arn(
    partition: &str,
    region: &str,
    account: &str,
    lb_name: &str,
    lb_rand: &str,
) -> String {
    let rand = random_hex(16);
    format!(
        "arn:{partition}:elasticloadbalancing:{region}:{account}:listener/app/{lb_name}/{lb_rand}/{rand}"
    )
}

/// Rule ARN.
pub fn rule_arn(
    partition: &str,
    region: &str,
    account: &str,
    lb_name: &str,
    lb_rand: &str,
    listener_rand: &str,
) -> String {
    let rand = random_hex(16);
    format!(
        "arn:{partition}:elasticloadbalancing:{region}:{account}:listener-rule/app/{lb_name}/{lb_rand}/{listener_rand}/{rand}"
    )
}

/// DNS name for a load balancer. AWS prefixes the name with `internal-`
/// when the scheme is `internal`, and keeps the public host pattern
/// otherwise. The simulator mirrors that contract so cross-tool URL
/// matching (Route53 alias targets, app config) lines up.
pub fn lb_dns_name(name: &str, region: &str, scheme: &str) -> String {
    let rand = random_hex(8);
    let prefix = if scheme == "internal" {
        "internal-"
    } else {
        ""
    };
    format!("{prefix}{name}-{rand}.{region}.elb.localhost")
}

/// AWS publishes a per-region canonical hosted zone ID for each
/// load-balancer family. We synthesize a deterministic 14-char Z-id so
/// the field is stable across simulator restarts but still per-region.
/// `lb_type` may be one of `application`, `network`, `gateway` — they
/// share an ID space in our model.
pub fn canonical_hosted_zone_id(region: &str, lb_type: &str) -> String {
    // Real AWS uses fixed IDs per region; without a static table we
    // hash region + type to a 14-character upper-hex string with a
    // `Z` prefix. FNV-1a 64-bit keeps the computation tiny and
    // deterministic.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in region.as_bytes().iter().chain(lb_type.as_bytes().iter()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("Z{:013X}", hash & 0xFFFFFFFFFFFFFF)
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

#[cfg(test)]
mod dns_tests {
    use super::*;

    #[test]
    fn internal_scheme_prefixes_hostname() {
        let dns = lb_dns_name("api", "us-east-1", "internal");
        assert!(dns.starts_with("internal-api-"));
        assert!(dns.ends_with(".us-east-1.elb.localhost"));
    }

    #[test]
    fn public_scheme_does_not_prefix_hostname() {
        let dns = lb_dns_name("api", "us-east-1", "internet-facing");
        assert!(!dns.contains("internal-"));
        assert!(dns.starts_with("api-"));
    }

    #[test]
    fn canonical_hosted_zone_id_is_stable_per_region_and_type() {
        let a = canonical_hosted_zone_id("us-east-1", "application");
        let b = canonical_hosted_zone_id("us-east-1", "application");
        assert_eq!(a, b);
        assert!(a.starts_with('Z'));
        // 1 for the 'Z' + at most 14 upper-hex chars.
        assert!((10..=20).contains(&a.len()));
    }

    #[test]
    fn canonical_hosted_zone_id_differs_across_regions() {
        let east = canonical_hosted_zone_id("us-east-1", "application");
        let west = canonical_hosted_zone_id("us-west-2", "application");
        assert_ne!(east, west);
    }

    #[test]
    fn canonical_hosted_zone_id_differs_across_lb_types() {
        let app = canonical_hosted_zone_id("us-east-1", "application");
        let net = canonical_hosted_zone_id("us-east-1", "network");
        assert_ne!(app, net);
    }
}
