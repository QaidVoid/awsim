use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::resource_not_found,
    ids::{arn_suffix, rule_arn},
    state::{ElbState, Rule},
};

use super::{extract_string_list, opt_str, require_str};
use super::listeners::parse_actions;

pub fn rule_to_value(r: &Rule) -> Value {
    let actions: Vec<Value> = r
        .actions
        .iter()
        .map(|a| {
            let mut v = json!({ "Type": a.action_type });
            if let Some(ref tg) = a.target_group_arn {
                v["TargetGroupArn"] = json!(tg);
            }
            v
        })
        .collect();

    json!({
        "RuleArn": r.arn,
        "Priority": r.priority,
        "Conditions": r.conditions,
        "Actions": { "member": actions },
        "IsDefault": r.is_default,
    })
}

pub fn create_rule(
    state: &ElbState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let listener_arn_str = require_str(input, "ListenerArn")?.to_string();

    // Ensure listener exists and find LB name/rand for the ARN
    let listener = state
        .listeners
        .get(&listener_arn_str)
        .ok_or_else(|| resource_not_found("listener", &listener_arn_str))?;

    let lb_arn = listener.load_balancer_arn.clone();
    drop(listener);

    let lb = state
        .load_balancers
        .get(&lb_arn)
        .ok_or_else(|| resource_not_found("load balancer", &lb_arn))?;

    let lb_name = lb.name.clone();
    let lb_rand = arn_suffix(&lb_arn).to_string();
    let listener_rand = arn_suffix(&listener_arn_str).to_string();
    drop(lb);

    let priority = opt_str(input, "Priority").unwrap_or("1").to_string();

    let conditions = input
        .get("Conditions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let actions = parse_actions(input, "Actions");

    let arn = rule_arn(&ctx.region, &ctx.account_id, &lb_name, &lb_rand, &listener_rand);

    let rule = Rule {
        arn: arn.clone(),
        listener_arn: listener_arn_str,
        priority,
        conditions,
        actions,
        is_default: false,
    };

    let result = rule_to_value(&rule);
    state.rules.insert(arn, rule);

    Ok(json!({
        "CreateRuleResult": {
            "Rules": {
                "member": [result]
            }
        }
    }))
}

pub fn delete_rule(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "RuleArn")?;

    if state.rules.remove(arn).is_none() {
        return Err(resource_not_found("rule", arn));
    }

    Ok(json!({ "DeleteRuleResult": {} }))
}

pub fn modify_rule(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let arn = require_str(input, "RuleArn")?;

    let mut rule = state
        .rules
        .get_mut(arn)
        .ok_or_else(|| resource_not_found("rule", arn))?;

    if let Some(conditions) = input.get("Conditions").and_then(|v| v.as_array()) {
        rule.conditions = conditions.to_vec();
    }

    let new_actions = parse_actions(input, "Actions");
    if !new_actions.is_empty() {
        rule.actions = new_actions;
    }

    let result = rule_to_value(&rule);

    Ok(json!({
        "ModifyRuleResult": {
            "Rules": {
                "member": [result]
            }
        }
    }))
}

pub fn describe_rules(state: &ElbState, input: &Value) -> Result<Value, AwsError> {
    let listener_arn_filter = opt_str(input, "ListenerArn").map(|s| s.to_string());
    let rule_arns = extract_string_list(input, "RuleArns");

    let rules: Vec<Value> = state
        .rules
        .iter()
        .filter(|e| {
            let r = e.value();
            let listener_ok = listener_arn_filter
                .as_ref()
                .map_or(true, |arn| &r.listener_arn == arn);
            let arn_ok = rule_arns.is_empty() || rule_arns.contains(&r.arn);
            listener_ok && arn_ok
        })
        .map(|e| rule_to_value(e.value()))
        .collect();

    Ok(json!({
        "DescribeRulesResult": {
            "Rules": {
                "member": rules
            },
            "NextMarker": null
        }
    }))
}
