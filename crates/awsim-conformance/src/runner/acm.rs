use crate::chk;
use crate::runner::common::*;

pub async fn test_acm(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_acm::Client::new(&config);
    let mut results = Vec::new();

    // RequestCertificate
    let request_r = client
        .request_certificate()
        .domain_name("conformance.example.com")
        .send()
        .await;
    let cert_arn = request_r
        .as_ref()
        .ok()
        .and_then(|r| r.certificate_arn.clone());
    results.push(chk!("RequestCertificate", request_r, verbose));

    // ListCertificates
    results.push(chk!(
        "ListCertificates",
        client.list_certificates().send().await,
        verbose
    ));

    if let Some(ref arn) = cert_arn {
        // DescribeCertificate
        results.push(chk!(
            "DescribeCertificate",
            client.describe_certificate().certificate_arn(arn).send().await,
            verbose
        ));

        // GetCertificate
        results.push(chk!(
            "GetCertificate",
            client.get_certificate().certificate_arn(arn).send().await,
            verbose
        ));

        // AddTagsToCertificate
        results.push(chk!(
            "AddTagsToCertificate",
            client
                .add_tags_to_certificate()
                .certificate_arn(arn)
                .tags(
                    aws_sdk_acm::types::Tag::builder()
                        .key("env")
                        .value("conformance")
                        .build()
                        .unwrap(),
                )
                .send()
                .await,
            verbose
        ));

        // ListTagsForCertificate
        results.push(chk!(
            "ListTagsForCertificate",
            client
                .list_tags_for_certificate()
                .certificate_arn(arn)
                .send()
                .await,
            verbose
        ));

        // RenewCertificate
        results.push(chk!(
            "RenewCertificate",
            client.renew_certificate().certificate_arn(arn).send().await,
            verbose
        ));

        // DeleteCertificate
        results.push(chk!(
            "DeleteCertificate",
            client.delete_certificate().certificate_arn(arn).send().await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeCertificate".to_string()));
        results.push(OpResult::Skipped("GetCertificate".to_string()));
        results.push(OpResult::Skipped("AddTagsToCertificate".to_string()));
        results.push(OpResult::Skipped("ListTagsForCertificate".to_string()));
        results.push(OpResult::Skipped("RenewCertificate".to_string()));
        results.push(OpResult::Skipped("DeleteCertificate".to_string()));
    }

    results
}
