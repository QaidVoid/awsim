//! CloudTrail error helpers. Most cloudtrail errors are JSON
//! `*Exception` codes with explicit `httpError` traits in the model.

use awsim_core::AwsError;

pub fn trail_not_found(name: &str) -> AwsError {
    AwsError::not_found("TrailNotFoundException", format!("Trail {name} not found"))
}
