//! RFC 6238 Time-based One-Time Password (TOTP) verifier.
//!
//! Used by the IAM service for MFA device enrolment and any future
//! operator-auth flow that prompts for a 6-digit code from a
//! virtual MFA app (Google Authenticator, 1Password, etc.).
//!
//! Implements the AWS-style virtual-MFA shape: HMAC-SHA1, 30-second
//! step, 6-digit code, base32 secret. The verifier accepts a small
//! window around the current time slice to absorb clock skew between
//! the device and the server.

use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha1 = Hmac<Sha1>;

/// AWS-style virtual MFA defaults: 30 s step, 6 digits, SHA-1.
const STEP_SECONDS: u64 = 30;
const DIGITS: u32 = 6;

/// Decode a base32-encoded TOTP secret (no padding, RFC 4648
/// alphabet). Returns `None` on a malformed seed.
pub fn decode_base32(seed: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = Vec::with_capacity(seed.len() * 5 / 8 + 1);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for c in seed.chars().filter(|c| *c != '=' && !c.is_whitespace()) {
        let upper = c.to_ascii_uppercase();
        let pos = ALPHABET.iter().position(|&a| a == upper as u8)?;
        buf = (buf << 5) | pos as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1u32 << bits) - 1;
        }
    }
    Some(out)
}

/// Compute the 6-digit TOTP code for `secret` at time slice
/// `unix_seconds / 30`.
pub fn code_at(secret: &[u8], unix_seconds: u64) -> u32 {
    let counter = unix_seconds / STEP_SECONDS;
    hotp(secret, counter)
}

/// String-shaped variant of [`verify`] for callers that already
/// have the 6-digit code as a `&str` (e.g. parsed straight from a
/// JSON request body). Rejects payloads that are not exactly six
/// ASCII digits without consulting the secret, so a malformed
/// caller can't probe the seed via timing.
pub fn verify_str(seed_base32: &str, code: &str, window_steps: i64) -> bool {
    if code.len() != DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Ok(parsed) = code.parse::<u32>() else {
        return false;
    };
    verify(seed_base32, parsed, SystemTime::now(), window_steps)
}

/// Verify `code` against `seed_base32` at `now`, tolerating
/// `window_steps` 30-second slices in either direction.
///
/// Returns `true` if any slice in `[now - window, now + window]`
/// produces `code`. AWS's `EnableMFADevice` flow accepts two
/// *consecutive* codes; callers verifying that flow run this twice
/// with the second code and `now += STEP_SECONDS`.
pub fn verify(seed_base32: &str, code: u32, now: SystemTime, window_steps: i64) -> bool {
    let Some(secret) = decode_base32(seed_base32) else {
        return false;
    };
    let Ok(secs) = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()) else {
        return false;
    };
    let center = (secs / STEP_SECONDS) as i64;
    for offset in -window_steps..=window_steps {
        let slice = center + offset;
        if slice < 0 {
            continue;
        }
        let counter = slice as u64;
        if hotp(&secret, counter) == code {
            return true;
        }
    }
    false
}

/// Verify two consecutive 30-second-window codes, the way AWS
/// `EnableMFADevice` and `ResyncMFADevice` do. Returns true when
/// both codes match for back-to-back time slices anywhere in the
/// tolerance window.
pub fn verify_consecutive(
    seed_base32: &str,
    code1: u32,
    code2: u32,
    now: SystemTime,
    window_steps: i64,
) -> bool {
    let Some(secret) = decode_base32(seed_base32) else {
        return false;
    };
    let Ok(secs) = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()) else {
        return false;
    };
    let center = (secs / STEP_SECONDS) as i64;
    for offset in -window_steps..=window_steps {
        let slice = center + offset;
        if slice < 0 {
            continue;
        }
        let c1 = hotp(&secret, slice as u64);
        let c2 = hotp(&secret, (slice + 1) as u64);
        if c1 == code1 && c2 == code2 {
            return true;
        }
    }
    false
}

fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac = HmacSha1::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let bin = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    bin % 10u32.pow(DIGITS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// RFC 6238 Appendix B test vector: secret "12345678901234567890"
    /// at t=59 yields 287082.
    #[test]
    fn rfc6238_vector_t59() {
        let secret = b"12345678901234567890";
        let counter = 59 / STEP_SECONDS;
        assert_eq!(hotp(secret, counter), 287082);
    }

    #[test]
    fn rfc6238_vector_t1111111109() {
        let secret = b"12345678901234567890";
        let counter = 1_111_111_109 / STEP_SECONDS;
        assert_eq!(hotp(secret, counter), 81804);
    }

    #[test]
    fn base32_round_trip_aws_example() {
        // The AWS-style base32 seed for "12345678901234567890" is
        // "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".
        let decoded = decode_base32("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap();
        assert_eq!(decoded, b"12345678901234567890");
    }

    #[test]
    fn base32_handles_lowercase_and_whitespace() {
        let decoded = decode_base32("ge zd gn bv").unwrap();
        let upper = decode_base32("GEZDGNBV").unwrap();
        assert_eq!(decoded, upper);
    }

    #[test]
    fn base32_rejects_invalid_character() {
        assert!(decode_base32("0123!@").is_none());
    }

    #[test]
    fn verify_accepts_matching_code_within_window() {
        let seed = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        let secret = decode_base32(seed).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(59);
        let code = hotp(&secret, 59 / STEP_SECONDS);
        assert!(verify(seed, code, now, 1));
    }

    #[test]
    fn verify_rejects_wrong_code() {
        let seed = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        let now = UNIX_EPOCH + Duration::from_secs(59);
        assert!(!verify(seed, 999_999, now, 1));
    }

    #[test]
    fn verify_consecutive_accepts_back_to_back_codes() {
        let seed = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        let secret = decode_base32(seed).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(60);
        let slice = 60 / STEP_SECONDS;
        let code1 = hotp(&secret, slice);
        let code2 = hotp(&secret, slice + 1);
        assert!(verify_consecutive(seed, code1, code2, now, 1));
    }

    #[test]
    fn verify_consecutive_rejects_swapped_order() {
        let seed = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        let secret = decode_base32(seed).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(60);
        let slice = 60 / STEP_SECONDS;
        let code1 = hotp(&secret, slice);
        let code2 = hotp(&secret, slice + 1);
        assert!(!verify_consecutive(seed, code2, code1, now, 1));
    }

    #[test]
    fn verify_with_malformed_seed_returns_false() {
        let now = UNIX_EPOCH + Duration::from_secs(59);
        assert!(!verify("not-base32!@#", 0, now, 1));
    }
}
