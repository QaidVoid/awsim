//! Cognito-flavoured SRP6a server-side primitives.
//!
//! Real Cognito uses SRP-6a with the 3072-bit group from RFC 5054, SHA-256
//! as the hash, and HKDF-SHA256 to derive the session key from the shared
//! secret S. The key on-the-wire identities are:
//!
//! ```text
//!   N, g          // group prime + generator (constants)
//!   k = H(N || PAD(g))
//!   x = H(salt || H(pool_short_name || ":" || username || ":" || password))
//!   v = g^x mod N                       // verifier (stored at user creation)
//!   b = random in [1, N-2]              // server private
//!   B = (k*v + g^b) mod N               // server public
//!   A                                   // client public, sent on InitiateAuth
//!   u = H(PAD(A) || PAD(B))
//!   S = (A * v^u)^b mod N               // shared secret
//!   K = HKDF-SHA256(salt=u_bytes, ikm=S, info="Caldera Derived Key", L=16)
//!   expected_M1 = HMAC-SHA256(K, pool_short_name || username || secret_block || timestamp)
//! ```
//!
//! awsim emulates only the *server* side: it stores `(salt, v)` per user
//! (computed from the plaintext password at user creation), generates fresh
//! `(b, B)` per challenge, and verifies the client's `M1` against the
//! expected value. The `secret_block` is opaque to the client; it round-trips
//! it back unchanged in the challenge response.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use num_traits::Num;
use rand::RngCore as _;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// 3072-bit safe prime from RFC 5054 / RFC 3526. Cognito uses this group.
const N_HEX: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74",
    "020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437",
    "4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF05",
    "98DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB",
    "9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B",
    "E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF695581718",
    "3995497CEA956AE515D2261898FA051015728E5A8AAAC42DAD33170D04507A33",
    "A85521ABDF1CBA64ECFB850458DBEF0A8AEA71575D060C7DB3970F85A6E1E4C7",
    "ABF5AE8CDB0933D71E8C94E04A25619DCEE3D2261AD2EE6BF12FFA06D98A0864",
    "D87602733EC86A64521F2B18177B200CBBE117577A615D6C770988C0BAD946E2",
    "08E24FA074E5AB3143DB5BFCE0FD108E4B82D120A93AD2CAFFFFFFFFFFFFFFFF",
);
const G: u32 = 2;

/// Total key-byte length (3072 bits = 384 bytes). Used for left-padding.
const KEY_BYTES: usize = 384;

/// Length of `K` derived via HKDF. Cognito uses 16 bytes.
const K_BYTES: usize = 16;

/// HKDF info string Cognito uses when deriving K from the shared secret S.
const HKDF_INFO: &[u8] = b"Caldera Derived Key";

fn n() -> BigUint {
    BigUint::from_str_radix(N_HEX, 16).expect("N is well-formed hex")
}

fn g() -> BigUint {
    BigUint::from(G)
}

/// Left-pad `bytes` with leading zeros until it is exactly KEY_BYTES long.
/// SRP relies on a fixed-width representation when hashing values that
/// originated as BigUints, since otherwise the digest depends on the
/// modulus's leading-zero count.
fn pad(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() >= KEY_BYTES {
        return bytes.to_vec();
    }
    let mut out = vec![0u8; KEY_BYTES - bytes.len()];
    out.extend_from_slice(bytes);
    out
}

fn pad_biguint(v: &BigUint) -> Vec<u8> {
    pad(&v.to_bytes_be())
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// k = H(N || PAD(g)). Constant for our group; computed each call to keep
/// the module dependency-free, since Sha256 is fast.
fn k_value() -> BigUint {
    let n_bytes = n().to_bytes_be();
    let g_bytes = pad_biguint(&g());
    BigUint::from_bytes_be(&sha256(&[&n_bytes, &g_bytes]))
}

/// `x = H(salt || H(pool_short_name || username || ":" || password))`.
///
/// Matches Cognito's amplify-js implementation. Note that the inner hash
/// runs over the literal UTF-8 bytes; salt is interpreted as raw bytes
/// (the wire form is hex but we accept either via `salt`).
pub fn derive_x(pool_short_name: &str, username: &str, password: &str, salt: &[u8]) -> BigUint {
    let inner = sha256(&[
        pool_short_name.as_bytes(),
        username.as_bytes(),
        b":",
        password.as_bytes(),
    ]);
    BigUint::from_bytes_be(&sha256(&[salt, &inner]))
}

/// v = g^x mod N. The server stores v at user creation; it never sees x
/// again during authentication.
pub fn verifier(x: &BigUint) -> BigUint {
    g().modpow(x, &n())
}

/// Generate a fresh server keypair `(b, B)` for one challenge.
///
/// SRP requires `B != 0 mod N`. With 256 bits of entropy that condition
/// fails with negligible probability, but if it ever does we draw fresh
/// entropy and retry rather than handing back a degenerate key.
pub fn server_keys(verifier: &BigUint) -> (BigUint, BigUint) {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    let nn = n();
    let zero = BigUint::from(0u32);
    loop {
        rng.fill_bytes(&mut bytes);
        let b_priv = BigUint::from_bytes_be(&bytes);
        let kv = (k_value() * verifier) % &nn;
        let gb = g().modpow(&b_priv, &nn);
        let b_pub = (kv + gb) % &nn;
        if b_pub != zero {
            return (b_priv, b_pub);
        }
    }
}

/// Compute the server-side session key K from the client's public A.
///
/// Returns None if A mod N is zero, which SRP forbids.
pub fn derive_k(
    a_pub: &BigUint,
    b_pub: &BigUint,
    b_priv: &BigUint,
    verifier: &BigUint,
) -> Option<[u8; K_BYTES]> {
    let nn = n();
    let a_mod = a_pub % &nn;
    if a_mod == BigUint::from(0u32) {
        return None;
    }
    let u = BigUint::from_bytes_be(&sha256(&[&pad_biguint(a_pub), &pad_biguint(b_pub)]));
    if u == BigUint::from(0u32) {
        return None;
    }
    let av_u = (a_pub * verifier.modpow(&u, &nn)) % &nn;
    let s = av_u.modpow(b_priv, &nn);
    // HKDF: salt = u as bytes (Cognito uses u-bytes as the HKDF salt),
    // ikm = S bytes, info = "Caldera Derived Key", out len = 16.
    let salt = u.to_bytes_be();
    let ikm = pad_biguint(&s);
    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut out = [0u8; K_BYTES];
    hk.expand(HKDF_INFO, &mut out).ok()?;
    Some(out)
}

/// Compute the expected client proof M1.
///
/// `M1 = HMAC-SHA256(K, pool_short_name || username || secret_block || timestamp)`.
pub fn expected_m1(
    k_session: &[u8; K_BYTES],
    pool_short_name: &str,
    username: &str,
    secret_block: &[u8],
    timestamp: &str,
) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(k_session).expect("HMAC accepts any key length");
    mac.update(pool_short_name.as_bytes());
    mac.update(username.as_bytes());
    mac.update(secret_block);
    mac.update(timestamp.as_bytes());
    let out = mac.finalize().into_bytes();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// Constant-time slice equality.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Generate a 16-byte salt encoded as lowercase hex, matching Cognito's
/// wire format on the InitiateAuth challenge.
pub fn random_salt_hex() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Generate a 64-byte opaque secret block, base64-encoded, that the client
/// echoes back unchanged on PASSWORD_VERIFIER.
#[allow(dead_code)]
pub fn random_secret_block_b64() -> String {
    let mut bytes = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut bytes);
    BASE64.encode(bytes)
}

/// Decode a hex-encoded big-endian integer to BigUint. Returns None on bad
/// hex digits.
#[allow(dead_code)]
pub fn biguint_from_hex(s: &str) -> Option<BigUint> {
    BigUint::from_str_radix(s.trim(), 16).ok()
}

/// Encode a BigUint as lowercase hex, with no padding (Cognito's wire form
/// is variable-width hex).
#[allow(dead_code)]
pub fn biguint_to_hex(v: &BigUint) -> String {
    v.to_str_radix(16)
}

/// Decode salt from the wire (hex) into raw bytes.
pub fn decode_salt_hex(hex: &str) -> Option<Vec<u8>> {
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_then_v_then_b_round_trip() {
        // Walk through one full handshake against ourselves: derive x and v
        // from a password, generate (b, B), then on the "client" side reuse
        // the same x to compute A = g^a, then check that both ends arrive at
        // the same K.
        let pool_short = "abcdef";
        let username = "alice";
        let password = "Hunter2!";
        let salt_bytes = b"\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10";

        let x = derive_x(pool_short, username, password, salt_bytes);
        let v = verifier(&x);

        let (b_priv, b_pub) = server_keys(&v);

        // Client-side: pick random a, compute A = g^a mod N.
        let mut a_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut a_bytes);
        let a_priv = BigUint::from_bytes_be(&a_bytes);
        let a_pub = g().modpow(&a_priv, &n());

        let server_k = derive_k(&a_pub, &b_pub, &b_priv, &v).expect("server K derives for valid A");

        // Client computes K too: u = H(PAD(A) || PAD(B)), S = (B - k*v)^(a + u*x) mod N.
        let u = BigUint::from_bytes_be(&sha256(&[&pad_biguint(&a_pub), &pad_biguint(&b_pub)]));
        let kv = (k_value() * &v) % &n();
        // (B - kv) mod N, with manual underflow handling because BigUint
        // subtraction panics on negatives.
        let b_sub_kv = if b_pub >= kv {
            (&b_pub - &kv) % &n()
        } else {
            (&b_pub + &n() - kv) % &n()
        };
        let exp = (&a_priv + &u * &x) % (&n() - BigUint::from(1u32));
        let s_client = b_sub_kv.modpow(&exp, &n());
        let salt = u.to_bytes_be();
        let ikm = pad_biguint(&s_client);
        let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
        let mut client_k = [0u8; K_BYTES];
        hk.expand(HKDF_INFO, &mut client_k).unwrap();

        assert_eq!(server_k, client_k, "client and server should derive same K");
    }

    #[test]
    fn m1_matches_when_inputs_match() {
        let k = [7u8; K_BYTES];
        let pool = "abcdef";
        let user = "alice";
        let secret = b"opaque-server-blob";
        let ts = "Sun Apr 21 12:00:00 UTC 2024";
        let a = expected_m1(&k, pool, user, secret, ts);
        let b = expected_m1(&k, pool, user, secret, ts);
        assert!(ct_eq(&a, &b));
    }

    #[test]
    fn m1_differs_when_password_differs() {
        // Different K -> different M1.
        let k1 = [1u8; K_BYTES];
        let k2 = [2u8; K_BYTES];
        let pool = "abcdef";
        let user = "alice";
        let secret = b"sb";
        let ts = "Sun Apr 21 12:00:00 UTC 2024";
        assert!(!ct_eq(
            &expected_m1(&k1, pool, user, secret, ts),
            &expected_m1(&k2, pool, user, secret, ts)
        ));
    }

    #[test]
    fn salt_hex_decodes_round_trip() {
        let hex = random_salt_hex();
        let bytes = decode_salt_hex(&hex).unwrap();
        assert_eq!(bytes.len(), 16);
        assert_eq!(hex.len(), 32);
    }
}
