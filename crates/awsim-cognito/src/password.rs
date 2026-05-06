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
        .map_err(|e| AwsError::internal(format!("password hashing failed: {e}")))
}

/// Verify a plaintext password against a stored bcrypt hash.
///
/// Returns `false` for any verification failure, including malformed hashes
/// (which can happen if state was hand-edited or restored from a legacy
/// snapshot containing plaintext).
pub fn verify(plain: &str, hashed: &str) -> bool {
    bcrypt::verify(plain, hashed).unwrap_or(false)
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
