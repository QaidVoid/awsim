use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;

/// AWS returns at most 1000 queue URLs per page and defaults to the same.
const DEFAULT_MAX_RESULTS: usize = 1000;
const MAX_MAX_RESULTS: usize = 1000;

pub fn handle(state: &SqsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let prefix = input["QueueNamePrefix"].as_str().unwrap_or("");

    let mut urls: Vec<String> = state
        .queues
        .iter()
        .filter(|entry| entry.key().starts_with(prefix))
        .map(|entry| entry.value().url.clone())
        .collect();

    urls.sort();

    // `MaxResults` and `NextToken` were accepted and ignored, so a
    // caller asking for one page got every queue back and no token to
    // continue with.
    let limit = cap_max_results(
        input["MaxResults"].as_i64(),
        DEFAULT_MAX_RESULTS,
        MAX_MAX_RESULTS,
    );
    let page = paginate(urls, limit, input["NextToken"].as_str(), Clone::clone)?;

    let mut resp = json!({ "QueueUrls": page.items });
    if let Some(token) = page.next_token {
        resp["NextToken"] = json!(token);
    }
    Ok(resp)
}
