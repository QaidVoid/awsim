//! Process-wide RSA signing key for Cognito JWTs.
//!
//! AWS Cognito signs ID and access tokens with a private RSA key per pool and
//! exposes the matching public key at the pool's JWKS endpoint. awsim mirrors
//! that contract with a single 2048-bit keypair generated lazily on first use,
//! so SDK clients that fully verify signatures via JWKS see a consistent view.
//!
//! The key is intentionally per-process (regenerated on restart): for an
//! offline emulator there is no value in stable signing material, and the
//! alternative (persisting a private key to disk) creates an obvious footgun
//! when the workspace is shared.

use std::sync::OnceLock;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Validation};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{Value, json};

/// JWKS `kid` advertised for the (single) signing key.
pub const KID: &str = "awsim-key-1";

struct SigningMaterial {
    encoding: EncodingKey,
    decoding: DecodingKey,
    n_b64url: String,
    e_b64url: String,
}

static MATERIAL: OnceLock<SigningMaterial> = OnceLock::new();

fn material() -> &'static SigningMaterial {
    MATERIAL.get_or_init(|| {
        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048)
            .expect("RSA key generation should succeed with a working RNG");
        let public = RsaPublicKey::from(&private);

        let der = private
            .to_pkcs1_der()
            .expect("freshly generated RSA key encodes to PKCS#1");
        let encoding = EncodingKey::from_rsa_der(der.as_bytes());

        let n_bytes = public.n().to_bytes_be();
        let e_bytes = public.e().to_bytes_be();
        let n_b64url = URL_SAFE_NO_PAD.encode(&n_bytes);
        let e_b64url = URL_SAFE_NO_PAD.encode(&e_bytes);
        let decoding = DecodingKey::from_rsa_components(&n_b64url, &e_b64url)
            .expect("base64url-encoded RSA components are well-formed");

        SigningMaterial {
            encoding,
            decoding,
            n_b64url,
            e_b64url,
        }
    })
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
