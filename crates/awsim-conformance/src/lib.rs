//! Library surface of the conformance crate.
//!
//! Exposes the Smithy model parser and the error-shape extractor so
//! integration tests (under `tests/`) can validate AWSim's error
//! catalog against the AWS-spec models in `models/`.

pub mod smithy;
pub mod smithy_errors;
