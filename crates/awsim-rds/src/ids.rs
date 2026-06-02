/// Return the default engine version for a given engine name.
pub fn default_engine_version(engine: &str) -> &'static str {
    match engine {
        "postgres" => "16.1",
        "mysql" => "8.0.35",
        "mariadb" => "10.11.6",
        "docdb" => "5.0.0",
        "neptune" => "1.3.1.0",
        _ => "1.0",
    }
}

/// Return the default port for a given engine name.
pub fn default_port(engine: &str) -> u16 {
    match engine {
        "postgres" => 5432,
        "docdb" => 27017,
        "neptune" => 8182,
        _ => 3306,
    }
}

/// Build an endpoint address for a DB instance.
pub fn instance_endpoint(identifier: &str, region: &str) -> String {
    format!("{identifier}.awsim.{region}.rds.localhost")
}

/// Build a cluster endpoint address.
pub fn cluster_endpoint(identifier: &str, region: &str) -> String {
    format!("{identifier}.cluster.awsim.{region}.rds.localhost")
}

/// Build a cluster reader endpoint address.
pub fn cluster_reader_endpoint(identifier: &str, region: &str) -> String {
    format!("{identifier}.cluster-ro.awsim.{region}.rds.localhost")
}

/// Build a DB instance ARN.
pub fn instance_arn(partition: &str, region: &str, account: &str, identifier: &str) -> String {
    format!("arn:{partition}:rds:{region}:{account}:db:{identifier}")
}

/// Build a DB cluster ARN.
pub fn cluster_arn(partition: &str, region: &str, account: &str, identifier: &str) -> String {
    format!("arn:{partition}:rds:{region}:{account}:cluster:{identifier}")
}

/// Build a DB subnet group ARN.
pub fn subnet_group_arn(partition: &str, region: &str, account: &str, name: &str) -> String {
    format!("arn:{partition}:rds:{region}:{account}:subgrp:{name}")
}

/// Build a DB parameter group ARN.
pub fn parameter_group_arn(partition: &str, region: &str, account: &str, name: &str) -> String {
    format!("arn:{partition}:rds:{region}:{account}:pg:{name}")
}

/// Build a DB snapshot ARN.
pub fn snapshot_arn(partition: &str, region: &str, account: &str, identifier: &str) -> String {
    format!("arn:{partition}:rds:{region}:{account}:snapshot:{identifier}")
}

/// Build a DB cluster endpoint ARN.
pub fn cluster_endpoint_arn(
    partition: &str,
    region: &str,
    account: &str,
    endpoint_identifier: &str,
) -> String {
    format!("arn:{partition}:rds:{region}:{account}:cluster-endpoint:{endpoint_identifier}")
}

/// Build a cluster custom endpoint address.
pub fn cluster_custom_endpoint(endpoint_identifier: &str, region: &str) -> String {
    format!("{endpoint_identifier}.cluster-custom.awsim.{region}.rds.localhost")
}

/// Current UTC timestamp in ISO 8601 format (same impl as awsim-iam).
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
