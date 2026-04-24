use crate::chk;
use crate::runner::common::*;

pub async fn test_sts(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_sts::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "GetCallerIdentity",
        client.get_caller_identity().send().await,
        verbose
    ));

    results.push(chk!(
        "AssumeRole",
        client
            .assume_role()
            .role_arn("arn:aws:iam::000000000000:role/ConformanceRole")
            .role_session_name("conformance-session")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetSessionToken",
        client.get_session_token().send().await,
        verbose
    ));

    results.push(chk!(
        "GetFederationToken",
        client
            .get_federation_token()
            .name("conformance-fed-user")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "DecodeAuthorizationMessage",
        client
            .decode_authorization_message()
            .encoded_message("FAKE-ENCODED-AUTH-MESSAGE")
            .send()
            .await,
        verbose
    ));

    results.push(chk!(
        "GetAccessKeyInfo",
        client
            .get_access_key_info()
            .access_key_id("ASIAEXAMPLEACCESSKEY")
            .send()
            .await,
        verbose
    ));

    results
}
