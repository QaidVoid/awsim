//! Pins each AWSim per-service error factory against the AWS Smithy
//! spec. The test loads each `models/<service>.json` model and asserts
//! the factory's emitted `code` matches the on-wire code
//! (`awsQueryError.code` for Query services, the shape name for JSON
//! services) and the HTTP status matches `smithy.api#httpError`.
//!
//! Add new entries here whenever a service grows a new error factory.
//! Regressions in either field will fail the test loudly, so SDK
//! retry logic stays correct.

use std::path::PathBuf;

use awsim_conformance::smithy_errors::{self, SmithyError};
use awsim_core::AwsError;

fn models_dir() -> PathBuf {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .ancestors()
        .nth(2)
        .expect("workspace root above crates/awsim-conformance")
        .join("models")
}

fn load(service: &str) -> Vec<SmithyError> {
    smithy_errors::load_errors(&models_dir().join(format!("{service}.json")))
}

fn expect(errors: &[SmithyError], shape: &str) -> SmithyError {
    smithy_errors::find(errors, shape)
        .unwrap_or_else(|| panic!("Smithy model missing error shape `{shape}`"))
        .clone()
}

fn assert_matches(err: AwsError, smithy: &SmithyError, label: &str) {
    assert_eq!(
        err.code, smithy.wire_code,
        "[{label}] code mismatch (smithy shape {})",
        smithy.shape_name
    );
    assert_eq!(
        err.status.as_u16(),
        smithy.http_status,
        "[{label}] HTTP status mismatch (smithy shape {})",
        smithy.shape_name
    );
}

#[test]
fn iam_error_factories_match_smithy() {
    let errors = load("iam");

    assert_matches(
        awsim_iam::error::no_such_entity("User", "alice"),
        &expect(&errors, "NoSuchEntityException"),
        "no_such_entity",
    );
    assert_matches(
        awsim_iam::error::entity_already_exists("User", "alice"),
        &expect(&errors, "EntityAlreadyExistsException"),
        "entity_already_exists",
    );
    assert_matches(
        awsim_iam::error::delete_conflict("group not empty"),
        &expect(&errors, "DeleteConflictException"),
        "delete_conflict",
    );
    assert_matches(
        awsim_iam::error::malformed_policy_document("not JSON"),
        &expect(&errors, "MalformedPolicyDocumentException"),
        "malformed_policy_document",
    );
    assert_matches(
        awsim_iam::error::limit_exceeded("cannot attach more than 10 policies"),
        &expect(&errors, "LimitExceededException"),
        "limit_exceeded",
    );
}

#[test]
fn kms_error_factories_match_smithy() {
    let errors = load("kms");

    assert_matches(
        awsim_kms::error::not_found("Key"),
        &expect(&errors, "NotFoundException"),
        "not_found",
    );
    assert_matches(
        awsim_kms::error::alias_exists("alias/foo"),
        &expect(&errors, "AlreadyExistsException"),
        "alias_exists",
    );
    assert_matches(
        awsim_kms::error::key_disabled("abc"),
        &expect(&errors, "DisabledException"),
        "key_disabled",
    );
    assert_matches(
        awsim_kms::error::kms_invalid_state("pending deletion"),
        &expect(&errors, "KMSInvalidStateException"),
        "kms_invalid_state",
    );
    assert_matches(
        awsim_kms::error::invalid_key_usage("usage mismatch"),
        &expect(&errors, "InvalidKeyUsageException"),
        "invalid_key_usage",
    );
    // KMS does not model a generic `InvalidParameterException` or
    // `MissingParameter` shape; AWS returns `SerializationException` from
    // the protocol layer for malformed/missing JSON. The awsim helpers
    // `invalid_parameter` and `missing_parameter` are tracked for a
    // follow-up sweep and intentionally have no Smithy peer here.
}

#[test]
fn lambda_error_factories_match_smithy() {
    let errors = load("lambda");

    assert_matches(
        awsim_lambda::error::resource_not_found("Function", "f"),
        &expect(&errors, "ResourceNotFoundException"),
        "resource_not_found",
    );
    assert_matches(
        awsim_lambda::error::resource_conflict("update in progress"),
        &expect(&errors, "ResourceConflictException"),
        "resource_conflict",
    );
    assert_matches(
        awsim_lambda::error::invalid_parameter("Runtime is invalid"),
        &expect(&errors, "InvalidParameterValueException"),
        "invalid_parameter",
    );
}
