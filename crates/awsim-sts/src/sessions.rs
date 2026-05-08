//! Tracking of STS-issued temp credentials.
//!
//! `AssumeRole` and the various role-assumption variants — together with
//! Cognito Identity's `GetCredentialsForIdentity` — return short-lived
//! credentials (`ASIA...`) that callers immediately use to sign further
//! requests. Without a way to map those access keys back to the role
//! that issued them, the IAM enforcement layer can't resolve a
//! principal for the caller, and every authenticated request is denied
//! before policy evaluation runs.
//!
//! [`StsSessionStore`] holds that mapping. STS handlers record into it
//! when they mint credentials; the principal-lookup chain consults it
//! ahead of the IAM user/role lookup; expired sessions are swept on
//! lookup so the map doesn't grow unbounded for long-running servers.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use dashmap::DashMap;

/// One issued assumed-role session: enough information to materialise
/// the assumed-role principal ARN and to find the role's identity
/// policies via an IAM lookup.
#[derive(Clone, Debug)]
pub struct AssumedRoleSession {
    /// `ASIA...` access key returned to the caller. Used as the lookup
    /// key on subsequent signed requests.
    pub access_key: String,
    /// Role ARN the session is bound to (used to resolve identity
    /// policies via the IAM principal lookup).
    pub role_arn: String,
    /// Role short name parsed off the role ARN. Cached so the hot
    /// authz path doesn't re-parse on every request.
    pub role_name: String,
    /// AWS-style session name. For `AssumeRole` this is the
    /// `RoleSessionName` parameter; for Cognito Identity vending it's
    /// the `IdentityId`.
    pub session_name: String,
    /// Account that owns the role.
    pub account_id: String,
    /// `AROA...:session_name` — the AWS `UserId` shape that
    /// `GetCallerIdentity` reports for assumed-role callers.
    pub assumed_role_id: String,
    /// Wall-clock expiration. Real AWS sessions cap at 12 hours; we
    /// honour whatever the caller passed in. `None` means "no expiry"
    /// — only used by tests; production paths always set one.
    pub expiry: Option<SystemTime>,
}

impl AssumedRoleSession {
    pub fn is_expired(&self, now: SystemTime) -> bool {
        match self.expiry {
            Some(t) => now >= t,
            None => false,
        }
    }

    /// Convenience: convert a `duration_seconds` (the field every
    /// AssumeRole-shaped API takes) into an absolute `SystemTime` from
    /// now.
    pub fn expiry_from_duration(seconds: u64) -> Option<SystemTime> {
        SystemTime::now().checked_add(Duration::from_secs(seconds))
    }
}

#[derive(Default)]
pub struct StsSessionStore {
    sessions: DashMap<String, AssumedRoleSession>,
}

impl StsSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a freshly issued session. Replaces any prior entry under
    /// the same access key (collisions are vanishingly unlikely with
    /// 16 hex chars of entropy, but the replacement keeps the map's
    /// invariants simple).
    pub fn record(&self, session: AssumedRoleSession) {
        self.sessions.insert(session.access_key.clone(), session);
    }

    /// Look up a session by access key. Returns `None` if absent or if
    /// the entry has expired — expired entries are removed in-place so
    /// the next caller doesn't have to re-check.
    pub fn lookup(&self, access_key: &str) -> Option<AssumedRoleSession> {
        let now = SystemTime::now();
        let session = self.sessions.get(access_key)?.clone();
        if session.is_expired(now) {
            self.sessions.remove(access_key);
            return None;
        }
        Some(session)
    }

    /// Drop every expired entry. Cheap to call periodically from a
    /// background tick; not on the hot path.
    pub fn purge_expired(&self) {
        let now = SystemTime::now();
        self.sessions.retain(|_, s| !s.is_expired(now));
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// Helper for callers that hold an `Arc<StsSessionStore>` already and
/// just want a fresh handle without importing `Arc` themselves.
pub fn shared() -> Arc<StsSessionStore> {
    Arc::new(StsSessionStore::new())
}

/// Parse the role short name off an IAM role ARN
/// (`arn:aws:iam::ACCT:role/Name` -> `Name`). Falls back to the full
/// ARN when the input doesn't have the expected shape, so a malformed
/// caller doesn't silently produce an empty session-name.
pub fn role_name_from_arn(role_arn: &str) -> String {
    role_arn.rsplit('/').next().unwrap_or(role_arn).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(key: &str, expiry: Option<SystemTime>) -> AssumedRoleSession {
        AssumedRoleSession {
            access_key: key.to_string(),
            role_arn: "arn:aws:iam::000000000000:role/Test".to_string(),
            role_name: "Test".to_string(),
            session_name: "s1".to_string(),
            account_id: "000000000000".to_string(),
            assumed_role_id: "AROAFAKE:s1".to_string(),
            expiry,
        }
    }

    #[test]
    fn record_and_lookup_roundtrips() {
        let store = StsSessionStore::new();
        let s = session("ASIA1", None);
        store.record(s.clone());
        let got = store.lookup("ASIA1").expect("present");
        assert_eq!(got.role_arn, s.role_arn);
    }

    #[test]
    fn lookup_drops_expired_entries() {
        let store = StsSessionStore::new();
        let past = SystemTime::now() - Duration::from_secs(10);
        store.record(session("ASIA-EXP", Some(past)));
        assert!(store.lookup("ASIA-EXP").is_none());
        // Side-effect: lookup also evicts.
        assert!(store.is_empty());
    }

    #[test]
    fn purge_expired_clears_only_expired() {
        let store = StsSessionStore::new();
        let past = SystemTime::now() - Duration::from_secs(10);
        let future = SystemTime::now() + Duration::from_secs(60);
        store.record(session("OLD", Some(past)));
        store.record(session("NEW", Some(future)));
        store.purge_expired();
        assert!(store.lookup("OLD").is_none());
        assert!(store.lookup("NEW").is_some());
    }

    #[test]
    fn role_name_parses_off_arn() {
        assert_eq!(
            role_name_from_arn("arn:aws:iam::000000000000:role/MyRole"),
            "MyRole"
        );
        assert_eq!(role_name_from_arn("not-an-arn"), "not-an-arn");
    }
}
