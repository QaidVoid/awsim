use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, KmsKeyLookup, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{SnsState, Topic};

// ---------------------------------------------------------------------------
// CreateTopic
// ---------------------------------------------------------------------------

pub fn create_topic(
    state: &SnsState,
    input: &Value,
    ctx: &RequestContext,
    kms_lookup: Option<&dyn KmsKeyLookup>,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    validate_topic_name(name)?;

    let is_fifo = name.ends_with(".fifo");

    // If FifoTopic attribute present, validate consistency
    if let Some(attrs) = input["Attributes"].as_object()
        && let Some(fifo_val) = attrs.get("FifoTopic")
    {
        let attr_fifo = fifo_val.as_str() == Some("true");
        if attr_fifo != is_fifo {
            return Err(AwsError::bad_request(
                "InvalidParameter",
                "FifoTopic attribute must be consistent with .fifo suffix",
            ));
        }
    }

    let arn = format!("arn:aws:sns:{}:{}:{}", ctx.region, ctx.account_id, name);

    // Return existing if already present
    if state.topics.contains_key(&arn) {
        info!(topic = %arn, "Topic already exists, returning existing ARN");
        return Ok(json!({ "TopicArn": arn }));
    }

    let created_at = now_epoch_str();

    let mut attributes: HashMap<String, String> = HashMap::new();
    if let Some(attrs) = input["Attributes"].as_object() {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                attributes.insert(k.clone(), s.to_string());
            }
        }
    }

    if let Some(key_ref) = attributes.get("KmsMasterKeyId")
        && !key_ref.is_empty()
    {
        validate_kms_master_key_id(key_ref, ctx, kms_lookup)?;
    }

    let mut tags: HashMap<String, String> = HashMap::new();
    if let Some(tag_list) = input["Tags"].as_array() {
        for tag in tag_list {
            if let (Some(k), Some(v)) = (tag["Key"].as_str(), tag["Value"].as_str()) {
                tags.insert(k.to_string(), v.to_string());
            }
        }
    }

    let topic = Topic::new(
        arn.clone(),
        name.to_string(),
        is_fifo,
        created_at,
        attributes,
        tags,
    );

    info!(topic = %arn, is_fifo, "Created topic");
    state.topics.insert(arn.clone(), topic);

    Ok(json!({ "TopicArn": arn }))
}

// ---------------------------------------------------------------------------
// DeleteTopic
// ---------------------------------------------------------------------------

pub fn delete_topic(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;

    let topic = state
        .topics
        .get(topic_arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Topic not found: {topic_arn}")))?;

    // Remove all subscriptions belonging to this topic
    let sub_arns: Vec<String> = topic.subscription_arns.clone();
    drop(topic);

    for sub_arn in sub_arns {
        state.subscriptions.remove(&sub_arn);
    }

    state.topics.remove(topic_arn);
    info!(topic = %topic_arn, "Deleted topic");

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListTopics
// ---------------------------------------------------------------------------

pub fn list_topics(
    state: &SnsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let topics: Vec<Value> = state
        .topics
        .iter()
        .map(|entry| json!({ "TopicArn": entry.key() }))
        .collect();

    Ok(json!({ "Topics": topics }))
}

// ---------------------------------------------------------------------------
// GetTopicAttributes
// ---------------------------------------------------------------------------

pub fn get_topic_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;

    let topic = state
        .topics
        .get(topic_arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Topic not found: {topic_arn}")))?;

    // Build subscription counts from live state
    let confirmed_count = state
        .subscriptions
        .iter()
        .filter(|e| e.topic_arn == topic_arn && e.confirmed)
        .count();
    let pending_count = state
        .subscriptions
        .iter()
        .filter(|e| e.topic_arn == topic_arn && !e.confirmed)
        .count();

    let mut attrs: serde_json::Map<String, Value> = topic
        .attributes
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    attrs.insert(
        "SubscriptionsConfirmed".to_string(),
        Value::String(confirmed_count.to_string()),
    );
    attrs.insert(
        "SubscriptionsPending".to_string(),
        Value::String(pending_count.to_string()),
    );

    Ok(json!({ "Attributes": attrs }))
}

// ---------------------------------------------------------------------------
// SetTopicAttributes
// ---------------------------------------------------------------------------

pub fn set_topic_attributes(
    state: &SnsState,
    input: &Value,
    ctx: &RequestContext,
    kms_lookup: Option<&dyn KmsKeyLookup>,
) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;

    let attr_name = input["AttributeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AttributeName is required"))?;

    let attr_value = input["AttributeValue"].as_str().unwrap_or("");

    if attr_name == "KmsMasterKeyId" && !attr_value.is_empty() {
        validate_kms_master_key_id(attr_value, ctx, kms_lookup)?;
    }

    let mut topic = state
        .topics
        .get_mut(topic_arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Topic not found: {topic_arn}")))?;

    topic
        .attributes
        .insert(attr_name.to_string(), attr_value.to_string());

    Ok(json!({}))
}

/// Validate a `KmsMasterKeyId` value. Accepts:
///   * the AWS-managed key alias `alias/aws/sns` (no lookup needed),
///   * any other key id / key ARN / alias / alias ARN — when a
///     [`KmsKeyLookup`] is wired, the reference must resolve in the
///     topic's account/region.
///
/// When no lookup is wired (standalone tests), only the AWS-managed
/// alias is accepted opportunistically; any other reference is
/// allowed through unchanged so SNS keeps working without KMS state.
fn validate_kms_master_key_id(
    key_ref: &str,
    ctx: &RequestContext,
    lookup: Option<&dyn KmsKeyLookup>,
) -> Result<(), AwsError> {
    if key_ref == "alias/aws/sns" {
        return Ok(());
    }
    let Some(lookup) = lookup else {
        return Ok(());
    };
    if lookup
        .resolve_key(key_ref, &ctx.account_id, &ctx.region)
        .is_some()
    {
        return Ok(());
    }
    Err(AwsError::bad_request(
        "KMSNotFoundException",
        format!("KmsMasterKeyId `{key_ref}` does not resolve to a KMS key."),
    ))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn validate_topic_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Topic name must not be empty",
        ));
    }
    if name.len() > 256 {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Topic name must be at most 256 characters",
        ));
    }
    let base = name.strip_suffix(".fifo").unwrap_or(name);
    if !base
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AwsError::bad_request(
            "InvalidParameter",
            "Topic name must contain only alphanumeric characters, hyphens, or underscores",
        ));
    }
    Ok(())
}

pub fn now_epoch_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod kms_validation_tests {
    use super::*;
    use std::collections::HashSet;

    struct StubLookup {
        known: HashSet<String>,
    }

    impl KmsKeyLookup for StubLookup {
        fn resolve_key(&self, key_ref: &str, _account: &str, _region: &str) -> Option<String> {
            if self.known.contains(key_ref) {
                Some(key_ref.to_string())
            } else {
                None
            }
        }
    }

    fn ctx() -> RequestContext {
        let mut c = RequestContext::new("sns", "us-east-1");
        c.account_id = "000000000000".into();
        c
    }

    #[test]
    fn aws_managed_alias_allowed_without_lookup() {
        validate_kms_master_key_id("alias/aws/sns", &ctx(), None).expect("alias/aws/sns ok");
    }

    #[test]
    fn unknown_key_rejected_when_lookup_wired() {
        let lookup = StubLookup {
            known: HashSet::new(),
        };
        let err = validate_kms_master_key_id("alias/customer", &ctx(), Some(&lookup)).unwrap_err();
        assert_eq!(err.code, "KMSNotFoundException");
    }

    #[test]
    fn known_alias_accepted_when_lookup_wired() {
        let mut known = HashSet::new();
        known.insert("alias/customer".to_string());
        let lookup = StubLookup { known };
        validate_kms_master_key_id("alias/customer", &ctx(), Some(&lookup))
            .expect("known alias resolves");
    }

    #[test]
    fn without_lookup_non_aws_alias_passes_through() {
        // Standalone test setup with no KMS state: only the AWS-managed
        // alias is the safe assertion; anything else must not block
        // CreateTopic for tests that don't wire KMS.
        validate_kms_master_key_id("alias/custom", &ctx(), None).expect("no lookup, no block");
    }
}
