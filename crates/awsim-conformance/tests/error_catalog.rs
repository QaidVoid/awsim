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

/// Variant for `awsQueryCompatible` JSON services (SQS): the factory's
/// code is expected to match `awsQueryError.code` rather than the
/// shape name.
fn assert_query_compat_matches(err: AwsError, smithy: &SmithyError, label: &str) {
    let expected_code = smithy
        .query_error_code
        .as_deref()
        .unwrap_or(&smithy.wire_code);
    assert_eq!(
        err.code, expected_code,
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
fn secretsmanager_error_factories_match_smithy() {
    let errors = load("secretsmanager");

    assert_matches(
        awsim_secretsmanager::error::resource_not_found("MySecret"),
        &expect(&errors, "ResourceNotFoundException"),
        "resource_not_found",
    );
    assert_matches(
        awsim_secretsmanager::error::resource_exists("MySecret"),
        &expect(&errors, "ResourceExistsException"),
        "resource_exists",
    );
    assert_matches(
        awsim_secretsmanager::error::invalid_parameter("bad value"),
        &expect(&errors, "InvalidParameterException"),
        "invalid_parameter",
    );
    assert_matches(
        awsim_secretsmanager::error::invalid_request("conflicting state"),
        &expect(&errors, "InvalidRequestException"),
        "invalid_request",
    );
}

#[test]
fn kinesis_error_factories_match_smithy() {
    let errors = load("kinesis");

    assert_matches(
        awsim_kinesis::error::resource_not_found("Stream not found"),
        &expect(&errors, "ResourceNotFoundException"),
        "resource_not_found",
    );
    assert_matches(
        awsim_kinesis::error::resource_in_use("Stream already exists"),
        &expect(&errors, "ResourceInUseException"),
        "resource_in_use",
    );
    assert_matches(
        awsim_kinesis::error::limit_exceeded("too many streams"),
        &expect(&errors, "LimitExceededException"),
        "limit_exceeded",
    );
    assert_matches(
        awsim_kinesis::error::provisioned_throughput_exceeded("throttled"),
        &expect(&errors, "ProvisionedThroughputExceededException"),
        "provisioned_throughput_exceeded",
    );
    assert_matches(
        awsim_kinesis::error::expired_iterator("iterator expired"),
        &expect(&errors, "ExpiredIteratorException"),
        "expired_iterator",
    );
    assert_matches(
        awsim_kinesis::error::invalid_argument("bad arg"),
        &expect(&errors, "InvalidArgumentException"),
        "invalid_argument",
    );
}

#[test]
fn s3_error_factories_match_smithy() {
    let errors = load("s3");

    assert_matches(
        awsim_s3::error::no_such_bucket("missing"),
        &expect(&errors, "NoSuchBucket"),
        "no_such_bucket",
    );
    assert_matches(
        awsim_s3::error::no_such_key("missing.txt"),
        &expect(&errors, "NoSuchKey"),
        "no_such_key",
    );
    assert_matches(
        awsim_s3::error::no_such_upload("upload-1"),
        &expect(&errors, "NoSuchUpload"),
        "no_such_upload",
    );
    assert_matches(
        awsim_s3::error::bucket_already_exists("b"),
        &expect(&errors, "BucketAlreadyExists"),
        "bucket_already_exists",
    );
    assert_matches(
        awsim_s3::error::bucket_already_owned_by_you("b"),
        &expect(&errors, "BucketAlreadyOwnedByYou"),
        "bucket_already_owned_by_you",
    );
}

#[test]
fn sns_error_factories_match_smithy() {
    let errors = load("sns");

    assert_matches(
        awsim_sns::error::invalid_parameter("ResourceArn is required"),
        &expect(&errors, "InvalidParameterException"),
        "invalid_parameter",
    );
    assert_matches(
        awsim_sns::error::not_found("Topic not found: arn:aws:sns:..."),
        &expect(&errors, "NotFoundException"),
        "not_found",
    );
}

#[test]
fn sqs_error_factories_match_smithy() {
    // SQS is awsJson1_0 + awsQueryCompatible. SDKs read the Query-style
    // code from the `x-amzn-query-error` header, but awsim currently
    // emits the Query-style code as the JSON `__type` too for legacy
    // SDK compatibility. Pin against awsQueryError.code rather than the
    // shape name until the protocol layer learns to split header/body.
    let errors = load("sqs");
    assert_query_compat_matches(
        awsim_sqs::error::nonexistent_queue("my-queue"),
        &expect(&errors, "QueueDoesNotExist"),
        "nonexistent_queue",
    );
    assert_query_compat_matches(
        awsim_sqs::error::receipt_handle_invalid("bad handle"),
        &expect(&errors, "ReceiptHandleIsInvalid"),
        "receipt_handle_invalid",
    );
    assert_query_compat_matches(
        awsim_sqs::error::resource_not_found("not here"),
        &expect(&errors, "ResourceNotFoundException"),
        "resource_not_found",
    );
    assert_query_compat_matches(
        awsim_sqs::error::queue_name_exists("dup"),
        &expect(&errors, "QueueNameExists"),
        "queue_name_exists",
    );
    assert_query_compat_matches(
        awsim_sqs::error::batch_request_too_long("over 256kb"),
        &expect(&errors, "BatchRequestTooLong"),
        "batch_request_too_long",
    );
}

#[test]
fn dynamodb_error_factories_match_smithy() {
    let errors = load("dynamodb");

    assert_matches(
        awsim_dynamodb::error::resource_in_use("Table is being created"),
        &expect(&errors, "ResourceInUseException"),
        "resource_in_use",
    );
    assert_matches(
        awsim_dynamodb::error::table_already_exists("Users"),
        &expect(&errors, "TableAlreadyExistsException"),
        "table_already_exists",
    );
    assert_matches(
        awsim_dynamodb::error::global_table_already_exists("Users"),
        &expect(&errors, "GlobalTableAlreadyExistsException"),
        "global_table_already_exists",
    );
    assert_matches(
        awsim_dynamodb::error::resource_not_found("Table not found"),
        &expect(&errors, "ResourceNotFoundException"),
        "resource_not_found",
    );
    assert_matches(
        awsim_dynamodb::error::provisioned_throughput_exceeded("throttled"),
        &expect(&errors, "ProvisionedThroughputExceededException"),
        "provisioned_throughput_exceeded",
    );
    assert_matches(
        awsim_dynamodb::error::conditional_check_failed("condition failed"),
        &expect(&errors, "ConditionalCheckFailedException"),
        "conditional_check_failed",
    );
    assert_matches(
        awsim_dynamodb::error::transaction_canceled("cancelled"),
        &expect(&errors, "TransactionCanceledException"),
        "transaction_canceled",
    );
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
