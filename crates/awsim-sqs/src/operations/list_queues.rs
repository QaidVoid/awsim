use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SqsState;

pub fn handle(state: &SqsState, input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    let prefix = input["QueueNamePrefix"].as_str().unwrap_or("");

    let mut urls: Vec<String> = state
        .queues
        .iter()
        .filter(|entry| entry.key().starts_with(prefix))
        .map(|entry| entry.value().url.clone())
        .collect();

    urls.sort();

    Ok(json!({ "QueueUrls": urls }))
}
