//! RFC 6238 TOTP (Time-based One-Time Passwords) for Cognito MFA.
//!
//! AWS Cognito's `SOFTWARE_TOKEN_MFA` uses a standard 30-second TOTP based on
//! HMAC-SHA1 with a 20-byte secret, exactly matching the Google Authenticator
//! defaults. Verification accepts the codes for the previous, current, and
//! next time step (a +/- 30s window) to absorb clock skew, the same tolerance
//! Cognito advertises.

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

const STEP_SECONDS: u64 = 30;
const DIGITS: u32 = 6;

/// Decode an RFC 4648 (no-padding, uppercase) base32 string into bytes.
fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut buffer: u64 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::with_capacity((s.len() * 5) / 8);
    for c in s.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let value: u64 = match c {
            'A'..='Z' => (c as u32 - 'A' as u32) as u64,
            'a'..='z' => (c as u32 - 'a' as u32) as u64,
            '2'..='7' => (26 + (c as u32 - '2' as u32)) as u64,
            _ => return None,
        };
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Compute the TOTP code for a given step counter, formatted as a zero-padded
/// 6-digit string.
fn code_for_counter(secret: &[u8], counter: u64) -> String {
    let mut mac = HmacSha1::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let tag = mac.finalize().into_bytes();

    let offset = (tag[tag.len() - 1] & 0x0F) as usize;
    let bin = ((tag[offset] & 0x7F) as u32) << 24
        | (tag[offset + 1] as u32) << 16
        | (tag[offset + 2] as u32) << 8
        | (tag[offset + 3] as u32);
    let modulus = 10u32.pow(DIGITS);
    format!("{:0width$}", bin % modulus, width = DIGITS as usize)
}

/// Verify a user-supplied TOTP code against a base32-encoded secret.
///
/// Accepts the code for the previous, current, and next 30s window so a
/// reasonably-skewed client clock still passes. Comparison is timing-safe
/// to avoid leaking partial matches.
pub fn verify(secret_b32: &str, code: &str) -> bool {
    if code.len() != DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Some(secret) = base32_decode(secret_b32) else {
        return false;
    };
    let counter = now_secs() / STEP_SECONDS;
    let candidates = [
        code_for_counter(&secret, counter.saturating_sub(1)),
        code_for_counter(&secret, counter),
        code_for_counter(&secret, counter.saturating_add(1)),
    ];

    let mut ok = 0u8;
    for cand in &candidates {
        let mut diff = 0u8;
        let cb = cand.as_bytes();
        let ub = code.as_bytes();
        for i in 0..cb.len() {
            diff |= cb[i] ^ ub[i];
        }
        // Branchless OR: ok stays 1 if any candidate matched (diff == 0).
        ok |= (diff == 0) as u8;
    }
    ok != 0
}

/// Compute the TOTP code valid right now for a base32-encoded secret. Used
/// in tests so we can drive the verifier without an external authenticator.
#[cfg(test)]
pub fn current_code(secret_b32: &str) -> Option<String> {
    let secret = base32_decode(secret_b32)?;
    Some(code_for_counter(&secret, now_secs() / STEP_SECONDS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_code_passes_verification() {
        // Google Authenticator demo secret.
        let secret = "JBSWY3DPEHPK3PXP";
        let code = current_code(secret).unwrap();
        assert!(verify(secret, &code));
    }

    #[test]
    fn wrong_code_fails() {
        let secret = "JBSWY3DPEHPK3PXP";
        assert!(!verify(secret, "000000"));
        assert!(!verify(secret, "abcdef"));
        assert!(!verify(secret, "12345"));
        assert!(!verify(secret, "1234567"));
    }

    #[test]
    fn malformed_secret_fails_closed() {
        // Lowercase plus non-base32 chars must not accidentally accept any code.
        assert!(!verify("not!base32", "123456"));
    }

    #[test]
    fn rfc6238_test_vector_sha1() {
        // RFC 6238 Appendix B test vector for SHA-1: at T=59s the TOTP for
        // ASCII secret "12345678901234567890" is 94287082.
        // Base32-encode that secret first.
        let secret_ascii = b"12345678901234567890";
        let secret_b32 = super::totp_test_helpers::base32_encode(secret_ascii);
        let counter = 59u64 / STEP_SECONDS;
        let secret_bytes = base32_decode(&secret_b32).unwrap();
        let code = code_for_counter(&secret_bytes, counter);
        // Last 6 digits of 94287082 = 287082.
        assert_eq!(code, "287082");
    }
}

#[cfg(test)]
mod totp_test_helpers {
    pub fn base32_encode(data: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut out = String::new();
        let mut buf: u64 = 0;
        let mut bits: u32 = 0;
        for &b in data {
            buf = (buf << 8) | b as u64;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out.push(ALPHABET[((buf >> bits) & 0x1F) as usize] as char);
            }
        }
        if bits > 0 {
            out.push(ALPHABET[((buf << (5 - bits)) & 0x1F) as usize] as char);
        }
        out
    }
}
