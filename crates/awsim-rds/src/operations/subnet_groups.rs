use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{db_subnet_group_already_exists, db_subnet_group_not_found, missing_parameter},
    ids::subnet_group_arn,
    state::{DbSubnetGroup, RdsState},
};

use super::{opt_str, require_str};

fn subnet_group_to_value(sg: &DbSubnetGroup) -> Value {
    json!({
        "DBSubnetGroupName": sg.name,
        "DBSubnetGroupArn": sg.arn,
        "DBSubnetGroupDescription": sg.description,
        "SubnetGroupStatus": sg.status,
        "Subnets": sg.subnet_ids.iter().map(|id| json!({
            "SubnetIdentifier": id,
            "SubnetStatus": "Active",
        })).collect::<Vec<_>>(),
    })
}

pub fn create_db_subnet_group(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBSubnetGroupName")?;
    let description = require_str(input, "DBSubnetGroupDescription")?;

    // SubnetIds is a list
    let subnet_ids: Vec<String> = input
        .get("SubnetIds")
        .and_then(|v| {
            // Accept both array and comma-separated string forms.
            if let Some(arr) = v.as_array() {
                Some(arr.iter().filter_map(|x| x.as_str()).map(|s| s.to_string()).collect())
            } else if let Some(s) = v.as_str() {
                Some(s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            } else {
                None
            }
        })
        .unwrap_or_default();

    if subnet_ids.is_empty() {
        return Err(missing_parameter("SubnetIds"));
    }

    if state.subnet_groups.contains_key(name) {
        return Err(db_subnet_group_already_exists(name));
    }

    let arn = subnet_group_arn(&ctx.region, &ctx.account_id, name);
    let sg = DbSubnetGroup {
        name: name.to_string(),
        arn,
        description: description.to_string(),
        subnet_ids,
        status: "Complete".to_string(),
    };

    let result = subnet_group_to_value(&sg);
    state.subnet_groups.insert(name.to_string(), sg);

    Ok(json!({ "DBSubnetGroup": result }))
}

pub fn delete_db_subnet_group(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBSubnetGroupName")?;

    if !state.subnet_groups.contains_key(name) {
        return Err(db_subnet_group_not_found(name));
    }

    state.subnet_groups.remove(name);
    Ok(json!({}))
}

pub fn describe_db_subnet_groups(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_name = opt_str(input, "DBSubnetGroupName");

    if let Some(name) = filter_name {
        let sg = state
            .subnet_groups
            .get(name)
            .ok_or_else(|| db_subnet_group_not_found(name))?;
        let items = vec![subnet_group_to_value(&sg)];
        return Ok(json!({
            "DBSubnetGroups": { "DBSubnetGroup": items },
            "Marker": null,
        }));
    }

    let items: Vec<Value> = state
        .subnet_groups
        .iter()
        .map(|e| subnet_group_to_value(e.value()))
        .collect();

    Ok(json!({
        "DBSubnetGroups": { "DBSubnetGroup": items },
        "Marker": null,
    }))
}
