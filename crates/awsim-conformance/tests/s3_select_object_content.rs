//! SelectObjectContent behavior contract.
//!
//! Drives a SQL query over a CSV object through the S3 SDK, collects the
//! event-stream Records payloads, and asserts the projected and filtered
//! result, confirming both the query engine and the event-stream framing.

use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{
    CsvInput, CsvOutput, ExpressionType, FileHeaderInfo, InputSerialization, OutputSerialization,
    SelectObjectContentEventStream,
};

#[tokio::test]
async fn select_filters_and_projects_csv() {
    let endpoint = awsim_conformance::server::start().await;
    let config = awsim_conformance::runner::common::make_config(&endpoint).await;
    let client = aws_sdk_s3::Client::new(&config);

    client
        .create_bucket()
        .bucket("select-bucket")
        .send()
        .await
        .expect("create bucket");
    client
        .put_object()
        .bucket("select-bucket")
        .key("people.csv")
        .body(ByteStream::from_static(
            b"name,age,city\nAlice,30,NYC\nBob,25,LA\nCarol,40,NYC\n",
        ))
        .send()
        .await
        .expect("put object");

    let mut output = client
        .select_object_content()
        .bucket("select-bucket")
        .key("people.csv")
        .expression("SELECT name, city FROM S3Object s WHERE s.age > 28")
        .expression_type(ExpressionType::Sql)
        .input_serialization(
            InputSerialization::builder()
                .csv(
                    CsvInput::builder()
                        .file_header_info(FileHeaderInfo::Use)
                        .build(),
                )
                .build(),
        )
        .output_serialization(
            OutputSerialization::builder()
                .csv(CsvOutput::builder().build())
                .build(),
        )
        .send()
        .await
        .expect("select");

    let mut records = Vec::new();
    while let Some(event) = output.payload.recv().await.expect("recv event") {
        if let SelectObjectContentEventStream::Records(r) = event
            && let Some(payload) = r.payload()
        {
            records.extend_from_slice(payload.as_ref());
        }
    }

    let text = String::from_utf8(records).expect("utf8");
    assert_eq!(text, "Alice,NYC\nCarol,NYC\n");
}
