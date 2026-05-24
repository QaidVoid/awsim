//! Generic state-machine helper for resources that need an
//! observable transient lifecycle.
//!
//! Most AWS resources have a state field that progresses through
//! `CREATING -> ACTIVE -> UPDATING -> ACTIVE -> DELETING -> gone`
//! (with `*_FAILED` branches for unhappy paths). Real AWS exposes
//! these intermediate states to clients that poll `Describe*`;
//! emulators that flip straight to `ACTIVE` mask race conditions in
//! caller code. This module provides a small parking-lot-backed
//! struct each resource holds, plus a `Tickable`-style `observe`
//! that promotes the resource when the scheduled deadline elapses.
//!
//! ## Fast mode for tests
//!
//! Setting `AWSIM_LIFECYCLE_FAST=1` collapses every transition to a
//! zero-duration delay. Use this in CI or local-dev so test suites
//! aren't paying real seconds per resource.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// A state a [`LifecycleSm`] can occupy.
///
/// Implementations are typically a simple `enum` whose variants
/// match the AWS state vocabulary for the service. The trait
/// methods describe the state graph (which states are terminal,
/// which transitions are valid) so the helper can promote on tick
/// without each service open-coding it.
pub trait LifecycleState:
    Copy + Clone + PartialEq + Eq + fmt::Debug + Send + Sync + 'static
{
    /// True for states that should be promoted on the next tick
    /// when their deadline has elapsed (i.e. transient states like
    /// `CREATING`/`UPDATING`/`DELETING`). Terminal states return
    /// false and stay put.
    fn is_transient(&self) -> bool;
}

/// Read-only view of a [`LifecycleSm`] taken via [`LifecycleSm::observe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleView<S> {
    pub state: S,
    /// Optional human-readable reason set on a failure transition.
    pub reason: Option<&'static str>,
}

/// Generic lifecycle state machine for an AWS resource.
///
/// The struct is internally synchronized via a `Mutex` and is
/// serializable through helpers so it round-trips through
/// snapshot/restore. Drive transitions with [`Self::start_transition`]
/// and let the tick loop call [`Self::observe`] to promote when
/// the deadline arrives.
#[derive(Debug)]
pub struct LifecycleSm<S: LifecycleState> {
    inner: Mutex<Inner<S>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inner<S> {
    current: S,
    next: Option<S>,
    transition_at: Option<SystemTime>,
    reason: Option<&'static str>,
}

impl<S: LifecycleState> LifecycleSm<S> {
    /// Build a new state machine initialised to `initial`.
    pub fn new(initial: S) -> Self {
        Self {
            inner: Mutex::new(Inner {
                current: initial,
                next: None,
                transition_at: None,
                reason: None,
            }),
        }
    }

    /// Current observed state without advancing the clock.
    pub fn current(&self) -> S {
        self.inner.lock().unwrap().current
    }

    /// Snapshot the state, optionally promoting if a pending
    /// transition's deadline has passed by `now`.
    ///
    /// Call this from a service's `Describe*` path and from the
    /// tick handler. Idempotent.
    pub fn observe(&self, now: SystemTime) -> LifecycleView<S> {
        let mut g = self.inner.lock().unwrap();
        if let (Some(next), Some(at)) = (g.next, g.transition_at)
            && now >= at
        {
            g.current = next;
            g.next = None;
            g.transition_at = None;
        }
        LifecycleView {
            state: g.current,
            reason: g.reason,
        }
    }

    /// Begin a transition from `from` to `to` over `delay`. Fails
    /// silently if the current state has already changed (e.g. a
    /// racing concurrent update) so the helper stays idempotent.
    ///
    /// Pass `Duration::ZERO` for synchronous transitions, or use
    /// the [`fast_mode`] helper which collapses every delay when
    /// `AWSIM_LIFECYCLE_FAST=1`.
    pub fn start_transition(&self, from: S, to: S, delay: Duration) {
        let effective = if fast_mode() { Duration::ZERO } else { delay };
        let mut g = self.inner.lock().unwrap();
        if g.current != from {
            return;
        }
        if effective.is_zero() {
            g.current = to;
            g.next = None;
            g.transition_at = None;
        } else {
            g.next = Some(to);
            g.transition_at = Some(SystemTime::now() + effective);
        }
        g.reason = None;
    }

    /// Mark the resource as failed, attaching a static reason.
    pub fn fail(&self, failed_state: S, reason: &'static str) {
        let mut g = self.inner.lock().unwrap();
        g.current = failed_state;
        g.next = None;
        g.transition_at = None;
        g.reason = Some(reason);
    }

    /// True if `current()` is a transient state expected to
    /// promote on its own (the tick driver should keep observing).
    pub fn is_transient(&self) -> bool {
        self.inner.lock().unwrap().current.is_transient()
    }

    /// Reject a modifying call when the resource is mid-transition.
    /// Returns the AWS-standard `ResourceInUseException` so callers
    /// can `?` it from a handler.
    pub fn reject_if_busy(&self, ok_states: &[S]) -> Result<(), crate::error::AwsError> {
        let g = self.inner.lock().unwrap();
        if ok_states.contains(&g.current) {
            return Ok(());
        }
        Err(crate::error::AwsError::bad_request(
            "ResourceInUseException",
            format!(
                "Resource is in state {:?}; cannot proceed until it reaches one of {:?}.",
                g.current, ok_states
            ),
        ))
    }
}

impl<S: LifecycleState + Serialize + for<'de> Deserialize<'de>> LifecycleSm<S> {
    /// Snapshot the state machine to a serializable form.
    pub fn to_snapshot(&self) -> LifecycleSnapshot<S> {
        let g = self.inner.lock().unwrap();
        LifecycleSnapshot {
            current: g.current,
            next: g.next,
            transition_at: g.transition_at,
            reason: g.reason,
        }
    }

    /// Restore from a snapshot. The next observe will promote if
    /// the saved deadline has already passed.
    pub fn from_snapshot(snap: LifecycleSnapshot<S>) -> Self {
        Self {
            inner: Mutex::new(Inner {
                current: snap.current,
                next: snap.next,
                transition_at: snap.transition_at,
                reason: snap.reason,
            }),
        }
    }
}

/// Serializable snapshot for persistence round-trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleSnapshot<S> {
    pub current: S,
    pub next: Option<S>,
    pub transition_at: Option<SystemTime>,
    pub reason: Option<&'static str>,
}

static FAST_MODE: OnceLock<bool> = OnceLock::new();

/// Returns true when the AWSIM_LIFECYCLE_FAST env var is set to a
/// truthy value at first call.
///
/// The value is cached so flipping it mid-process has no effect;
/// tests that need to toggle it should set it before instantiating
/// any `LifecycleSm`.
pub fn fast_mode() -> bool {
    *FAST_MODE.get_or_init(|| {
        std::env::var("AWSIM_LIFECYCLE_FAST")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    enum TestState {
        Creating,
        Active,
        Updating,
        Deleting,
        Failed,
    }

    impl LifecycleState for TestState {
        fn is_transient(&self) -> bool {
            matches!(
                self,
                TestState::Creating | TestState::Updating | TestState::Deleting
            )
        }
    }

    #[test]
    fn observe_promotes_after_deadline() {
        let sm = LifecycleSm::new(TestState::Creating);
        sm.start_transition(
            TestState::Creating,
            TestState::Active,
            Duration::from_millis(10),
        );
        // Before deadline: still creating.
        assert_eq!(sm.observe(SystemTime::now()).state, TestState::Creating);
        // After deadline: promotes.
        let later = SystemTime::now() + Duration::from_secs(1);
        assert_eq!(sm.observe(later).state, TestState::Active);
        // Repeat observe: no change.
        assert_eq!(sm.observe(later).state, TestState::Active);
    }

    #[test]
    fn start_transition_ignores_wrong_from_state() {
        let sm = LifecycleSm::new(TestState::Active);
        sm.start_transition(TestState::Creating, TestState::Updating, Duration::ZERO);
        assert_eq!(sm.current(), TestState::Active);
    }

    #[test]
    fn zero_delay_promotes_synchronously() {
        let sm = LifecycleSm::new(TestState::Creating);
        sm.start_transition(TestState::Creating, TestState::Active, Duration::ZERO);
        assert_eq!(sm.current(), TestState::Active);
    }

    #[test]
    fn reject_if_busy_returns_error_when_not_in_allowed_set() {
        let sm = LifecycleSm::new(TestState::Updating);
        let err = sm.reject_if_busy(&[TestState::Active]).unwrap_err();
        assert_eq!(err.code, "ResourceInUseException");

        let sm2 = LifecycleSm::new(TestState::Active);
        sm2.reject_if_busy(&[TestState::Active]).unwrap();
    }

    #[test]
    fn fail_records_reason_and_terminates() {
        let sm = LifecycleSm::new(TestState::Creating);
        sm.fail(TestState::Failed, "boot disk corrupt");
        let view = sm.observe(SystemTime::now());
        assert_eq!(view.state, TestState::Failed);
        assert_eq!(view.reason, Some("boot disk corrupt"));
    }

    #[test]
    fn snapshot_round_trip_preserves_pending_transition() {
        let sm = LifecycleSm::new(TestState::Creating);
        sm.start_transition(
            TestState::Creating,
            TestState::Active,
            Duration::from_secs(60),
        );
        let snap = sm.to_snapshot();
        let restored: LifecycleSm<TestState> = LifecycleSm::from_snapshot(snap);
        assert_eq!(restored.current(), TestState::Creating);
        // Force the deadline.
        let later = SystemTime::now() + Duration::from_secs(120);
        assert_eq!(restored.observe(later).state, TestState::Active);
    }

    #[test]
    fn is_transient_reflects_current_state() {
        let sm = LifecycleSm::new(TestState::Creating);
        assert!(sm.is_transient());
        sm.start_transition(TestState::Creating, TestState::Active, Duration::ZERO);
        assert!(!sm.is_transient());
    }
}
