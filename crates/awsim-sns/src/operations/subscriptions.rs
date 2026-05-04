use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::filter;
use crate::state::{SnsState, Subscription};

/// Parse and validate a FilterPolicy JSON string. AWS rejects
/// structurally invalid policies on Subscribe / SetSubscriptionAttributes
/// with InvalidParameter; storing them silently means filtering would
/// fail open.
fn validate_filter_policy_str(policy_str: &str) -> Result<(), AwsError> {
    let parsed: Value = serde_json::from_str(policy_str)
        .map_err(|_| AwsError::bad_request("InvalidParameter", "FilterPolicy is not valid JSON"))?;
    filter::validate_filter_policy(&parsed)
        .map_err(|msg| AwsError::bad_request("InvalidParameter", format!("FilterPolicy: {msg}")))
}

// ---------------------------------------------------------------------------
// Subscribe
// ---------------------------------------------------------------------------

/// Internal attribute key used to stash the confirmation token alongside
/// the subscription. AWS doesn't expose this on GetSubscriptionAttributes;
/// `subscription_summary` and `get_subscription_attributes` filter it out.
const CONFIRMATION_TOKEN_ATTR: &str = "_AwsimConfirmationToken";

/// Protocols where the subscription is immediately ready for delivery
/// and no confirmation token round-trip is required.
fn protocol_auto_confirms(protocol: &str) -> bool {
    matches!(protocol, "sqs" | "lambda" | "application" | "firehose")
}

pub fn subscribe(state: &SnsState, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;

    let protocol = input["Protocol"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Protocol is required"))?;

    validate_protocol(protocol)?;

    let endpoint = input["Endpoint"].as_str().unwrap_or("").to_string();
    let auto_confirm = protocol_auto_confirms(protocol);

    let mut topic = state
        .topics
        .get_mut(topic_arn)
        .ok_or_else(|| AwsError::not_found("NotFound", format!("Topic not found: {topic_arn}")))?;

    let sub_id = Uuid::new_v4();
    let topic_name = topic.name.clone();
    let sub_arn = format!(
        "arn:aws:sns:{}:{}:{}:{}",
        ctx.region, ctx.account_id, topic_name, sub_id
    );

    let mut sub_attributes: HashMap<String, String> = HashMap::new();
    sub_attributes.insert("SubscriptionArn".to_string(), sub_arn.clone());
    sub_attributes.insert("TopicArn".to_string(), topic_arn.to_string());
    sub_attributes.insert("Protocol".to_string(), protocol.to_string());
    sub_attributes.insert("Endpoint".to_string(), endpoint.clone());
    sub_attributes.insert(
        "ConfirmationWasAuthenticated".to_string(),
        auto_confirm.to_string(),
    );
    sub_attributes.insert(
        "PendingConfirmation".to_string(),
        (!auto_confirm).to_string(),
    );
    sub_attributes.insert("RawMessageDelivery".to_string(), "false".to_string());

    // For HTTP/HTTPS/email/sms, AWS issues a confirmation token via the
    // SubscriptionConfirmation control message. We stash a UUID-derived
    // token on the subscription so ConfirmSubscription can validate it.
    let confirmation_token = if auto_confirm {
        None
    } else {
        let token = Uuid::new_v4().simple().to_string().repeat(2);
        sub_attributes.insert(CONFIRMATION_TOKEN_ATTR.to_string(), token.clone());
        Some(token)
    };

    // Collect user-supplied attributes (overlay onto defaults). Validate
    // FilterPolicy up-front so a malformed policy is rejected at
    // Subscribe rather than silently letting every message through.
    if let Some(attrs) = input["Attributes"].as_object() {
        if let Some(fp) = attrs.get("FilterPolicy").and_then(Value::as_str)
            && !fp.is_empty()
        {
            validate_filter_policy_str(fp)?;
        }
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                sub_attributes.insert(k.clone(), s.to_string());
            }
        }
    }

    let subscription = Subscription {
        arn: sub_arn.clone(),
        topic_arn: topic_arn.to_string(),
        protocol: protocol.to_string(),
        endpoint,
        confirmed: auto_confirm,
        attributes: sub_attributes,
    };

    topic.subscription_arns.push(sub_arn.clone());
    drop(topic);
    state.subscriptions.insert(sub_arn.clone(), subscription);

    info!(
        sub = %sub_arn,
        topic = %topic_arn,
        protocol,
        auto_confirm,
        "Subscribed"
    );

    // Per AWS, Subscribe returns the placeholder string "pending confirmation"
    // as SubscriptionArn for protocols that require token round-trip. The
    // real ARN appears only after ConfirmSubscription succeeds.
    let returned_arn = if auto_confirm {
        sub_arn
    } else {
        let _ = confirmation_token; // token is on the subscription's attrs
        "pending confirmation".to_string()
    };

    Ok(json!({ "SubscriptionArn": returned_arn }))
}

// ---------------------------------------------------------------------------
// Unsubscribe
// ---------------------------------------------------------------------------

pub fn unsubscribe(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sub_arn = input["SubscriptionArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SubscriptionArn is required"))?;

    let removed = state.subscriptions.remove(sub_arn);
    if removed.is_none() {
        return Err(AwsError::not_found(
            "NotFound",
            format!("Subscription not found: {sub_arn}"),
        ));
    }

    // Remove from parent topic's list
    let topic_arn = removed.unwrap().1.topic_arn;
    if let Some(mut topic) = state.topics.get_mut(&topic_arn) {
        topic.subscription_arns.retain(|a| a != sub_arn);
    }

    info!(sub = %sub_arn, "Unsubscribed");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListSubscriptions
// ---------------------------------------------------------------------------

pub fn list_subscriptions(
    state: &SnsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let subs: Vec<Value> = state
        .subscriptions
        .iter()
        .map(|e| subscription_summary(&e))
        .collect();

    Ok(json!({ "Subscriptions": subs }))
}

// ---------------------------------------------------------------------------
// ListSubscriptionsByTopic
// ---------------------------------------------------------------------------

pub fn list_subscriptions_by_topic(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;

    if !state.topics.contains_key(topic_arn) {
        return Err(AwsError::not_found(
            "NotFound",
            format!("Topic not found: {topic_arn}"),
        ));
    }

    let subs: Vec<Value> = state
        .subscriptions
        .iter()
        .filter(|e| e.topic_arn == topic_arn)
        .map(|e| subscription_summary(&e))
        .collect();

    Ok(json!({ "Subscriptions": subs }))
}

// ---------------------------------------------------------------------------
// GetSubscriptionAttributes
// ---------------------------------------------------------------------------

pub fn get_subscription_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sub_arn = input["SubscriptionArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SubscriptionArn is required"))?;

    let sub = state.subscriptions.get(sub_arn).ok_or_else(|| {
        AwsError::not_found("NotFound", format!("Subscription not found: {sub_arn}"))
    })?;

    let attrs: serde_json::Map<String, Value> = sub
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != CONFIRMATION_TOKEN_ATTR)
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    Ok(json!({ "Attributes": attrs }))
}

// ---------------------------------------------------------------------------
// SetSubscriptionAttributes
// ---------------------------------------------------------------------------

pub fn set_subscription_attributes(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let sub_arn = input["SubscriptionArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "SubscriptionArn is required"))?;

    let attr_name = input["AttributeName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AttributeName is required"))?;

    let attr_value = input["AttributeValue"].as_str().unwrap_or("");

    if attr_name == "FilterPolicy" && !attr_value.is_empty() {
        validate_filter_policy_str(attr_value)?;
    }

    let mut sub = state.subscriptions.get_mut(sub_arn).ok_or_else(|| {
        AwsError::not_found("NotFound", format!("Subscription not found: {sub_arn}"))
    })?;

    sub.attributes
        .insert(attr_name.to_string(), attr_value.to_string());

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ConfirmSubscription
// ---------------------------------------------------------------------------

pub fn confirm_subscription(
    state: &SnsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;

    let token = input["Token"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Token is required"))?;

    // Find the pending subscription on this topic whose stored token
    // matches. Multiple subs on the same topic don't conflict because
    // each carries a unique token.
    let matching = state
        .subscriptions
        .iter()
        .find(|e| {
            e.topic_arn == topic_arn
                && e.attributes
                    .get(CONFIRMATION_TOKEN_ATTR)
                    .map(String::as_str)
                    == Some(token)
        })
        .map(|e| e.arn.clone());

    let arn = matching.ok_or_else(|| {
        AwsError::bad_request(
            "InvalidParameter",
            "Confirmation token does not match any pending subscription",
        )
    })?;

    if let Some(mut sub) = state.subscriptions.get_mut(&arn) {
        sub.confirmed = true;
        sub.attributes
            .insert("PendingConfirmation".to_string(), "false".to_string());
        sub.attributes.remove(CONFIRMATION_TOKEN_ATTR);
    }
    Ok(json!({ "SubscriptionArn": arn }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn validate_protocol(protocol: &str) -> Result<(), AwsError> {
    match protocol {
        "sqs" | "lambda" | "http" | "https" | "email" | "email-json" | "sms" | "application"
        | "firehose" => Ok(()),
        _ => Err(AwsError::bad_request(
            "InvalidParameter",
            format!(
                "Invalid protocol: {protocol}. Must be one of: sqs, lambda, http, https, email, email-json, sms, application, firehose"
            ),
        )),
    }
}

fn subscription_summary(sub: &Subscription) -> Value {
    json!({
        "SubscriptionArn": sub.arn,
        "TopicArn": sub.topic_arn,
        "Protocol": sub.protocol,
        "Endpoint": sub.endpoint,
        "Owner": "",
    })
}
