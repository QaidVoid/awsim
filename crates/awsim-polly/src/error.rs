//! Polly error helpers. LexiconNotFoundException is HTTP 404 per the
//! Smithy override; every other polly error (including
//! SynthesisTaskNotFoundException) defaults to 400.

use awsim_core::AwsError;

pub fn lexicon_not_found(name: &str) -> AwsError {
    AwsError::not_found(
        "LexiconNotFoundException",
        format!("Lexicon {name} not found"),
    )
}

pub fn synthesis_task_not_found(id: &str) -> AwsError {
    AwsError::bad_request(
        "SynthesisTaskNotFoundException",
        format!("Synthesis task {id} not found"),
    )
}
