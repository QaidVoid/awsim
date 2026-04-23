use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{
    ConfigurationSet, Contact, ContactList, CustomVerificationTemplate, DedicatedIpPool,
    EventDestination, SentEmail, SesState, SuppressedDestination,
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn send_bulk_email(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let from = input["FromEmailAddress"].as_str().unwrap_or("noreply@awsim.local").to_string();
    let entries = input["BulkEmailEntries"].as_array().cloned().unwrap_or_default();

    let mut results = Vec::new();
    for entry in entries {
        let to: Vec<String> = entry["Destination"]["ToAddresses"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let message_id = Uuid::new_v4().to_string();
        let email = SentEmail {
            message_id: message_id.clone(),
            from: from.clone(),
            to,
            cc: vec![],
            bcc: vec![],
            subject: None,
            body_text: None,
            body_html: None,
            raw: None,
            sent_at: now(),
        };
        state.sent_emails.insert(message_id.clone(), email);
        results.push(json!({
            "MessageId": message_id,
            "Status": "SUCCESS",
        }));
    }

    Ok(json!({ "BulkEmailEntryResults": results }))
}

pub fn send_custom_verification_email(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["EmailAddress"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailAddress is required"))?;
    let template = input["TemplateName"].as_str().unwrap_or("default");
    let message_id = Uuid::new_v4().to_string();
    let entry = SentEmail {
        message_id: message_id.clone(),
        from: format!("verification@awsim.local"),
        to: vec![email.to_string()],
        cc: vec![],
        bcc: vec![],
        subject: Some(format!("Verify {email} via {template}")),
        body_text: None,
        body_html: None,
        raw: None,
        sent_at: now(),
    };
    state.sent_emails.insert(message_id.clone(), entry);
    Ok(json!({ "MessageId": message_id }))
}

pub fn put_email_identity_dkim_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn put_email_identity_mail_from_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn put_email_identity_feedback_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn create_email_identity_policy(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailIdentity is required"))?;
    let policy_name = input["PolicyName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PolicyName is required"))?;
    let policy = input["Policy"].as_str().unwrap_or("{}");
    state
        .identity_policies
        .entry(identity.to_string())
        .or_default()
        .insert(policy_name.to_string(), policy.to_string());
    Ok(json!({}))
}

pub fn delete_email_identity_policy(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"].as_str().unwrap_or("");
    let policy_name = input["PolicyName"].as_str().unwrap_or("");
    if let Some(mut entry) = state.identity_policies.get_mut(identity) {
        entry.remove(policy_name);
    }
    Ok(json!({}))
}

pub fn get_email_identity_policies(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identity = input["EmailIdentity"].as_str().unwrap_or("");
    let policies: HashMap<String, String> = state
        .identity_policies
        .get(identity)
        .map(|p| p.clone())
        .unwrap_or_default();
    let mut obj = serde_json::Map::new();
    for (k, v) in policies {
        obj.insert(k, Value::String(v));
    }
    Ok(json!({ "Policies": Value::Object(obj) }))
}

pub fn update_email_identity_policy(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    create_email_identity_policy(state, input, _ctx)
}

pub fn create_configuration_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "ConfigurationSetName is required")
        })?;
    let mut cs = ConfigurationSet {
        name: name.to_string(),
        sending_enabled: true,
        reputation_metrics_enabled: true,
        ..Default::default()
    };
    if let Some(t) = input["Tags"].as_array() {
        for tag in t {
            let k = tag["Key"].as_str().unwrap_or("").to_string();
            let v = tag["Value"].as_str().unwrap_or("").to_string();
            cs.tags.insert(k, v);
        }
    }
    state.configuration_sets.insert(name.to_string(), cs);
    Ok(json!({}))
}

pub fn delete_configuration_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"].as_str().unwrap_or("");
    state.configuration_sets.remove(name);
    Ok(json!({}))
}

pub fn get_configuration_set(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ConfigurationSetName"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "ConfigurationSetName is required")
        })?;
    let cs = state
        .configuration_sets
        .get(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    let tags: Vec<Value> = cs
        .tags
        .iter()
        .map(|(k, v)| json!({ "Key": k, "Value": v }))
        .collect();
    Ok(json!({
        "ConfigurationSetName": cs.name,
        "Tags": tags,
        "SendingOptions": { "SendingEnabled": cs.sending_enabled },
        "ReputationOptions": { "ReputationMetricsEnabled": cs.reputation_metrics_enabled },
    }))
}

pub fn list_configuration_sets(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<String> = state
        .configuration_sets
        .iter()
        .map(|e| e.key().clone())
        .collect();
    Ok(json!({ "ConfigurationSets": names }))
}

pub fn create_configuration_set_event_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cs_name = input["ConfigurationSetName"].as_str().unwrap_or("");
    let dest_name = input["EventDestinationName"].as_str().unwrap_or("default");
    let event_dest = &input["EventDestination"];
    let event_types: Vec<String> = event_dest["MatchingEventTypes"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if let Some(mut cs) = state.configuration_sets.get_mut(cs_name) {
        cs.event_destinations.push(EventDestination {
            name: dest_name.to_string(),
            enabled: event_dest["Enabled"].as_bool().unwrap_or(true),
            matching_event_types: event_types,
        });
    }
    Ok(json!({}))
}

pub fn delete_configuration_set_event_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cs_name = input["ConfigurationSetName"].as_str().unwrap_or("");
    let dest_name = input["EventDestinationName"].as_str().unwrap_or("");
    if let Some(mut cs) = state.configuration_sets.get_mut(cs_name) {
        cs.event_destinations.retain(|d| d.name != dest_name);
    }
    Ok(json!({}))
}

pub fn get_configuration_set_event_destinations(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cs_name = input["ConfigurationSetName"].as_str().unwrap_or("");
    let destinations: Vec<Value> = state
        .configuration_sets
        .get(cs_name)
        .map(|cs| {
            cs.event_destinations
                .iter()
                .map(|d| {
                    json!({
                        "Name": d.name,
                        "Enabled": d.enabled,
                        "MatchingEventTypes": d.matching_event_types,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "EventDestinations": destinations }))
}

pub fn create_dedicated_ip_pool(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["PoolName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "PoolName is required"))?;
    let scaling_mode = input["ScalingMode"].as_str().unwrap_or("STANDARD").to_string();
    let pool = DedicatedIpPool {
        name: name.to_string(),
        scaling_mode,
        ips: vec!["192.0.2.1".to_string(), "192.0.2.2".to_string()],
    };
    state.dedicated_ip_pools.insert(name.to_string(), pool);
    Ok(json!({}))
}

pub fn delete_dedicated_ip_pool(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["PoolName"].as_str().unwrap_or("");
    state.dedicated_ip_pools.remove(name);
    Ok(json!({}))
}

pub fn get_dedicated_ip_pool(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["PoolName"].as_str().unwrap_or("");
    let pool = state
        .dedicated_ip_pools
        .get(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    Ok(json!({
        "DedicatedIpPool": {
            "PoolName": pool.name,
            "ScalingMode": pool.scaling_mode,
        }
    }))
}

pub fn list_dedicated_ip_pools(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let names: Vec<String> = state
        .dedicated_ip_pools
        .iter()
        .map(|e| e.key().clone())
        .collect();
    Ok(json!({ "DedicatedIpPools": names }))
}

pub fn get_dedicated_ips(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["PoolName"].as_str().unwrap_or("");
    let ips: Vec<Value> = state
        .dedicated_ip_pools
        .get(name)
        .map(|p| {
            p.ips
                .iter()
                .map(|ip| {
                    json!({
                        "Ip": ip,
                        "WarmupStatus": "DONE",
                        "WarmupPercentage": 100,
                        "PoolName": p.name,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "DedicatedIps": ips }))
}

pub fn put_suppressed_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["EmailAddress"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailAddress is required"))?;
    let reason = input["Reason"].as_str().unwrap_or("BOUNCE").to_string();
    state.suppressed_destinations.insert(
        email.to_string(),
        SuppressedDestination {
            email: email.to_string(),
            reason,
            last_update: now(),
        },
    );
    Ok(json!({}))
}

pub fn delete_suppressed_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["EmailAddress"].as_str().unwrap_or("");
    state.suppressed_destinations.remove(email);
    Ok(json!({}))
}

pub fn get_suppressed_destination(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["EmailAddress"].as_str().unwrap_or("");
    let s = state
        .suppressed_destinations
        .get(email)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {email}")))?;
    Ok(json!({
        "SuppressedDestination": {
            "EmailAddress": s.email,
            "Reason": s.reason,
            "LastUpdateTime": s.last_update,
        }
    }))
}

pub fn list_suppressed_destinations(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let summaries: Vec<Value> = state
        .suppressed_destinations
        .iter()
        .map(|e| {
            json!({
                "EmailAddress": e.value().email,
                "Reason": e.value().reason,
                "LastUpdateTime": e.value().last_update,
            })
        })
        .collect();
    Ok(json!({ "SuppressedDestinationSummaries": summaries }))
}

pub fn create_contact_list(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ContactListName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "ContactListName is required"))?;
    let description = input["Description"].as_str().map(String::from);
    let topics = input["Topics"].as_array().cloned().unwrap_or_default();
    state.contact_lists.insert(
        name.to_string(),
        ContactList {
            name: name.to_string(),
            description,
            topics,
            created_at: now(),
        },
    );
    Ok(json!({}))
}

pub fn delete_contact_list(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ContactListName"].as_str().unwrap_or("");
    state.contact_lists.remove(name);
    Ok(json!({}))
}

pub fn get_contact_list(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ContactListName"].as_str().unwrap_or("");
    let cl = state
        .contact_lists
        .get(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    Ok(json!({
        "ContactListName": cl.name,
        "Description": cl.description,
        "Topics": cl.topics,
        "CreatedTimestamp": cl.created_at,
    }))
}

pub fn list_contact_lists(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let lists: Vec<Value> = state
        .contact_lists
        .iter()
        .map(|e| {
            json!({
                "ContactListName": e.value().name,
                "LastUpdatedTimestamp": e.value().created_at,
            })
        })
        .collect();
    Ok(json!({ "ContactLists": lists }))
}

pub fn update_contact_list(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["ContactListName"].as_str().unwrap_or("");
    if let Some(mut cl) = state.contact_lists.get_mut(name) {
        if let Some(d) = input["Description"].as_str() {
            cl.description = Some(d.to_string());
        }
        if let Some(t) = input["Topics"].as_array() {
            cl.topics = t.clone();
        }
    }
    Ok(json!({}))
}

fn contact_key(list: &str, email: &str) -> String {
    format!("{list}#{email}")
}

pub fn create_contact(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let email = input["EmailAddress"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "EmailAddress is required"))?;
    let topic_prefs = input["TopicPreferences"].as_array().cloned().unwrap_or_default();
    let unsubscribe = input["UnsubscribeAll"].as_bool().unwrap_or(false);
    let attributes = input["AttributesData"].as_str().map(String::from);
    state.contacts.insert(
        contact_key(list_name, email),
        Contact {
            email: email.to_string(),
            list_name: list_name.to_string(),
            topic_preferences: topic_prefs,
            unsubscribe_all: unsubscribe,
            attributes,
            created_at: now(),
        },
    );
    Ok(json!({}))
}

pub fn delete_contact(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let email = input["EmailAddress"].as_str().unwrap_or("");
    state.contacts.remove(&contact_key(list_name, email));
    Ok(json!({}))
}

pub fn get_contact(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let email = input["EmailAddress"].as_str().unwrap_or("");
    let c = state
        .contacts
        .get(&contact_key(list_name, email))
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {email}")))?;
    Ok(json!({
        "ContactListName": c.list_name,
        "EmailAddress": c.email,
        "TopicPreferences": c.topic_preferences,
        "UnsubscribeAll": c.unsubscribe_all,
        "AttributesData": c.attributes,
        "CreatedTimestamp": c.created_at,
    }))
}

pub fn list_contacts(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let contacts: Vec<Value> = state
        .contacts
        .iter()
        .filter(|e| e.value().list_name == list_name)
        .map(|e| {
            json!({
                "EmailAddress": e.value().email,
                "TopicPreferences": e.value().topic_preferences,
                "UnsubscribeAll": e.value().unsubscribe_all,
            })
        })
        .collect();
    Ok(json!({ "Contacts": contacts }))
}

pub fn update_contact(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list_name = input["ContactListName"].as_str().unwrap_or("");
    let email = input["EmailAddress"].as_str().unwrap_or("");
    if let Some(mut c) = state.contacts.get_mut(&contact_key(list_name, email)) {
        if let Some(t) = input["TopicPreferences"].as_array() {
            c.topic_preferences = t.clone();
        }
        if let Some(u) = input["UnsubscribeAll"].as_bool() {
            c.unsubscribe_all = u;
        }
        if let Some(a) = input["AttributesData"].as_str() {
            c.attributes = Some(a.to_string());
        }
    }
    Ok(json!({}))
}

pub fn create_custom_verification_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TemplateName is required"))?;
    let cv = CustomVerificationTemplate {
        name: name.to_string(),
        from: input["FromEmailAddress"].as_str().unwrap_or("").to_string(),
        subject: input["TemplateSubject"].as_str().unwrap_or("").to_string(),
        content: input["TemplateContent"].as_str().unwrap_or("").to_string(),
        success_url: input["SuccessRedirectionURL"].as_str().unwrap_or("").to_string(),
        failure_url: input["FailureRedirectionURL"].as_str().unwrap_or("").to_string(),
    };
    state.custom_verification_templates.insert(name.to_string(), cv);
    Ok(json!({}))
}

pub fn delete_custom_verification_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"].as_str().unwrap_or("");
    state.custom_verification_templates.remove(name);
    Ok(json!({}))
}

pub fn get_custom_verification_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"].as_str().unwrap_or("");
    let cv = state
        .custom_verification_templates
        .get(name)
        .ok_or_else(|| AwsError::not_found("NotFoundException", format!("Not found: {name}")))?;
    Ok(json!({
        "TemplateName": cv.name,
        "FromEmailAddress": cv.from,
        "TemplateSubject": cv.subject,
        "TemplateContent": cv.content,
        "SuccessRedirectionURL": cv.success_url,
        "FailureRedirectionURL": cv.failure_url,
    }))
}

pub fn list_custom_verification_email_templates(
    state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let templates: Vec<Value> = state
        .custom_verification_templates
        .iter()
        .map(|e| {
            json!({
                "TemplateName": e.value().name,
                "FromEmailAddress": e.value().from,
                "TemplateSubject": e.value().subject,
                "SuccessRedirectionURL": e.value().success_url,
                "FailureRedirectionURL": e.value().failure_url,
            })
        })
        .collect();
    Ok(json!({ "CustomVerificationEmailTemplates": templates }))
}

pub fn update_email_template(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["TemplateName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "TemplateName is required"))?;
    let content = &input["TemplateContent"];
    if let Some(mut t) = state.templates.get_mut(name) {
        if let Some(s) = content["Subject"].as_str() {
            t.subject = Some(s.to_string());
        }
        if let Some(h) = content["Html"].as_str() {
            t.html = Some(h.to_string());
        }
        if let Some(tx) = content["Text"].as_str() {
            t.text = Some(tx.to_string());
        }
    }
    Ok(json!({}))
}

pub fn tag_resource(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceArn"].as_str().unwrap_or("");
    let tags = input["Tags"].as_array().cloned().unwrap_or_default();
    let mut entry = state.identity_tags.entry(arn.to_string()).or_default();
    for tag in tags {
        let k = tag["Key"].as_str().unwrap_or("").to_string();
        let v = tag["Value"].as_str().unwrap_or("").to_string();
        entry.insert(k, v);
    }
    Ok(json!({}))
}

pub fn untag_resource(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceArn"].as_str().unwrap_or("");
    let keys = input["TagKeys"].as_array().cloned().unwrap_or_default();
    if let Some(mut entry) = state.identity_tags.get_mut(arn) {
        for k in keys {
            if let Some(s) = k.as_str() {
                entry.remove(s);
            }
        }
    }
    Ok(json!({}))
}

pub fn list_tags_for_resource(
    state: &SesState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let arn = input["ResourceArn"].as_str().unwrap_or("");
    let tags: Vec<Value> = state
        .identity_tags
        .get(arn)
        .map(|t| {
            t.iter()
                .map(|(k, v)| json!({ "Key": k, "Value": v }))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "Tags": tags }))
}

pub fn put_account_sending_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn put_account_suppression_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn put_account_dedicated_ip_warmup_attributes(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn get_deliverability_dashboard_options(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({
        "DashboardEnabled": false,
        "ActiveSubscribedDomains": [],
        "PendingExpirationSubscribedDomains": [],
    }))
}

pub fn put_deliverability_dashboard_option(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({}))
}

pub fn get_blacklist_reports(
    _state: &SesState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    Ok(json!({ "BlacklistReport": {} }))
}
