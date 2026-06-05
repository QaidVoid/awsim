//! Library surface of the conformance crate.
//!
//! Exposes the Smithy model parser and error-shape extractor, plus the
//! in-process AWSim `server` and the SDK `runner`, so integration tests
//! (under `tests/`) can both validate AWSim's error catalog against the
//! AWS-spec models in `models/` and drive the real AWS SDKs against a
//! live server to assert behavior, not just response-envelope shape.

pub mod runner;
pub mod server;
pub mod smithy;
pub mod smithy_errors;
