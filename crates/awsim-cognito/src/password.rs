//! Password hashing and verification for Cognito users.
//!
//! Real AWS Cognito never stores user passwords in clear: even AWS internal
//! staff cannot recover the original. awsim mirrors that contract by hashing
//! every password with bcrypt before it lands in [`CognitoUser::password_hash`]
//! and only ever comparing through [`verify`].
//!
//! The cost factor is intentionally modest (the moral equivalent of "high
//! enough that a snapshot leak is not trivially crackable, low enough that
//! tests don't pay seconds of hashing"). It is *not* a substitute for real
//! Cognito's KDF, but it does eliminate the previous class of bug where a
//! plaintext snapshot leaked credentials.
//!
//! [`CognitoUser::password_hash`]: crate::state::CognitoUser::password_hash

use awsim_core::AwsError;

/// bcrypt cost factor. Default is 12; we use 6 which still produces a real
/// salted bcrypt hash but keeps the unit-test loop snappy. The threat model
/// for an offline emulator is "do not persist plaintext", not "withstand a
/// dedicated GPU farm".
const COST: u32 = 6;

/// Hash a plaintext password with bcrypt.
pub fn hash(plain: &str) -> Result<String, AwsError> {
    bcrypt::hash(plain, COST)
        .map_err(|e| crate::error::internal_error(format!("password hashing failed: {e}")))
}

/// Verify a plaintext password against a stored bcrypt hash.
///
/// Returns `false` for any verification failure, including malformed hashes
/// (which can happen if state was hand-edited or restored from a legacy
/// snapshot containing plaintext).
pub fn verify(plain: &str, hashed: &str) -> bool {
    bcrypt::verify(plain, hashed).unwrap_or(false)
}

/// Pool ID short name: the part of `us-east-1_abcDEF` after the `_`.
/// Used by the Cognito SRP variant when computing the inner password hash.
pub fn pool_short_name(pool_id: &str) -> &str {
    pool_id.split_once('_').map(|(_, s)| s).unwrap_or(pool_id)
}

/// Compute and return `(salt_hex, verifier_hex)` for `password` so the
/// USER_SRP_AUTH flow can verify a future client without ever storing the
/// plaintext. Both sides depend on `pool_short_name(pool_id)` so the
/// material is bound to the pool.
pub fn srp_material(pool_id: &str, username: &str, password: &str) -> (String, String) {
    let short = pool_short_name(pool_id);
    let salt_hex = crate::srp::random_salt_hex();
    let salt_bytes =
        crate::srp::decode_salt_hex(&salt_hex).expect("random_salt_hex always emits valid hex");
    let x = crate::srp::derive_x(short, username, password, &salt_bytes);
    let v = crate::srp::verifier(&x);
    let v_hex = v.to_str_radix(16);
    (salt_hex, v_hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_succeeds() {
        let h = hash("Hunter2!").unwrap();
        assert!(verify("Hunter2!", &h));
    }

    #[test]
    fn wrong_password_fails() {
        let h = hash("Hunter2!").unwrap();
        assert!(!verify("hunter2!", &h));
        assert!(!verify("", &h));
    }

    #[test]
    fn legacy_plaintext_does_not_pass() {
        // Old snapshots stored the plain password directly. After the
        // hashing migration, comparing a plaintext field as a "hash" must
        // still fail closed.
        assert!(!verify("Hunter2!", "Hunter2!"));
    }

    #[test]
    fn salt_means_two_hashes_of_same_password_differ() {
        let a = hash("same").unwrap();
        let b = hash("same").unwrap();
        assert_ne!(a, b);
        assert!(verify("same", &a));
        assert!(verify("same", &b));
    }
}
