use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{db_parameter_group_already_exists, db_parameter_group_not_found},
    ids::cluster_parameter_group_arn,
    state::{DbClusterParameterGroup, RdsState},
};

use super::{opt_str, require_str};

/// One engine-default parameter in a family's catalog.
struct CatalogParam {
    name: &'static str,
    value: &'static str,
    apply_type: &'static str,
    data_type: &'static str,
    description: &'static str,
}

/// Engine-default cluster parameters for a parameter group family. The
/// catalog branches on the family prefix so Aurora PostgreSQL and Aurora
/// MySQL each surface their own well-known settings.
fn default_cluster_parameters(family: &str) -> &'static [CatalogParam] {
    const POSTGRES: &[CatalogParam] = &[
        CatalogParam {
            name: "rds.force_ssl",
            value: "1",
            apply_type: "static",
            data_type: "boolean",
            description: "Force SSL connections to the database.",
        },
        CatalogParam {
            name: "timezone",
            value: "UTC",
            apply_type: "dynamic",
            data_type: "string",
            description: "Sets the time zone for displaying and interpreting timestamps.",
        },
        CatalogParam {
            name: "log_statement",
            value: "none",
            apply_type: "dynamic",
            data_type: "string",
            description: "Sets the type of statements logged.",
        },
        CatalogParam {
            name: "shared_preload_libraries",
            value: "",
            apply_type: "static",
            data_type: "string",
            description: "Lists shared libraries to preload into the server.",
        },
    ];
    const MYSQL: &[CatalogParam] = &[
        CatalogParam {
            name: "character_set_server",
            value: "latin1",
            apply_type: "dynamic",
            data_type: "string",
            description: "The server's default character set.",
        },
        CatalogParam {
            name: "time_zone",
            value: "UTC",
            apply_type: "dynamic",
            data_type: "string",
            description: "The server's time zone.",
        },
        CatalogParam {
            name: "binlog_format",
            value: "ROW",
            apply_type: "dynamic",
            data_type: "string",
            description: "The binary logging format for replication.",
        },
        CatalogParam {
            name: "server_audit_logging",
            value: "0",
            apply_type: "dynamic",
            data_type: "boolean",
            description: "Enables or disables audit logging.",
        },
    ];
    if family.starts_with("aurora-postgresql") || family.starts_with("postgres") {
        POSTGRES
    } else {
        MYSQL
    }
}

fn group_to_value(pg: &DbClusterParameterGroup) -> Value {
    json!({
        "DBClusterParameterGroupName": pg.name,
        "DBClusterParameterGroupArn": pg.arn,
        "DBParameterGroupFamily": pg.family,
        "Description": pg.description,
    })
}

/// `CreateDBClusterParameterGroup` registers a new cluster parameter
/// group. It starts with the family's engine defaults; callers override
/// individual values later with `ModifyDBClusterParameterGroup`.
pub fn create_db_cluster_parameter_group(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBClusterParameterGroupName")?;
    let family = require_str(input, "DBParameterGroupFamily")?;
    let description = require_str(input, "Description")?;

    if state.cluster_parameter_groups.contains_key(name) {
        return Err(db_parameter_group_already_exists(name));
    }

    let arn = cluster_parameter_group_arn(&ctx.partition, &ctx.region, &ctx.account_id, name);
    let pg = DbClusterParameterGroup {
        name: name.to_string(),
        arn,
        family: family.to_string(),
        description: description.to_string(),
        parameters: std::collections::HashMap::new(),
    };

    let result = group_to_value(&pg);
    state.cluster_parameter_groups.insert(name.to_string(), pg);

    Ok(json!({ "DBClusterParameterGroup": result }))
}

/// `DescribeDBClusterParameterGroups` lists cluster parameter groups,
/// optionally filtered by name.
pub fn describe_db_cluster_parameter_groups(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_name = opt_str(input, "DBClusterParameterGroupName");

    if let Some(name) = filter_name {
        let pg = state
            .cluster_parameter_groups
            .get(name)
            .ok_or_else(|| db_parameter_group_not_found(name))?;
        let items = vec![group_to_value(&pg)];
        return Ok(json!({
            "DBClusterParameterGroups": { "DBClusterParameterGroup": items },
            "Marker": null,
        }));
    }

    let mut items: Vec<(String, Value)> = state
        .cluster_parameter_groups
        .iter()
        .map(|e| (e.key().clone(), group_to_value(e.value())))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let groups: Vec<Value> = items.into_iter().map(|(_, v)| v).collect();

    Ok(json!({
        "DBClusterParameterGroups": { "DBClusterParameterGroup": groups },
        "Marker": null,
    }))
}

/// `DeleteDBClusterParameterGroup` removes a cluster parameter group.
pub fn delete_db_cluster_parameter_group(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBClusterParameterGroupName")?;

    if !state.cluster_parameter_groups.contains_key(name) {
        return Err(db_parameter_group_not_found(name));
    }

    state.cluster_parameter_groups.remove(name);
    Ok(json!({}))
}

/// `DescribeDBClusterParameters` returns the resolved parameter list for
/// a group: the family's engine defaults with any caller overrides
/// applied. Overridden parameters report a `user` source. An optional
/// `Source` filter narrows the result to `user` or `engine-default`.
pub fn describe_db_cluster_parameters(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBClusterParameterGroupName")?;
    let source_filter = opt_str(input, "Source");

    let pg = state
        .cluster_parameter_groups
        .get(name)
        .ok_or_else(|| db_parameter_group_not_found(name))?;

    let mut params: Vec<Value> = Vec::new();
    for entry in default_cluster_parameters(&pg.family) {
        let overridden = pg.parameters.get(entry.name);
        let source = if overridden.is_some() {
            "user"
        } else {
            "engine-default"
        };
        if source_filter.is_some_and(|s| s != source) {
            continue;
        }
        params.push(json!({
            "ParameterName": entry.name,
            "ParameterValue": overridden.map(String::as_str).unwrap_or(entry.value),
            "Description": entry.description,
            "Source": source,
            "ApplyType": entry.apply_type,
            "DataType": entry.data_type,
            "IsModifiable": true,
            "ApplyMethod": "pending-reboot",
        }));
    }

    // Surface caller-set parameters that are not part of the catalog so
    // a round-trip of an unknown parameter still describes back.
    let catalog: Vec<&str> = default_cluster_parameters(&pg.family)
        .iter()
        .map(|p| p.name)
        .collect();
    for (key, value) in pg.parameters.iter() {
        if catalog.contains(&key.as_str()) {
            continue;
        }
        if source_filter.is_some_and(|s| s != "user") {
            continue;
        }
        params.push(json!({
            "ParameterName": key,
            "ParameterValue": value,
            "Source": "user",
            "ApplyType": "dynamic",
            "DataType": "string",
            "IsModifiable": true,
            "ApplyMethod": "pending-reboot",
        }));
    }
    params.sort_by(|a, b| {
        a["ParameterName"]
            .as_str()
            .unwrap_or("")
            .cmp(b["ParameterName"].as_str().unwrap_or(""))
    });

    Ok(json!({
        "Parameters": { "Parameter": params },
        "Marker": null,
    }))
}

/// `ModifyDBClusterParameterGroup` overrides one or more parameter
/// values in a group.
pub fn modify_db_cluster_parameter_group(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBClusterParameterGroupName")?;

    let mut pg = state
        .cluster_parameter_groups
        .get_mut(name)
        .ok_or_else(|| db_parameter_group_not_found(name))?;

    if let Some(params) = input["Parameters"].as_array() {
        for param in params {
            let Some(param_name) = param.get("ParameterName").and_then(|v| v.as_str()) else {
                continue;
            };
            let value = param
                .get("ParameterValue")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            pg.parameters
                .insert(param_name.to_string(), value.to_string());
        }
    }

    Ok(json!({ "DBClusterParameterGroupName": name }))
}

/// `ResetDBClusterParameterGroup` clears caller overrides, returning the
/// affected parameters to their engine defaults. With
/// `ResetAllParameters=true` every override is dropped; otherwise only
/// the named parameters are reset.
pub fn reset_db_cluster_parameter_group(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = require_str(input, "DBClusterParameterGroupName")?;

    let mut pg = state
        .cluster_parameter_groups
        .get_mut(name)
        .ok_or_else(|| db_parameter_group_not_found(name))?;

    let reset_all = input
        .get("ResetAllParameters")
        .and_then(super::coerce_bool)
        .unwrap_or(false);

    if reset_all {
        pg.parameters.clear();
    } else if let Some(params) = input["Parameters"].as_array() {
        for param in params {
            if let Some(param_name) = param.get("ParameterName").and_then(|v| v.as_str()) {
                pg.parameters.remove(param_name);
            }
        }
    }

    Ok(json!({ "DBClusterParameterGroupName": name }))
}

#[cfg(test)]
mod cluster_parameter_group_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn create_group(state: &RdsState, name: &str, family: &str) {
        create_db_cluster_parameter_group(
            state,
            &json!({
                "DBClusterParameterGroupName": name,
                "DBParameterGroupFamily": family,
                "Description": "test group",
            }),
            &ctx(),
        )
        .unwrap();
    }

    fn parameters(state: &RdsState, name: &str, source: Option<&str>) -> Vec<Value> {
        let mut input = json!({ "DBClusterParameterGroupName": name });
        if let Some(s) = source {
            input["Source"] = json!(s);
        }
        let resp = describe_db_cluster_parameters(state, &input, &ctx()).unwrap();
        resp["Parameters"]["Parameter"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn create_and_describe_group() {
        let state = RdsState::default();
        create_group(&state, "pg-params", "aurora-postgresql16");
        let resp = describe_db_cluster_parameter_groups(
            &state,
            &json!({ "DBClusterParameterGroupName": "pg-params" }),
            &ctx(),
        )
        .unwrap();
        let group = &resp["DBClusterParameterGroups"]["DBClusterParameterGroup"][0];
        assert_eq!(group["DBClusterParameterGroupName"], "pg-params");
        assert_eq!(group["DBParameterGroupFamily"], "aurora-postgresql16");
        assert!(
            group["DBClusterParameterGroupArn"]
                .as_str()
                .unwrap()
                .contains(":cluster-pg:pg-params")
        );
    }

    #[test]
    fn duplicate_group_is_rejected() {
        let state = RdsState::default();
        create_group(&state, "pg-params", "aurora-mysql8.0");
        let err = create_db_cluster_parameter_group(
            &state,
            &json!({
                "DBClusterParameterGroupName": "pg-params",
                "DBParameterGroupFamily": "aurora-mysql8.0",
                "Description": "dup",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "DBParameterGroupAlreadyExists");
    }

    #[test]
    fn describe_parameters_returns_family_defaults() {
        let state = RdsState::default();
        create_group(&state, "pg-params", "aurora-postgresql16");
        let params = parameters(&state, "pg-params", None);
        assert!(params.iter().all(|p| p["Source"] == "engine-default"));
        assert!(params.iter().any(|p| p["ParameterName"] == "rds.force_ssl"));
    }

    #[test]
    fn modify_then_describe_marks_user_source() {
        let state = RdsState::default();
        create_group(&state, "pg-params", "aurora-postgresql16");
        modify_db_cluster_parameter_group(
            &state,
            &json!({
                "DBClusterParameterGroupName": "pg-params",
                "Parameters": [
                    { "ParameterName": "log_statement", "ParameterValue": "all" },
                ],
            }),
            &ctx(),
        )
        .unwrap();

        let user = parameters(&state, "pg-params", Some("user"));
        assert_eq!(user.len(), 1);
        assert_eq!(user[0]["ParameterName"], "log_statement");
        assert_eq!(user[0]["ParameterValue"], "all");
        assert_eq!(user[0]["Source"], "user");
    }

    #[test]
    fn reset_named_parameter_restores_default() {
        let state = RdsState::default();
        create_group(&state, "pg-params", "aurora-postgresql16");
        modify_db_cluster_parameter_group(
            &state,
            &json!({
                "DBClusterParameterGroupName": "pg-params",
                "Parameters": [{ "ParameterName": "log_statement", "ParameterValue": "all" }],
            }),
            &ctx(),
        )
        .unwrap();
        reset_db_cluster_parameter_group(
            &state,
            &json!({
                "DBClusterParameterGroupName": "pg-params",
                "Parameters": [{ "ParameterName": "log_statement" }],
            }),
            &ctx(),
        )
        .unwrap();
        assert!(parameters(&state, "pg-params", Some("user")).is_empty());
    }

    #[test]
    fn reset_all_clears_every_override() {
        let state = RdsState::default();
        create_group(&state, "my-params", "aurora-mysql8.0");
        modify_db_cluster_parameter_group(
            &state,
            &json!({
                "DBClusterParameterGroupName": "my-params",
                "Parameters": [
                    { "ParameterName": "time_zone", "ParameterValue": "US/Pacific" },
                    { "ParameterName": "binlog_format", "ParameterValue": "MIXED" },
                ],
            }),
            &ctx(),
        )
        .unwrap();
        reset_db_cluster_parameter_group(
            &state,
            &json!({
                "DBClusterParameterGroupName": "my-params",
                "ResetAllParameters": true,
            }),
            &ctx(),
        )
        .unwrap();
        assert!(parameters(&state, "my-params", Some("user")).is_empty());
    }

    #[test]
    fn operations_on_unknown_group_are_not_found() {
        let state = RdsState::default();
        let err = describe_db_cluster_parameters(
            &state,
            &json!({ "DBClusterParameterGroupName": "ghost" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "DBParameterGroupNotFound");

        let err = delete_db_cluster_parameter_group(
            &state,
            &json!({ "DBClusterParameterGroupName": "ghost" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "DBParameterGroupNotFound");
    }
}
