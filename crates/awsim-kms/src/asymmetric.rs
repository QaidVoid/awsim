//! Real asymmetric key material for RSA and ECC KMS keys.
//!
//! Private keys are stored as PKCS#8 DER. Public keys are returned as
//! DER SubjectPublicKeyInfo and ECDSA signatures as DER `ECDSA-Sig-Value`,
//! matching the byte formats real KMS uses.

use awsim_core::AwsError;
use rand::rngs::OsRng;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use rsa::{Pkcs1v15Sign, Pss, RsaPrivateKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha384, Sha512};

const RSA_SIGNING: &[&str] = &[
    "RSASSA_PKCS1_V1_5_SHA_256",
    "RSASSA_PKCS1_V1_5_SHA_384",
    "RSASSA_PKCS1_V1_5_SHA_512",
    "RSASSA_PSS_SHA_256",
    "RSASSA_PSS_SHA_384",
    "RSASSA_PSS_SHA_512",
];

/// Signing algorithms a key spec supports, as KMS lists them.
pub fn signing_algorithms(key_spec: &str) -> &'static [&'static str] {
    match key_spec {
        s if s.starts_with("RSA_") => RSA_SIGNING,
        "ECC_NIST_P256" | "ECC_SECG_P256K1" => &["ECDSA_SHA_256"],
        "ECC_NIST_P384" => &["ECDSA_SHA_384"],
        "ECC_NIST_P521" => &["ECDSA_SHA_512"],
        "SM2" => &["SM2DSA"],
        _ => &[],
    }
}

fn encryption_algorithms(key_spec: &str) -> &'static [&'static str] {
    match key_spec {
        "SYMMETRIC_DEFAULT" => &["SYMMETRIC_DEFAULT"],
        s if s.starts_with("RSA_") => &["RSAES_OAEP_SHA_1", "RSAES_OAEP_SHA_256"],
        "SM2" => &["SM2PKE"],
        _ => &[],
    }
}

fn mac_algorithms(key_spec: &str) -> &'static [&'static str] {
    match key_spec {
        "HMAC_224" => &["HMAC_SHA_224"],
        "HMAC_256" => &["HMAC_SHA_256"],
        "HMAC_384" => &["HMAC_SHA_384"],
        "HMAC_512" => &["HMAC_SHA_512"],
        _ => &[],
    }
}

/// Add the algorithm list that applies to the key's usage
/// (`EncryptionAlgorithms`, `SigningAlgorithms`, `KeyAgreementAlgorithms`
/// or `MacAlgorithms`) to a KeyMetadata or GetPublicKey response.
pub fn insert_algorithms(out: &mut Value, key_spec: &str, key_usage: &str) {
    let (field, algorithms): (&str, &[&str]) = match key_usage {
        "ENCRYPT_DECRYPT" => ("EncryptionAlgorithms", encryption_algorithms(key_spec)),
        "SIGN_VERIFY" => ("SigningAlgorithms", signing_algorithms(key_spec)),
        "KEY_AGREEMENT" => ("KeyAgreementAlgorithms", &["ECDH"]),
        "GENERATE_VERIFY_MAC" => ("MacAlgorithms", mac_algorithms(key_spec)),
        _ => return,
    };
    out[field] = json!(algorithms);
}

/// Generate PKCS#8 DER key material for an asymmetric key spec.
/// Returns `None` for specs awsim keeps no asymmetric material for
/// (symmetric, HMAC and SM2 keys).
pub fn generate(key_spec: &str) -> Result<Option<Vec<u8>>, AwsError> {
    let der = match key_spec {
        "RSA_2048" => rsa_generate(2048)?,
        "RSA_3072" => rsa_generate(3072)?,
        "RSA_4096" => rsa_generate(4096)?,
        "ECC_NIST_P256" => nist_p256::generate()?,
        "ECC_NIST_P384" => nist_p384::generate()?,
        "ECC_NIST_P521" => nist_p521::generate()?,
        "ECC_SECG_P256K1" => secg_p256k1::generate()?,
        _ => return Ok(None),
    };
    Ok(Some(der))
}

/// DER SubjectPublicKeyInfo for the key's public half.
pub fn public_key(key_spec: &str, private_key: &[u8]) -> Result<Vec<u8>, AwsError> {
    match key_spec {
        s if s.starts_with("RSA_") => rsa_private(private_key)?
            .to_public_key()
            .to_public_key_der()
            .map(|d| d.as_bytes().to_vec())
            .map_err(internal),
        "ECC_NIST_P256" => nist_p256::public_key(private_key),
        "ECC_NIST_P384" => nist_p384::public_key(private_key),
        "ECC_NIST_P521" => nist_p521::public_key(private_key),
        "ECC_SECG_P256K1" => secg_p256k1::public_key(private_key),
        _ => Err(unsupported(key_spec)),
    }
}

/// Sign `digest` (already hashed with the algorithm's hash function).
pub fn sign(
    key_spec: &str,
    private_key: &[u8],
    algorithm: &str,
    digest: &[u8],
) -> Result<Vec<u8>, AwsError> {
    match key_spec {
        s if s.starts_with("RSA_") => {
            let key = rsa_private(private_key)?;
            let result = if algorithm.starts_with("RSASSA_PSS_") {
                key.sign_with_rng(&mut OsRng, pss(algorithm), digest)
            } else {
                key.sign(pkcs1v15(algorithm), digest)
            };
            result.map_err(internal)
        }
        "ECC_NIST_P256" => nist_p256::sign(private_key, digest),
        "ECC_NIST_P384" => nist_p384::sign(private_key, digest),
        "ECC_NIST_P521" => nist_p521::sign(private_key, digest),
        "ECC_SECG_P256K1" => secg_p256k1::sign(private_key, digest),
        _ => Err(unsupported(key_spec)),
    }
}

/// Check `signature` over `digest` (already hashed with the algorithm's
/// hash function).
pub fn verify(
    key_spec: &str,
    private_key: &[u8],
    algorithm: &str,
    digest: &[u8],
    signature: &[u8],
) -> Result<bool, AwsError> {
    match key_spec {
        s if s.starts_with("RSA_") => {
            let key = rsa_private(private_key)?.to_public_key();
            let result = if algorithm.starts_with("RSASSA_PSS_") {
                key.verify(pss(algorithm), digest, signature)
            } else {
                key.verify(pkcs1v15(algorithm), digest, signature)
            };
            Ok(result.is_ok())
        }
        "ECC_NIST_P256" => nist_p256::verify(private_key, digest, signature),
        "ECC_NIST_P384" => nist_p384::verify(private_key, digest, signature),
        "ECC_NIST_P521" => nist_p521::verify(private_key, digest, signature),
        "ECC_SECG_P256K1" => secg_p256k1::verify(private_key, digest, signature),
        _ => Err(unsupported(key_spec)),
    }
}

/// Hash `message` for `algorithm`, or check that it is already a digest
/// of the right length when `MessageType` is `DIGEST`.
pub fn digest(algorithm: &str, message: &[u8], message_type: &str) -> Result<Vec<u8>, AwsError> {
    let len = match hash_bits(algorithm) {
        256 => 32,
        384 => 48,
        _ => 64,
    };
    match message_type {
        "RAW" => Ok(match len {
            32 => Sha256::digest(message).to_vec(),
            48 => Sha384::digest(message).to_vec(),
            _ => Sha512::digest(message).to_vec(),
        }),
        "DIGEST" if message.len() == len => Ok(message.to_vec()),
        "DIGEST" => Err(AwsError::validation(format!(
            "Digest is invalid length for algorithm {algorithm}."
        ))),
        other => Err(AwsError::validation(format!(
            "MessageType {other} is not supported for SigningAlgorithm {algorithm}."
        ))),
    }
}

fn hash_bits(algorithm: &str) -> u32 {
    if algorithm.ends_with("_384") {
        384
    } else if algorithm.ends_with("_512") {
        512
    } else {
        256
    }
}

fn pss(algorithm: &str) -> Pss {
    match hash_bits(algorithm) {
        384 => Pss::new::<Sha384>(),
        512 => Pss::new::<Sha512>(),
        _ => Pss::new::<Sha256>(),
    }
}

fn pkcs1v15(algorithm: &str) -> Pkcs1v15Sign {
    match hash_bits(algorithm) {
        384 => Pkcs1v15Sign::new::<Sha384>(),
        512 => Pkcs1v15Sign::new::<Sha512>(),
        _ => Pkcs1v15Sign::new::<Sha256>(),
    }
}

fn rsa_generate(bits: usize) -> Result<Vec<u8>, AwsError> {
    RsaPrivateKey::new(&mut OsRng, bits)
        .map_err(internal)?
        .to_pkcs8_der()
        .map(|d| d.as_bytes().to_vec())
        .map_err(internal)
}

fn rsa_private(der: &[u8]) -> Result<RsaPrivateKey, AwsError> {
    RsaPrivateKey::from_pkcs8_der(der).map_err(internal)
}

fn internal(err: impl std::fmt::Display) -> AwsError {
    AwsError::internal(format!("KMS key material error: {err}"))
}

fn unsupported(key_spec: &str) -> AwsError {
    AwsError::bad_request(
        "UnsupportedOperationException",
        format!("awsim does not implement key material for KeySpec {key_spec}"),
    )
}

macro_rules! ecdsa_curve {
    ($module:ident, $krate:ident) => {
        mod $module {
            use awsim_core::AwsError;
            use rand::rngs::OsRng;
            use $krate::SecretKey;
            use $krate::ecdsa::signature::hazmat::{PrehashVerifier, RandomizedPrehashSigner};
            use $krate::ecdsa::{Signature, SigningKey, VerifyingKey};
            use $krate::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};

            use super::internal;

            pub fn generate() -> Result<Vec<u8>, AwsError> {
                SecretKey::random(&mut OsRng)
                    .to_pkcs8_der()
                    .map(|d| d.as_bytes().to_vec())
                    .map_err(internal)
            }

            fn secret(der: &[u8]) -> Result<SecretKey, AwsError> {
                SecretKey::from_pkcs8_der(der).map_err(internal)
            }

            fn signing_key(der: &[u8]) -> Result<SigningKey, AwsError> {
                SigningKey::from_bytes(&secret(der)?.to_bytes()).map_err(internal)
            }

            pub fn public_key(der: &[u8]) -> Result<Vec<u8>, AwsError> {
                secret(der)?
                    .public_key()
                    .to_public_key_der()
                    .map(|d| d.as_bytes().to_vec())
                    .map_err(internal)
            }

            pub fn sign(der: &[u8], digest: &[u8]) -> Result<Vec<u8>, AwsError> {
                let signature: Signature = signing_key(der)?
                    .sign_prehash_with_rng(&mut OsRng, digest)
                    .map_err(internal)?;
                Ok(signature.to_der().as_bytes().to_vec())
            }

            pub fn verify(der: &[u8], digest: &[u8], signature: &[u8]) -> Result<bool, AwsError> {
                let key = VerifyingKey::from(&signing_key(der)?);
                let Ok(signature) = Signature::from_der(signature) else {
                    return Ok(false);
                };
                let signature = signature.normalize_s().unwrap_or(signature);
                Ok(key.verify_prehash(digest, &signature).is_ok())
            }
        }
    };
}

ecdsa_curve!(nist_p256, p256);
ecdsa_curve!(nist_p384, p384);
ecdsa_curve!(nist_p521, p521);
ecdsa_curve!(secg_p256k1, k256);
