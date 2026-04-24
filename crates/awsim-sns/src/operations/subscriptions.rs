use std::collections::HashMap;

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{SnsState, Subscription};

// ---------------------------------------------------------------------------
// Subscribe
// ---------------------------------------------------------------------------

pub fn subscribe(state: &SnsState, input: &Value, ctx: &RequestContext) -> Result<Value, AwsError> {
    let topic_arn = input["TopicArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TopicArn is required"))?;

    let protocol = input["Protocol"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Protocol is required"))?;

    validate_protocol(protocol)?;

    let endpoint = input["Endpoint"].as_str().unwrap_or("").to_string();

    // Topic must exist
    let topic_name = {
        let mut topic = state.topics.get_mut(topic_arn).ok_or_else(|| {
            AwsError::not_found("NotFound", format!("Topic not found: {topic_arn}"))
        })?;

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
            "true".to_string(),
        );
        sub_attributes.insert("PendingConfirmation".to_string(), "false".to_string());
        sub_attributes.insert("RawMessageDelivery".to_string(), "false".to_string());

        // Collect user-supplied attributes
        if let Some(attrs) = input["Attributes"].as_object() {
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
            confirmed: true, // auto-confirm for local dev
            attributes: sub_attributes,
        };

        topic.subscription_arns.push(sub_arn.clone());
        state.subscriptions.insert(sub_arn.clone(), subscription);

        info!(sub = %sub_arn, topic = %topic_arn, protocol, "Subscribed");
        topic_name
    };

    // Build the subscription ARN from the stored entry (already inserted above)
    // Re-derive it — we need the arn that was just inserted
    let sub_arn = state
        .subscriptions
        .iter()
        .find(|e| e.topic_arn == topic_arn && e.protocol == protocol)
        .map(|e| e.arn.clone())
        .unwrap_or_else(|| {
            format!(
                "arn:aws:sns:{}:{}:{}:{}",
                ctx.region,
                ctx.account_id,
                topic_name,
                Uuid::new_v4()
            )
        });

    Ok(json!({ "SubscriptionArn": sub_arn }))
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

    // Token is accepted but ignored — all subscriptions are auto-confirmed
    let _token = input["Token"].as_str().unwrap_or("");

    // Find any pending subscription for this topic and confirm it
    let sub_arn = state
        .subscriptions
        .iter()
        .find(|e| e.topic_arn == topic_arn)
        .map(|e| e.arn.clone());

    match sub_arn {
        Some(arn) => {
            if let Some(mut sub) = state.subscriptions.get_mut(&arn) {
                sub.confirmed = true;
            }
            Ok(json!({ "SubscriptionArn": arn }))
        }
        None => Err(AwsError::not_found(
            "NotFound",
            format!("No subscription found for topic: {topic_arn}"),
        )),
    }
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
