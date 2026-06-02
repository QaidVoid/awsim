use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{db_parameter_group_already_exists, db_parameter_group_not_found},
    ids::parameter_group_arn,
    state::{DbParameterGroup, RdsState},
};

use super::{opt_str, require_str};

fn parameter_group_to_value(pg: &DbParameterGroup) -> Value {
    json!({
        "DBParameterGroupName": pg.name,
        "DBParameterGroupArn": pg.arn,
        "DBParameterGroupFamily": pg.family,
        "Description": pg.description,
    })
}

pub fn create_db_parameter_group(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBParameterGroupName")?;
    let family = require_str(input, "DBParameterGroupFamily")?;
    let description = require_str(input, "Description")?;

    if state.parameter_groups.contains_key(name) {
        return Err(db_parameter_group_already_exists(name));
    }

    let arn = parameter_group_arn(&ctx.partition, &ctx.region, &ctx.account_id, name);
    let pg = DbParameterGroup {
        name: name.to_string(),
        arn,
        family: family.to_string(),
        description: description.to_string(),
    };

    let result = parameter_group_to_value(&pg);
    state.parameter_groups.insert(name.to_string(), pg);

    Ok(json!({ "DBParameterGroup": result }))
}

pub fn delete_db_parameter_group(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBParameterGroupName")?;

    if !state.parameter_groups.contains_key(name) {
        return Err(db_parameter_group_not_found(name));
    }

    state.parameter_groups.remove(name);
    Ok(json!({}))
}

pub fn describe_db_parameter_groups(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_name = opt_str(input, "DBParameterGroupName");

    if let Some(name) = filter_name {
        let pg = state
            .parameter_groups
            .get(name)
            .ok_or_else(|| db_parameter_group_not_found(name))?;
        let items = vec![parameter_group_to_value(&pg)];
        return Ok(json!({
            "DBParameterGroups": { "DBParameterGroup": items },
            "Marker": null,
        }));
    }

    let items: Vec<Value> = state
        .parameter_groups
        .iter()
        .map(|e| parameter_group_to_value(e.value()))
        .collect();

    Ok(json!({
        "DBParameterGroups": { "DBParameterGroup": items },
        "Marker": null,
    }))
}
