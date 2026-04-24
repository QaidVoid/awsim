use uuid::Uuid;

const UPPER_ALPHANUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const MIXED_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Generate `len` random characters from the given alphabet using UUID bytes as entropy.
fn random_chars(alphabet: &[u8], len: usize) -> String {
    // We use multiple UUIDs to ensure enough entropy for longer strings.
    let mut bytes = Vec::new();
    while bytes.len() < len {
        bytes.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    bytes[..len]
        .iter()
        .map(|b| alphabet[(*b as usize) % alphabet.len()] as char)
        .collect()
}

/// AIDA + 16 uppercase alphanumeric chars (total 20).
pub fn new_user_id() -> String {
    format!("AIDA{}", random_chars(UPPER_ALPHANUM, 16))
}

/// AGPA + 16 uppercase alphanumeric chars (total 20).
pub fn new_group_id() -> String {
    format!("AGPA{}", random_chars(UPPER_ALPHANUM, 16))
}

/// AROA + 16 uppercase alphanumeric chars (total 20).
pub fn new_role_id() -> String {
    format!("AROA{}", random_chars(UPPER_ALPHANUM, 16))
}

/// ANPA + 16 uppercase alphanumeric chars (total 20).
pub fn new_policy_id() -> String {
    format!("ANPA{}", random_chars(UPPER_ALPHANUM, 16))
}

/// AIPA + 16 uppercase alphanumeric chars (total 20).
pub fn new_instance_profile_id() -> String {
    format!("AIPA{}", random_chars(UPPER_ALPHANUM, 16))
}

/// AKIA + 16 uppercase alphanumeric chars (total 20).
pub fn new_access_key_id() -> String {
    format!("AKIA{}", random_chars(UPPER_ALPHANUM, 16))
}

/// 40 random mixed-case + digit + special chars for SecretAccessKey.
pub fn new_secret_access_key() -> String {
    random_chars(MIXED_CHARS, 40)
}

/// ASCA + 16 uppercase alphanumeric chars (total 20) for server certificates.
pub fn new_server_certificate_id() -> String {
    format!("ASCA{}", random_chars(UPPER_ALPHANUM, 16))
}

/// APKA + 16 uppercase alphanumeric chars (total 20) for SSH public keys.
pub fn new_ssh_public_key_id() -> String {
    format!("APKA{}", random_chars(UPPER_ALPHANUM, 16))
}

/// Generate a random base32 seed (40 chars) for MFA devices.
pub fn new_base32_seed() -> String {
    const BASE32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    random_chars(BASE32, 40)
}

/// Generate a new UUID string.
pub fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// Current UTC timestamp in ISO 8601 format.
pub fn now_iso8601() -> String {
    // Use a fixed-format timestamp. Since we don't pull in chrono, we read
    // UNIX time via std and format manually.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Convert UNIX seconds to Y-M-D H:M:S (no leap-second awareness, good enough for emulation).
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

    // Epoch is 1970-01-01
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

/// Build a normalised path: ensures leading and trailing slash.
pub fn normalize_path(path: Option<&str>) -> String {
    match path {
        None | Some("") => "/".to_string(),
        Some(p) => {
            let p = if p.starts_with('/') {
                p.to_string()
            } else {
                format!("/{p}")
            };
            if p.ends_with('/') { p } else { format!("{p}/") }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_id_format() {
        let id = new_user_id();
        assert!(
            id.starts_with("AIDA"),
            "user id should start with AIDA: {id}"
        );
        assert_eq!(id.len(), 20);
    }

    #[test]
    fn access_key_format() {
        let id = new_access_key_id();
        assert!(
            id.starts_with("AKIA"),
            "access key id should start with AKIA: {id}"
        );
        assert_eq!(id.len(), 20);
    }

    #[test]
    fn secret_key_length() {
        let sk = new_secret_access_key();
        assert_eq!(sk.len(), 40);
    }

    #[test]
    fn normalize_path_variants() {
        assert_eq!(normalize_path(None), "/");
        assert_eq!(normalize_path(Some("/")), "/");
        assert_eq!(normalize_path(Some("/engineering")), "/engineering/");
        assert_eq!(normalize_path(Some("/engineering/")), "/engineering/");
    }
}
