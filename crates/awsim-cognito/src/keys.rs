//! Process-wide RSA signing key for Cognito JWTs.
//!
//! AWS Cognito signs ID and access tokens with a private RSA key per pool and
//! exposes the matching public key at the pool's JWKS endpoint. awsim mirrors
//! that contract with a single 2048-bit keypair, so SDK clients that fully
//! verify signatures via JWKS see a consistent view.
//!
//! By default the key is per-process (generated lazily on first use and
//! regenerated on restart). When awsim runs with `--data-dir`, call
//! [`init_persistent`] at startup to load the key from disk (or create and
//! store it on first run) so tokens minted before a restart still verify
//! afterwards. The private key lives in a single file under the data dir with
//! owner-only permissions.

use std::path::Path;
use std::sync::OnceLock;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Validation};
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey};
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{Value, json};
use tracing::{info, warn};

/// JWKS `kid` advertised for the (single) signing key.
pub const KID: &str = "awsim-key-1";

/// File name of the persisted signing key under the data dir.
const KEY_FILE: &str = "cognito-signing-key.der";

struct SigningMaterial {
    encoding: EncodingKey,
    decoding: DecodingKey,
    n_b64url: String,
    e_b64url: String,
}

static MATERIAL: OnceLock<SigningMaterial> = OnceLock::new();

/// Derive the cached signing material (encoding/decoding keys + JWKS
/// components) from an RSA private key.
fn material_from_key(private: &RsaPrivateKey) -> SigningMaterial {
    let public = RsaPublicKey::from(private);

    let der = private.to_pkcs1_der().expect("RSA key encodes to PKCS#1");
    let encoding = EncodingKey::from_rsa_der(der.as_bytes());

    let n_b64url = URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
    let e_b64url = URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
    let decoding = DecodingKey::from_rsa_components(&n_b64url, &e_b64url)
        .expect("base64url-encoded RSA components are well-formed");

    SigningMaterial {
        encoding,
        decoding,
        n_b64url,
        e_b64url,
    }
}

/// Generate a fresh 2048-bit signing key (the ephemeral, in-memory default).
fn generate_ephemeral() -> SigningMaterial {
    let mut rng = rand::thread_rng();
    let private = RsaPrivateKey::new(&mut rng, 2048)
        .expect("RSA key generation should succeed with a working RNG");
    material_from_key(&private)
}

/// Load the signing key from `{dir}/cognito-signing-key.der`, or generate and
/// persist a new one when the file is missing or unreadable.
fn load_or_create(dir: &Path) -> SigningMaterial {
    let path = dir.join(KEY_FILE);
    if let Ok(bytes) = std::fs::read(&path) {
        match RsaPrivateKey::from_pkcs1_der(&bytes) {
            Ok(private) => {
                info!(path = %path.display(), "Cognito: loaded persisted signing key");
                return material_from_key(&private);
            }
            Err(e) => warn!(
                path = %path.display(),
                error = %e,
                "Cognito: persisted signing key is unreadable; regenerating"
            ),
        }
    }

    let mut rng = rand::thread_rng();
    let private = RsaPrivateKey::new(&mut rng, 2048)
        .expect("RSA key generation should succeed with a working RNG");
    match private.to_pkcs1_der() {
        Ok(der) => {
            if let Err(e) = write_key_file(&path, der.as_bytes()) {
                warn!(path = %path.display(), error = %e, "Cognito: could not persist signing key");
            } else {
                info!(path = %path.display(), "Cognito: generated and persisted signing key");
            }
        }
        Err(e) => warn!(error = %e, "Cognito: could not encode signing key for persistence"),
    }
    material_from_key(&private)
}

/// Write the key bytes with owner-only permissions where supported.
fn write_key_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Load (or create) a persistent signing key under `dir`. Call once at startup
/// before any token is signed; later calls are ignored. Without this the key
/// is generated lazily in memory and not persisted.
pub fn init_persistent(dir: &Path) {
    if MATERIAL.set(load_or_create(dir)).is_err() {
        warn!("Cognito: signing key already initialized; persistence call ignored");
    }
}

fn material() -> &'static SigningMaterial {
    MATERIAL.get_or_init(generate_ephemeral)
}

/// Return the RS256 encoding key (private) for signing JWTs.
pub fn encoding_key() -> &'static EncodingKey {
    &material().encoding
}

/// Return the RS256 decoding key (public) for verifying JWTs.
pub fn decoding_key() -> &'static DecodingKey {
    &material().decoding
}

/// Build the JWKS document advertised at the well-known endpoint.
pub fn jwks_document() -> Value {
    let m = material();
    json!({
        "keys": [{
            "kty": "RSA",
            "alg": "RS256",
            "use": "sig",
            "kid": KID,
            "n": m.n_b64url,
            "e": m.e_b64url,
        }]
    })
}

/// Build a Validation that requires RS256 + non-expired.
///
/// `iss` and `token_use` are intentionally not asserted here: callers may
/// operate behind custom hostnames (the OAuth router rewrites the issuer),
/// and `token_use` differentiation is the caller's concern. We do require
/// `iss` to be present so completely unsigned tokens cannot leak through.
pub fn validation() -> Validation {
    let mut v = Validation::new(Algorithm::RS256);
    v.set_required_spec_claims(&["exp", "iss"]);
    v.validate_aud = false;
    v.leeway = 30;
    v
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn load_or_create_persists_and_reuses_the_key() {
        let dir = std::env::temp_dir().join(format!("awsim-key-persist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // First call generates and writes the key file.
        let first = load_or_create(&dir);
        assert!(dir.join(KEY_FILE).exists(), "key file written");

        // Second call on the same dir reuses the persisted key.
        let second = load_or_create(&dir);
        assert_eq!(
            first.n_b64url, second.n_b64url,
            "modulus matches: key was reloaded, not regenerated"
        );

        // A different dir yields a different key.
        let other = dir.join("other");
        std::fs::create_dir_all(&other).unwrap();
        let third = load_or_create(&other);
        assert_ne!(first.n_b64url, third.n_b64url);

        std::fs::remove_dir_all(&dir).ok();
    }
}
