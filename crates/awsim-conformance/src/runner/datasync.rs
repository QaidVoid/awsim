use crate::chk;
use crate::runner::common::*;

pub async fn test_datasync(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_datasync::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "CreateLocationS3",
        client
            .create_location_s3()
            .s3_bucket_arn("arn:aws:s3:::conf-bucket")
            .s3_config(
                aws_sdk_datasync::types::S3Config::builder()
                    .bucket_access_role_arn("arn:aws:iam::000000000000:role/S3Role")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));
    results.push(chk!(
        "ListLocations",
        client.list_locations().send().await,
        verbose
    ));
    results.push(chk!("ListTasks", client.list_tasks().send().await, verbose));

    results
}
