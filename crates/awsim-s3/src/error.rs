//! S3 error helpers. S3 uses REST-XML; the wire `<Code>` field carries
//! the shape name verbatim and HTTP status mapping is shape-by-shape
//! per the `s3.json` model.

use awsim_core::AwsError;

pub fn no_such_bucket(bucket: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchBucket",
        format!("The specified bucket does not exist: {bucket}"),
    )
}

pub fn no_such_key(key: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchKey",
        format!("The specified key does not exist: {key}"),
    )
}

pub fn no_such_upload(upload_id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchUpload",
        format!("The specified upload '{upload_id}' does not exist"),
    )
}

pub fn bucket_already_owned_by_you(bucket: &str) -> AwsError {
    AwsError::conflict(
        "BucketAlreadyOwnedByYou",
        format!("The bucket '{bucket}' already exists and is owned by you"),
    )
}

pub fn bucket_already_exists(bucket: &str) -> AwsError {
    AwsError::conflict(
        "BucketAlreadyExists",
        format!("The requested bucket name {bucket} is not available"),
    )
}
