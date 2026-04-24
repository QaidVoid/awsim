use awsim_core::{AwsError, InternalEvent, RequestContext};
use serde_json::{Value, json};
use tracing::{info, warn};
use uuid::Uuid;

use crate::operations::buses::ensure_default_bus;
use crate::state::{EventBridgeState, Rule, StoredEvent};

// ---------------------------------------------------------------------------
// PutEvents
// ---------------------------------------------------------------------------

pub fn put_events(
    state: &EventBridgeState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let entries = input["Entries"]
        .as_array()
        .ok_or_else(|| AwsError::bad_request("InvalidParameterValue", "Entries is required"))?;

    ensure_default_bus(state, ctx);

    let mut result_entries: Vec<Value> = Vec::new();
    let mut failed_count = 0u64;

    for entry in entries {
        let bus_name = entry["EventBusName"].as_str().unwrap_or("default");
        let source = entry["Source"].as_str().unwrap_or("").to_string();
        let detail_type = entry["DetailType"].as_str().unwrap_or("").to_string();
        let detail = entry["Detail"].as_str().unwrap_or("{}").to_string();

        // Validate required fields
        if source.is_empty() {
            failed_count += 1;
            result_entries.push(json!({
                "ErrorCode": "InvalidParameterValue",
                "ErrorMessage": "Source is required",
            }));
            continue;
        }

        if detail_type.is_empty() {
            failed_count += 1;
            result_entries.push(json!({
                "ErrorCode": "InvalidParameterValue",
                "ErrorMessage": "DetailType is required",
            }));
            continue;
        }

        // Ensure bus exists
        if !state.event_buses.contains_key(bus_name) {
            failed_count += 1;
            result_entries.push(json!({
                "ErrorCode": "ResourceNotFoundException",
                "ErrorMessage": format!("Event bus {bus_name} does not exist"),
            }));
            continue;
        }

        let resources: Vec<String> = entry["Resources"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let event_id = Uuid::new_v4().to_string();

        // Build the original event object for target invocations.
        let original_event = json!({
            "id": event_id,
            "source": source,
            "detail-type": detail_type,
            "detail": detail,
            "resources": resources,
        });

        // Match event against rules on the bus
        let matched_rules = match_event_against_rules_with_targets(
            state,
            bus_name,
            &source,
            &detail_type,
            &original_event,
            ctx,
        );

        if !matched_rules.is_empty() {
            info!(
                event_id = %event_id,
                source = %source,
                detail_type = %detail_type,
                bus = %bus_name,
                matched_rules = ?matched_rules,
                "Event matched rules"
            );
        } else {
            info!(
                event_id = %event_id,
                source = %source,
                detail_type = %detail_type,
                bus = %bus_name,
                "Event delivered (no rules matched)"
            );
        }

        let stored = StoredEvent {
            event_id: event_id.clone(),
            source,
            detail_type,
            detail,
            event_bus_name: bus_name.to_string(),
            resources,
            matched_rules,
        };
        state.recent_events.insert(event_id.clone(), stored);

        result_entries.push(json!({ "EventId": event_id }));
    }

    Ok(json!({
        "FailedEntryCount": failed_count,
        "Entries": result_entries,
    }))
}

// ---------------------------------------------------------------------------
// Pattern matching helpers
// ---------------------------------------------------------------------------

/// Return the names of all ENABLED rules on `bus_name` that match this event.
/// For each matched rule, emit an `eventbridge:TargetInvocation` InternalEvent
/// for every configured target so the integration layer can dispatch them.
fn match_event_against_rules_with_targets(
    state: &EventBridgeState,
    bus_name: &str,
    source: &str,
    detail_type: &str,
    original_event: &Value,
    ctx: &RequestContext,
) -> Vec<String> {
    let bus = match state.event_buses.get(bus_name) {
        Some(b) => b,
        None => return vec![],
    };

    let mut matched_rule_names: Vec<String> = Vec::new();

    for rule in bus.rules.values() {
        if rule.state != "ENABLED" {
            continue;
        }
        if !matches_pattern(rule, source, detail_type) {
            continue;
        }

        matched_rule_names.push(rule.name.clone());

        // Emit one InternalEvent per target so the router can dispatch them.
        if let Some(ref event_bus) = ctx.event_bus {
            for target in &rule.targets {
                event_bus.publish(InternalEvent {
                    source: "events".to_string(),
                    event_type: "eventbridge:TargetInvocation".to_string(),
                    region: ctx.region.clone(),
                    account_id: ctx.account_id.clone(),
                    detail: json!({
                        "targetArn": target.arn,
                        "targetId": target.id,
                        "ruleName": rule.name,
                        "event": original_event,
                    }),
                });
            }
        }
    }

    matched_rule_names
}

/// Check whether an event matches a rule's EventPattern.
///
/// Supported pattern fields:
/// - `source`: array of allowed source strings
/// - `detail-type`: array of allowed detail-type strings
///
/// A rule with no EventPattern (schedule-only rule) never matches PutEvents.
fn matches_pattern(rule: &Rule, source: &str, detail_type: &str) -> bool {
    let pattern_str = match &rule.event_pattern {
        Some(p) => p,
        None => return false,
    };

    let pattern: Value = match serde_json::from_str(pattern_str) {
        Ok(v) => v,
        Err(e) => {
            warn!(rule = %rule.name, error = %e, "Failed to parse event pattern");
            return false;
        }
    };

    // Check `source` array (any-of semantics)
    if let Some(sources) = pattern["source"].as_array() {
        let matched = sources.iter().any(|s| s.as_str() == Some(source));
        if !matched {
            return false;
        }
    }

    // Check `detail-type` array (any-of semantics)
    if let Some(detail_types) = pattern["detail-type"].as_array() {
        let matched = detail_types
            .iter()
            .any(|dt| dt.as_str() == Some(detail_type));
        if !matched {
            return false;
        }
    }

    true
}
