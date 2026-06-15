use std::collections::HashMap;

use awsim_core::pagination::{cap_max_results, paginate};
use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::{
        db_cluster_not_found, db_cluster_snapshot_already_exists, db_cluster_snapshot_not_found,
    },
    ids::{cluster_snapshot_arn, now_iso8601},
    state::{DbClusterSnapshot, RdsState},
};

use super::{opt_str, require_str};

fn cluster_snapshot_to_value(s: &DbClusterSnapshot) -> Value {
    let mut obj = json!({
        "DBClusterSnapshotIdentifier": s.snapshot_identifier,
        "DBClusterSnapshotArn": s.arn,
        "DBClusterIdentifier": s.cluster_identifier,
        "Engine": s.engine,
        "EngineVersion": s.engine_version,
        "MasterUsername": s.master_username,
        "Status": s.status,
        "SnapshotCreateTime": s.created_at,
        "ClusterCreateTime": s.created_at,
        "SnapshotType": s.snapshot_type,
        "PercentProgress": 100,
        // Aurora reports an AllocatedStorage of 1 since cluster storage
        // is shared and managed rather than provisioned per snapshot.
        "AllocatedStorage": 1,
        "StorageEncrypted": s.kms_key_id.is_some(),
        "TagList": s
            .tags
            .iter()
            .map(|(k, v)| json!({ "Key": k, "Value": v }))
            .collect::<Vec<_>>(),
    });
    if let Some(ref k) = s.kms_key_id {
        obj["KmsKeyId"] = json!(k);
    }
    if let Some(ref r) = s.source_region {
        obj["SourceRegion"] = json!(r);
    }
    obj
}

/// Parse the `Tags` request member into a map of key to value.
fn request_tags(input: &Value) -> HashMap<String, String> {
    input["Tags"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let key = t.get("Key").and_then(|v| v.as_str())?;
                    let value = t.get("Value").and_then(|v| v.as_str()).unwrap_or("");
                    Some((key.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Collect the tags currently attached to a resource ARN.
fn tags_for_arn(state: &RdsState, arn: &str) -> HashMap<String, String> {
    state.tags.get(arn).map(|t| t.clone()).unwrap_or_default()
}

/// `CreateDBClusterSnapshot` captures a manual snapshot of an existing
/// Aurora cluster. The snapshot inherits the cluster's engine, version,
/// master username, and encryption key, and folds in the cluster's tags
/// plus any tags supplied on the request.
pub fn create_db_cluster_snapshot(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let snapshot_id = require_str(input, "DBClusterSnapshotIdentifier")?;
    let cluster_id = require_str(input, "DBClusterIdentifier")?;

    let cluster = state
        .clusters
        .get(cluster_id)
        .ok_or_else(|| db_cluster_not_found(cluster_id))?;

    if state.cluster_snapshots.contains_key(snapshot_id) {
        return Err(db_cluster_snapshot_already_exists(snapshot_id));
    }

    let mut tags = tags_for_arn(state, &cluster.arn);
    tags.extend(request_tags(input));

    let snapshot = DbClusterSnapshot {
        snapshot_identifier: snapshot_id.to_string(),
        arn: cluster_snapshot_arn(&ctx.partition, &ctx.region, &ctx.account_id, snapshot_id),
        cluster_identifier: cluster_id.to_string(),
        engine: cluster.engine.clone(),
        engine_version: cluster.engine_version.clone(),
        master_username: cluster.master_username.clone(),
        status: "available".to_string(),
        created_at: now_iso8601(),
        snapshot_type: "manual".to_string(),
        // The cluster does not track a storage encryption key yet, so a
        // freshly created snapshot is unencrypted unless a copy later
        // re-encrypts it with an explicit KmsKeyId.
        kms_key_id: None,
        tags,
        source_region: None,
    };
    drop(cluster);

    let result = cluster_snapshot_to_value(&snapshot);
    state
        .cluster_snapshots
        .insert(snapshot_id.to_string(), snapshot);

    Ok(json!({ "DBClusterSnapshot": result }))
}

/// `DescribeDBClusterSnapshots` lists cluster snapshots, optionally
/// filtered by snapshot identifier, source cluster, or snapshot type.
pub fn describe_db_cluster_snapshots(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_snapshot = opt_str(input, "DBClusterSnapshotIdentifier");
    let filter_cluster = opt_str(input, "DBClusterIdentifier");
    let filter_type = opt_str(input, "SnapshotType");

    if let Some(snap_id) = filter_snapshot {
        let snapshot = state
            .cluster_snapshots
            .get(snap_id)
            .ok_or_else(|| db_cluster_snapshot_not_found(snap_id))?;
        let items = vec![cluster_snapshot_to_value(&snapshot)];
        return Ok(json!({
            "DBClusterSnapshots": { "DBClusterSnapshot": items },
            "Marker": null,
        }));
    }

    let max_records = cap_max_results(input["MaxRecords"].as_i64(), 100, 100);
    let mut items: Vec<(String, Value)> = state
        .cluster_snapshots
        .iter()
        .filter(|e| {
            filter_cluster.is_none_or(|c| e.value().cluster_identifier == c)
                && filter_type.is_none_or(|t| e.value().snapshot_type == t)
        })
        .map(|e| (e.key().clone(), cluster_snapshot_to_value(e.value())))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let page = paginate(items, max_records, opt_str(input, "Marker"), |(k, _)| {
        k.clone()
    })?;
    let snapshots: Vec<Value> = page.items.into_iter().map(|(_, v)| v).collect();

    Ok(json!({
        "DBClusterSnapshots": { "DBClusterSnapshot": snapshots },
        "Marker": page.next_token,
    }))
}

/// `DeleteDBClusterSnapshot` removes a manual cluster snapshot.
pub fn delete_db_cluster_snapshot(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let snapshot_id = require_str(input, "DBClusterSnapshotIdentifier")?;

    let snapshot = state
        .cluster_snapshots
        .get(snapshot_id)
        .ok_or_else(|| db_cluster_snapshot_not_found(snapshot_id))?
        .clone();

    let result = cluster_snapshot_to_value(&snapshot);
    drop(snapshot);
    state.cluster_snapshots.remove(snapshot_id);

    Ok(json!({ "DBClusterSnapshot": result }))
}

/// `CopyDBClusterSnapshot` duplicates an existing cluster snapshot under
/// a new identifier, optionally re-encrypting it with a different KMS
/// key and recording the source region for cross-region copies.
pub fn copy_db_cluster_snapshot(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let source_id = require_str(input, "SourceDBClusterSnapshotIdentifier")?;
    let target_id = require_str(input, "TargetDBClusterSnapshotIdentifier")?;

    let source = state
        .cluster_snapshots
        .get(source_id)
        .ok_or_else(|| db_cluster_snapshot_not_found(source_id))?
        .clone();

    if state.cluster_snapshots.contains_key(target_id) {
        return Err(db_cluster_snapshot_already_exists(target_id));
    }

    let mut tags = source.tags.clone();
    tags.extend(request_tags(input));

    let copied = DbClusterSnapshot {
        snapshot_identifier: target_id.to_string(),
        arn: cluster_snapshot_arn(&ctx.partition, &ctx.region, &ctx.account_id, target_id),
        cluster_identifier: source.cluster_identifier.clone(),
        engine: source.engine.clone(),
        engine_version: source.engine_version.clone(),
        master_username: source.master_username.clone(),
        status: "available".to_string(),
        created_at: now_iso8601(),
        snapshot_type: "manual".to_string(),
        kms_key_id: opt_str(input, "KmsKeyId")
            .map(str::to_string)
            .or_else(|| source.kms_key_id.clone()),
        tags,
        source_region: opt_str(input, "SourceRegion").map(str::to_string),
    };

    let result = cluster_snapshot_to_value(&copied);
    state
        .cluster_snapshots
        .insert(target_id.to_string(), copied);

    Ok(json!({ "DBClusterSnapshot": result }))
}

#[cfg(test)]
mod cluster_snapshot_tests {
    use super::*;
    use crate::operations::clusters::create_db_cluster;
    use crate::operations::tags::add_tags_to_resource;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn create_cluster(state: &RdsState, id: &str) -> String {
        let resp = create_db_cluster(
            state,
            &json!({
                "DBClusterIdentifier": id,
                "Engine": "aurora-postgresql",
                "EngineVersion": "15.4",
                "MasterUsername": "clusteradmin",
                "MasterUserPassword": "secret123",
            }),
            &ctx(),
        )
        .unwrap();
        resp["DBCluster"]["DBClusterArn"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn create_snapshot_inherits_cluster_metadata() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");

        let resp = create_db_cluster_snapshot(
            &state,
            &json!({
                "DBClusterSnapshotIdentifier": "snap-1",
                "DBClusterIdentifier": "aurora-pg",
            }),
            &ctx(),
        )
        .unwrap();
        let snap = &resp["DBClusterSnapshot"];
        assert_eq!(snap["DBClusterIdentifier"], "aurora-pg");
        assert_eq!(snap["Engine"], "aurora-postgresql");
        assert_eq!(snap["EngineVersion"], "15.4");
        assert_eq!(snap["MasterUsername"], "clusteradmin");
        assert_eq!(snap["SnapshotType"], "manual");
        assert!(
            snap["DBClusterSnapshotArn"]
                .as_str()
                .unwrap()
                .contains(":cluster-snapshot:snap-1")
        );
    }

    #[test]
    fn create_snapshot_copies_cluster_tags() {
        let state = RdsState::default();
        let arn = create_cluster(&state, "aurora-pg");
        add_tags_to_resource(
            &state,
            &json!({
                "ResourceName": arn,
                "Tags": [{ "Key": "env", "Value": "prod" }],
            }),
        )
        .unwrap();

        let resp = create_db_cluster_snapshot(
            &state,
            &json!({
                "DBClusterSnapshotIdentifier": "snap-tags",
                "DBClusterIdentifier": "aurora-pg",
            }),
            &ctx(),
        )
        .unwrap();
        let tags = resp["DBClusterSnapshot"]["TagList"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0]["Key"], "env");
    }

    #[test]
    fn create_snapshot_rejects_unknown_cluster() {
        let state = RdsState::default();
        let err = create_db_cluster_snapshot(
            &state,
            &json!({
                "DBClusterSnapshotIdentifier": "snap-x",
                "DBClusterIdentifier": "missing",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "DBClusterNotFoundFault");
    }

    #[test]
    fn duplicate_snapshot_is_rejected() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");
        let input = json!({
            "DBClusterSnapshotIdentifier": "snap-dup",
            "DBClusterIdentifier": "aurora-pg",
        });
        create_db_cluster_snapshot(&state, &input, &ctx()).unwrap();
        let err = create_db_cluster_snapshot(&state, &input, &ctx()).unwrap_err();
        assert_eq!(err.code, "DBClusterSnapshotAlreadyExistsFault");
    }

    #[test]
    fn describe_filters_by_cluster_and_type() {
        let state = RdsState::default();
        create_cluster(&state, "cluster-a");
        create_cluster(&state, "cluster-b");
        create_db_cluster_snapshot(
            &state,
            &json!({ "DBClusterSnapshotIdentifier": "snap-a", "DBClusterIdentifier": "cluster-a" }),
            &ctx(),
        )
        .unwrap();
        create_db_cluster_snapshot(
            &state,
            &json!({ "DBClusterSnapshotIdentifier": "snap-b", "DBClusterIdentifier": "cluster-b" }),
            &ctx(),
        )
        .unwrap();

        let resp = describe_db_cluster_snapshots(
            &state,
            &json!({ "DBClusterIdentifier": "cluster-a" }),
            &ctx(),
        )
        .unwrap();
        let snaps = resp["DBClusterSnapshots"]["DBClusterSnapshot"]
            .as_array()
            .unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0]["DBClusterSnapshotIdentifier"], "snap-a");

        let manual =
            describe_db_cluster_snapshots(&state, &json!({ "SnapshotType": "automated" }), &ctx())
                .unwrap();
        assert!(
            manual["DBClusterSnapshots"]["DBClusterSnapshot"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn delete_then_describe_is_not_found() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");
        create_db_cluster_snapshot(
            &state,
            &json!({ "DBClusterSnapshotIdentifier": "snap-del", "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap();
        delete_db_cluster_snapshot(
            &state,
            &json!({ "DBClusterSnapshotIdentifier": "snap-del" }),
            &ctx(),
        )
        .unwrap();
        let err = describe_db_cluster_snapshots(
            &state,
            &json!({ "DBClusterSnapshotIdentifier": "snap-del" }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "DBClusterSnapshotNotFoundFault");
    }

    #[test]
    fn copy_carries_metadata_and_overrides_kms() {
        let state = RdsState::default();
        create_cluster(&state, "aurora-pg");
        create_db_cluster_snapshot(
            &state,
            &json!({ "DBClusterSnapshotIdentifier": "snap-src", "DBClusterIdentifier": "aurora-pg" }),
            &ctx(),
        )
        .unwrap();

        let copy = copy_db_cluster_snapshot(
            &state,
            &json!({
                "SourceDBClusterSnapshotIdentifier": "snap-src",
                "TargetDBClusterSnapshotIdentifier": "snap-dst",
                "SourceRegion": "us-west-2",
                "KmsKeyId": "alias/dst-key",
            }),
            &ctx(),
        )
        .unwrap();
        let snap = &copy["DBClusterSnapshot"];
        assert_eq!(snap["DBClusterIdentifier"], "aurora-pg");
        assert_eq!(snap["SourceRegion"], "us-west-2");
        assert_eq!(snap["KmsKeyId"], "alias/dst-key");
        assert_eq!(snap["StorageEncrypted"], true);
    }
}
