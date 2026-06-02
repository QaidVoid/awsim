use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::db_cluster_not_found,
    ids::{cluster_custom_endpoint, cluster_endpoint_arn},
    state::{DbClusterEndpoint, RdsState},
};

use super::{opt_str, require_str};

fn endpoint_to_value(e: &DbClusterEndpoint) -> Value {
    let mut obj = json!({
        "DBClusterEndpointIdentifier": e.endpoint_identifier,
        "DBClusterEndpointArn": e.arn,
        "DBClusterIdentifier": e.cluster_identifier,
        "EndpointType": e.endpoint_type,
        "Endpoint": e.endpoint,
        "Status": e.status,
        "StaticMembers": { "member": [] },
        "ExcludedMembers": { "member": [] },
        "DBClusterEndpointResourceIdentifier": e.endpoint_identifier.clone(),
    });
    if let Some(ct) = &e.custom_endpoint_type {
        obj["CustomEndpointType"] = json!(ct);
    }
    obj
}

/// DescribeDBClusterEndpoints — return cluster endpoints.
pub fn describe_db_cluster_endpoints(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_cluster = opt_str(input, "DBClusterIdentifier");
    let filter_endpoint = opt_str(input, "DBClusterEndpointIdentifier");

    let mut endpoints: Vec<Value> = Vec::new();

    // If a specific endpoint identifier is requested
    if let Some(ep_id) = filter_endpoint {
        let found = state.cluster_endpoints.iter().find_map(|entry| {
            entry
                .value()
                .iter()
                .find(|ep| ep.endpoint_identifier == ep_id)
                .map(endpoint_to_value)
        });
        if let Some(ep) = found {
            endpoints.push(ep);
        } else {
            return Err(AwsError::not_found(
                "DBClusterEndpointNotFoundFault",
                format!("Cluster endpoint not found: {ep_id}"),
            ));
        }
    } else {
        // Filter by cluster or return all
        for entry in state.cluster_endpoints.iter() {
            if let Some(cluster_id) = filter_cluster
                && entry.key() != cluster_id
            {
                continue;
            }
            for ep in entry.value() {
                endpoints.push(endpoint_to_value(ep));
            }
        }

        // Also include the default writer/reader endpoints from existing clusters
        for cluster_entry in state.clusters.iter() {
            let cluster = cluster_entry.value();
            if let Some(cluster_id) = filter_cluster
                && cluster.identifier != cluster_id
            {
                continue;
            }
            // Default writer endpoint
            endpoints.push(json!({
                "DBClusterEndpointIdentifier": format!("{}-writer", cluster.identifier),
                "DBClusterIdentifier": cluster.identifier,
                "EndpointType": "WRITER",
                "Endpoint": cluster.endpoint,
                "Status": "available",
                "StaticMembers": { "member": [] },
                "ExcludedMembers": { "member": [] },
                "DBClusterEndpointResourceIdentifier": format!("{}-writer", cluster.identifier),
            }));
            // Default reader endpoint
            endpoints.push(json!({
                "DBClusterEndpointIdentifier": format!("{}-reader", cluster.identifier),
                "DBClusterIdentifier": cluster.identifier,
                "EndpointType": "READER",
                "Endpoint": cluster.reader_endpoint,
                "Status": "available",
                "StaticMembers": { "member": [] },
                "ExcludedMembers": { "member": [] },
                "DBClusterEndpointResourceIdentifier": format!("{}-reader", cluster.identifier),
            }));
        }
    }

    Ok(json!({
        "DBClusterEndpoints": { "DBClusterEndpointList": endpoints },
        "Marker": null,
    }))
}

/// CreateDBClusterEndpoint — create a custom cluster endpoint.
pub fn create_db_cluster_endpoint(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let cluster_id = require_str(input, "DBClusterIdentifier")?;
    let endpoint_id = require_str(input, "DBClusterEndpointIdentifier")?;
    let endpoint_type = require_str(input, "EndpointType")?;

    // Verify cluster exists
    if !state.clusters.contains_key(cluster_id) {
        return Err(db_cluster_not_found(cluster_id));
    }

    let custom_endpoint_type = opt_str(input, "StaticMembers").map(|_| "READER".to_string());

    let ep = DbClusterEndpoint {
        endpoint_identifier: endpoint_id.to_string(),
        arn: cluster_endpoint_arn(&ctx.partition, &ctx.region, &ctx.account_id, endpoint_id),
        cluster_identifier: cluster_id.to_string(),
        endpoint_type: endpoint_type.to_string(),
        endpoint: cluster_custom_endpoint(endpoint_id, &ctx.region),
        status: "available".to_string(),
        custom_endpoint_type,
    };

    let result = endpoint_to_value(&ep);

    state
        .cluster_endpoints
        .entry(cluster_id.to_string())
        .or_default()
        .push(ep);

    Ok(json!({ "DBClusterEndpoint": result }))
}

/// DeleteDBClusterEndpoint — delete a custom cluster endpoint.
pub fn delete_db_cluster_endpoint(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let endpoint_id = require_str(input, "DBClusterEndpointIdentifier")?;

    let mut found_ep: Option<DbClusterEndpoint> = None;

    for mut entry in state.cluster_endpoints.iter_mut() {
        let endpoints = entry.value_mut();
        if let Some(pos) = endpoints
            .iter()
            .position(|ep| ep.endpoint_identifier == endpoint_id)
        {
            found_ep = Some(endpoints.remove(pos));
            break;
        }
    }

    let ep = found_ep.ok_or_else(|| {
        AwsError::not_found(
            "DBClusterEndpointNotFoundFault",
            format!("Cluster endpoint not found: {endpoint_id}"),
        )
    })?;

    Ok(json!({ "DBClusterEndpoint": endpoint_to_value(&ep) }))
}
