use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use crate::state::{Lexicon, PollyState};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn put_lexicon(
    state: &PollyState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidLexiconNameException", "Name is required"))?
        .to_string();
    let content = input["Content"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidLexiconException", "Content is required"))?
        .to_string();

    let lex = Lexicon {
        name: name.clone(),
        content,
        last_modified: now_secs(),
    };
    state.lexicons.insert(name, lex);

    Ok(json!({}))
}

pub fn get_lexicon(
    state: &PollyState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidLexiconNameException", "Name is required"))?;

    let lex = state.lexicons.get(name).ok_or_else(|| {
        AwsError::not_found(
            "LexiconNotFoundException",
            format!("Lexicon not found: {name}"),
        )
    })?;

    Ok(json!({
        "Lexicon": {
            "Content": lex.content,
            "Name": lex.name,
        },
        "LexiconAttributes": {
            "Alphabet": "ipa",
            "LanguageCode": "en-US",
            "LastModified": lex.last_modified,
            "LexiconArn": arn::build(ctx, "polly", format!("lexicon/{}", lex.name)),
            "LexemesCount": 0,
            "Size": lex.content.len(),
        }
    }))
}

pub fn list_lexicons(
    state: &PollyState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .lexicons
        .iter()
        .map(|e| {
            let l = e.value();
            json!({
                "Name": l.name,
                "Attributes": {
                    "Alphabet": "ipa",
                    "LanguageCode": "en-US",
                    "LastModified": l.last_modified,
                    "LexiconArn": arn::build(ctx, "polly", format!("lexicon/{}", l.name)),
                    "LexemesCount": 0,
                    "Size": l.content.len(),
                }
            })
        })
        .collect();

    Ok(json!({ "Lexicons": list }))
}

pub fn delete_lexicon(
    state: &PollyState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidLexiconNameException", "Name is required"))?;
    state.lexicons.remove(name);
    Ok(json!({}))
}
