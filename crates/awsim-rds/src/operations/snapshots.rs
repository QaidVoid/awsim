use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::{
    error::db_instance_not_found,
    ids::{now_iso8601, snapshot_arn},
    state::{DbSnapshot, RdsState},
};

use super::{opt_str, require_str};

fn snapshot_to_value(s: &DbSnapshot) -> Value {
    let mut obj = json!({
        "DBSnapshotIdentifier": s.snapshot_identifier,
        "DBSnapshotArn": s.arn,
        "DBInstanceIdentifier": s.db_instance_identifier,
        "Engine": s.engine,
        "EngineVersion": s.engine_version,
        "AllocatedStorage": s.allocated_storage,
        "Status": s.status,
        "SnapshotCreateTime": s.created_at,
        "SnapshotType": "manual",
        "PercentProgress": 100,
        "Encrypted": s.kms_key_id.is_some(),
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

/// CreateDBSnapshot — create a snapshot from an existing instance.
pub fn create_db_snapshot(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let snapshot_id = require_str(input, "DBSnapshotIdentifier")?;
    let instance_id = require_str(input, "DBInstanceIdentifier")?;

    let instance = state
        .instances
        .get(instance_id)
        .ok_or_else(|| db_instance_not_found(instance_id))?;

    if state.snapshots.contains_key(snapshot_id) {
        return Err(AwsError::conflict(
            "DBSnapshotAlreadyExists",
            format!("DB snapshot already exists: {snapshot_id}"),
        ));
    }

    // CopyTagsToSnapshot honors the instance flag; AWS folds the
    // instance's tag set into the snapshot when enabled.
    let mut tags: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if instance.copy_tags_to_snapshot {
        for (resource_arn, t) in state
            .tags
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
        {
            if resource_arn == instance.arn {
                tags.extend(t);
            }
        }
    }
    let snapshot = DbSnapshot {
        snapshot_identifier: snapshot_id.to_string(),
        arn: snapshot_arn(&ctx.region, &ctx.account_id, snapshot_id),
        db_instance_identifier: instance_id.to_string(),
        engine: instance.engine.clone(),
        engine_version: instance.engine_version.clone(),
        allocated_storage: instance.allocated_storage,
        status: "available".to_string(),
        created_at: now_iso8601(),
        tags,
        kms_key_id: instance.kms_key_id.clone(),
        source_region: None,
    };

    let result = snapshot_to_value(&snapshot);
    drop(instance);
    state.snapshots.insert(snapshot_id.to_string(), snapshot);

    Ok(json!({ "DBSnapshot": result }))
}

/// DeleteDBSnapshot — delete a snapshot.
pub fn delete_db_snapshot(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let snapshot_id = require_str(input, "DBSnapshotIdentifier")?;

    let snapshot = state
        .snapshots
        .get(snapshot_id)
        .ok_or_else(|| {
            AwsError::not_found(
                "DBSnapshotNotFound",
                format!("DB snapshot not found: {snapshot_id}"),
            )
        })?
        .clone();

    let result = snapshot_to_value(&snapshot);
    drop(snapshot);
    state.snapshots.remove(snapshot_id);

    Ok(json!({ "DBSnapshot": result }))
}

/// DescribeDBSnapshots — list snapshots with optional filter.
pub fn describe_db_snapshots(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let filter_snapshot = opt_str(input, "DBSnapshotIdentifier");
    let filter_instance = opt_str(input, "DBInstanceIdentifier");

    if let Some(snap_id) = filter_snapshot {
        let snapshot = state.snapshots.get(snap_id).ok_or_else(|| {
            AwsError::not_found(
                "DBSnapshotNotFound",
                format!("DB snapshot not found: {snap_id}"),
            )
        })?;
        let items = vec![snapshot_to_value(&snapshot)];
        return Ok(json!({
            "DBSnapshots": { "DBSnapshot": items },
            "Marker": null,
        }));
    }

    let items: Vec<Value> = state
        .snapshots
        .iter()
        .filter(|e| {
            if let Some(inst_id) = filter_instance {
                e.value().db_instance_identifier == inst_id
            } else {
                true
            }
        })
        .map(|e| snapshot_to_value(e.value()))
        .collect();

    Ok(json!({
        "DBSnapshots": { "DBSnapshot": items },
        "Marker": null,
    }))
}

/// CopyDBSnapshot — copy snapshot metadata (stub).
pub fn copy_db_snapshot(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let source_snapshot_id = require_str(input, "SourceDBSnapshotIdentifier")?;
    let target_snapshot_id = require_str(input, "TargetDBSnapshotIdentifier")?;

    let source = state
        .snapshots
        .get(source_snapshot_id)
        .ok_or_else(|| {
            AwsError::not_found(
                "DBSnapshotNotFound",
                format!("Source DB snapshot not found: {source_snapshot_id}"),
            )
        })?
        .clone();

    if state.snapshots.contains_key(target_snapshot_id) {
        return Err(AwsError::conflict(
            "DBSnapshotAlreadyExists",
            format!("DB snapshot already exists: {target_snapshot_id}"),
        ));
    }

    let copied = DbSnapshot {
        snapshot_identifier: target_snapshot_id.to_string(),
        arn: snapshot_arn(&ctx.region, &ctx.account_id, target_snapshot_id),
        db_instance_identifier: source.db_instance_identifier.clone(),
        engine: source.engine.clone(),
        engine_version: source.engine_version.clone(),
        allocated_storage: source.allocated_storage,
        status: "available".to_string(),
        created_at: now_iso8601(),
        tags: source.tags.clone(),
        kms_key_id: opt_str(input, "KmsKeyId")
            .map(str::to_string)
            .or_else(|| source.kms_key_id.clone()),
        source_region: opt_str(input, "SourceRegion").map(str::to_string),
    };

    let result = snapshot_to_value(&copied);
    state
        .snapshots
        .insert(target_snapshot_id.to_string(), copied);

    Ok(json!({ "DBSnapshot": result }))
}

/// DescribeEventSubscriptions — stub returning empty list.
pub fn describe_event_subscriptions(_input: &Value) -> Result<Value, AwsError> {
    Ok(json!({
        "EventSubscriptionsList": { "EventSubscription": [] },
        "Marker": null,
    }))
}

/// DescribeDBLogFiles — stub returning empty list.
pub fn describe_db_log_files(input: &Value) -> Result<Value, AwsError> {
    let _instance_id = require_str(input, "DBInstanceIdentifier")?;
    Ok(json!({
        "DescribeDBLogFiles": { "DescribeDBLogFilesDetails": [] },
        "Marker": null,
    }))
}

#[cfg(test)]
mod copy_tags_to_snapshot_tests {
    use super::*;
    use crate::operations::instances::create_db_instance;
    use crate::operations::tags::add_tags_to_resource;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    fn create_instance(state: &RdsState, copy_flag: bool, kms: Option<&str>) -> String {
        let mut input = json!({
            "DBInstanceIdentifier": "prod-db",
            "DBInstanceClass": "db.t3.micro",
            "Engine": "postgres",
            "MasterUsername": "admin",
            "MasterUserPassword": "secret123",
            "CopyTagsToSnapshot": copy_flag,
        });
        if let Some(k) = kms {
            input["KmsKeyId"] = json!(k);
        }
        let resp = create_db_instance(state, &input, &ctx()).unwrap();
        resp["DBInstance"]["DBInstanceArn"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn copies_tags_and_kms_when_flag_set() {
        let state = RdsState::default();
        let arn = create_instance(&state, true, Some("alias/aws/rds"));

        let tag_input = json!({
            "ResourceName": arn,
            "Tags": [{ "Key": "env", "Value": "prod" }],
        });
        add_tags_to_resource(&state, &tag_input).unwrap();

        let snap_input = json!({
            "DBSnapshotIdentifier": "snap-1",
            "DBInstanceIdentifier": "prod-db",
        });
        let resp = create_db_snapshot(&state, &snap_input, &ctx()).unwrap();
        let tags = resp["DBSnapshot"]["TagList"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0]["Key"], "env");
        assert_eq!(tags[0]["Value"], "prod");
        assert_eq!(resp["DBSnapshot"]["KmsKeyId"], "alias/aws/rds");
        assert_eq!(resp["DBSnapshot"]["Encrypted"], true);
    }

    #[test]
    fn skips_tag_copy_when_flag_unset() {
        let state = RdsState::default();
        let arn = create_instance(&state, false, None);

        let tag_input = json!({
            "ResourceName": arn,
            "Tags": [{ "Key": "env", "Value": "prod" }],
        });
        add_tags_to_resource(&state, &tag_input).unwrap();

        let snap_input = json!({
            "DBSnapshotIdentifier": "snap-2",
            "DBInstanceIdentifier": "prod-db",
        });
        let resp = create_db_snapshot(&state, &snap_input, &ctx()).unwrap();
        let tags = resp["DBSnapshot"]["TagList"].as_array().unwrap();
        assert!(tags.is_empty());
        assert!(resp["DBSnapshot"].get("KmsKeyId").is_none());
        assert_eq!(resp["DBSnapshot"]["Encrypted"], false);
    }

    #[test]
    fn copy_snapshot_carries_kms_and_tags_forward() {
        let state = RdsState::default();
        let arn = create_instance(&state, true, Some("alias/aws/rds"));

        let tag_input = json!({
            "ResourceName": arn,
            "Tags": [{ "Key": "team", "Value": "data" }],
        });
        add_tags_to_resource(&state, &tag_input).unwrap();

        create_db_snapshot(
            &state,
            &json!({
                "DBSnapshotIdentifier": "snap-src",
                "DBInstanceIdentifier": "prod-db",
            }),
            &ctx(),
        )
        .unwrap();

        let copy = copy_db_snapshot(
            &state,
            &json!({
                "SourceDBSnapshotIdentifier": "snap-src",
                "TargetDBSnapshotIdentifier": "snap-dst",
            }),
            &ctx(),
        )
        .unwrap();
        let tags = copy["DBSnapshot"]["TagList"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(copy["DBSnapshot"]["KmsKeyId"], "alias/aws/rds");
    }
}
