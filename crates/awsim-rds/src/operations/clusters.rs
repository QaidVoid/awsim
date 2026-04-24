use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{db_cluster_already_exists, db_cluster_not_found, invalid_parameter},
    ids::{
        cluster_arn, cluster_endpoint, cluster_reader_endpoint, default_engine_version, now_iso8601,
    },
    state::{DbCluster, RdsState},
};

use super::{opt_str, require_str};

fn cluster_to_value(c: &DbCluster) -> Value {
    json!({
        "DBClusterIdentifier": c.identifier,
        "DBClusterArn": c.arn,
        "Engine": c.engine,
        "EngineVersion": c.engine_version,
        "Status": c.status,
        "MasterUsername": c.master_username,
        "Endpoint": c.endpoint,
        "ReaderEndpoint": c.reader_endpoint,
        "DBClusterMembers": c.members.iter().map(|m| json!({
            "DBInstanceIdentifier": m,
            "IsClusterWriter": true,
        })).collect::<Vec<_>>(),
        "ClusterCreateTime": c.created_at,
    })
}

pub fn create_db_cluster(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;
    let engine = require_str(input, "Engine")?;
    let master_username = require_str(input, "MasterUsername")?;
    let _master_password = require_str(input, "MasterUserPassword")?;

    match engine {
        "aurora" | "aurora-mysql" | "aurora-postgresql" | "mysql" | "postgres" => {}
        _ => {
            return Err(invalid_parameter(format!(
                "Unknown engine for cluster: {engine}"
            )));
        }
    }

    if state.clusters.contains_key(identifier) {
        return Err(db_cluster_already_exists(identifier));
    }

    let engine_version = opt_str(input, "EngineVersion")
        .unwrap_or_else(|| default_engine_version(engine))
        .to_string();

    let arn = cluster_arn(&ctx.region, &ctx.account_id, identifier);
    let endpoint = cluster_endpoint(identifier, &ctx.region);
    let reader_endpoint = cluster_reader_endpoint(identifier, &ctx.region);

    let cluster = DbCluster {
        identifier: identifier.to_string(),
        arn: arn.clone(),
        engine: engine.to_string(),
        engine_version,
        status: "available".to_string(),
        master_username: master_username.to_string(),
        endpoint,
        reader_endpoint,
        members: vec![],
        created_at: now_iso8601(),
    };

    let result = cluster_to_value(&cluster);
    state.clusters.insert(identifier.to_string(), cluster);

    Ok(json!({ "DBCluster": result }))
}

pub fn delete_db_cluster(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let identifier = require_str(input, "DBClusterIdentifier")?;

    let cluster = state
        .clusters
        .get(identifier)
        .ok_or_else(|| db_cluster_not_found(identifier))?
        .clone();

    let result = cluster_to_value(&cluster);
    drop(cluster);
    state.clusters.remove(identifier);

    Ok(json!({ "DBCluster": result }))
}

pub fn describe_db_clusters(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_id = opt_str(input, "DBClusterIdentifier");

    if let Some(id) = filter_id {
        let cluster = state
            .clusters
            .get(id)
            .ok_or_else(|| db_cluster_not_found(id))?;
        let items = vec![cluster_to_value(&cluster)];
        return Ok(json!({
            "DBClusters": { "DBCluster": items },
            "Marker": null,
        }));
    }

    let items: Vec<Value> = state
        .clusters
        .iter()
        .map(|e| cluster_to_value(e.value()))
        .collect();

    Ok(json!({
        "DBClusters": { "DBCluster": items },
        "Marker": null,
    }))
}
