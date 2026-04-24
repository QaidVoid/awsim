use crate::chk;
use crate::runner::common::*;

pub async fn test_polly(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_polly::Client::new(&config);
    let mut results = Vec::new();

    results.push(chk!(
        "DescribeVoices",
        client.describe_voices().send().await,
        verbose
    ));
    results.push(chk!(
        "ListLexicons",
        client.list_lexicons().send().await,
        verbose
    ));
    results.push(chk!(
        "ListSpeechSynthesisTasks",
        client.list_speech_synthesis_tasks().send().await,
        verbose
    ));

    results
}
