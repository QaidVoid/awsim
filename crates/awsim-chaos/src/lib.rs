//! Chaos injection for AWSim — synthetic errors + latency on inbound
//! requests so application code can be exercised against realistic
//! AWS failure modes without leaving the offline emulator.
//!
//! The crate exposes a [`ChaosEngine`] that the gateway calls before
//! dispatching a request. Each call evaluates the configured rules
//! and returns either `None` (pass through) or a [`ChaosOutcome`]
//! describing how to mangle the request — sleep, error, or both.
//!
//! Engine state is persisted alongside service snapshots when
//! `--data-dir` is set, so chaos rules survive restarts.

pub mod engine;
pub mod presets;
pub mod rule;

pub use engine::{ChaosEngine, ChaosOutcome, RecentInjection};
pub use presets::{PRESETS, PresetInfo};
pub use rule::{
    ChaosEffect, ChaosRule, ChaosSchedule, ErrorEffect, Flap, LatencyEffect, OperationMatch,
    ServiceMatch, TimeWindow,
};
