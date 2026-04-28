use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{BackupRecord, DynamoState, Table};

use super::{opt_str, require_str};

fn now_secs_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn create_backup(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    let backup_name = require_str(input, "BackupName")?;

    let table_arn = {
        let t = state.tables.get(table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "TableNotFoundException",
                format!("Table '{table_name}' not found"),
            )
        })?;
        t.arn.clone()
    };

    let now = now_secs_f64();
    let backup_arn = format!(
        "arn:aws:dynamodb:{}:{}:table/{}/backup/{:016.0}",
        ctx.region, ctx.account_id, table_name, now
    );

    let record = BackupRecord {
        backup_arn: backup_arn.clone(),
        backup_name: backup_name.to_string(),
        table_name: table_name.to_string(),
        table_arn,
        backup_status: "AVAILABLE".to_string(),
        backup_type: "USER".to_string(),
        backup_creation_date_time: now,
        backup_size_bytes: 0,
    };

    state.backups.insert(backup_arn.clone(), record);

    Ok(json!({
        "BackupDetails": {
            "BackupArn": backup_arn,
            "BackupName": backup_name,
            "BackupStatus": "AVAILABLE",
            "BackupType": "USER",
            "BackupCreationDateTime": now,
            "BackupSizeBytes": 0
        }
    }))
}

pub fn delete_backup(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let backup_arn = require_str(input, "BackupArn")?;

    let record = state.backups.remove(backup_arn).map(|(_, r)| r);

    let (name, table_name, table_arn, created) = if let Some(r) = record {
        (
            r.backup_name,
            r.table_name,
            r.table_arn,
            r.backup_creation_date_time,
        )
    } else {
        (
            "deleted-backup".to_string(),
            String::new(),
            String::new(),
            0.0,
        )
    };

    Ok(json!({
        "BackupDescription": {
            "BackupDetails": {
                "BackupArn": backup_arn,
                "BackupName": name,
                "BackupStatus": "DELETED",
                "BackupType": "USER",
                "BackupCreationDateTime": created,
                "BackupSizeBytes": 0
            },
            "SourceTableDetails": {
                "TableName": table_name,
                "TableArn": table_arn,
                "TableId": "00000000-0000-0000-0000-000000000000",
                "TableSizeBytes": 0,
                "ItemCount": 0,
                "KeySchema": [],
                "TableCreationDateTime": created,
                "BillingMode": "PAY_PER_REQUEST"
            }
        }
    }))
}

pub fn describe_backup(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let backup_arn = require_str(input, "BackupArn")?;

    let record = state.backups.get(backup_arn).ok_or_else(|| {
        AwsError::service_not_found(
            "BackupNotFoundException",
            format!("Backup not found: {backup_arn}"),
        )
    })?;

    Ok(json!({
        "BackupDescription": {
            "BackupDetails": {
                "BackupArn": record.backup_arn,
                "BackupName": record.backup_name,
                "BackupStatus": record.backup_status,
                "BackupType": record.backup_type,
                "BackupCreationDateTime": record.backup_creation_date_time,
                "BackupSizeBytes": record.backup_size_bytes
            },
            "SourceTableDetails": {
                "TableName": record.table_name,
                "TableArn": record.table_arn,
                "TableId": "00000000-0000-0000-0000-000000000000",
                "TableSizeBytes": 0,
                "ItemCount": 0,
                "KeySchema": [],
                "TableCreationDateTime": record.backup_creation_date_time,
                "BillingMode": "PAY_PER_REQUEST"
            }
        }
    }))
}

pub fn list_backups(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_filter = opt_str(input, "TableName");
    let limit = input.get("Limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;

    let mut summaries: Vec<Value> = state
        .backups
        .iter()
        .filter(|e| {
            table_filter
                .map(|t| e.value().table_name == t)
                .unwrap_or(true)
        })
        .map(|e| {
            let r = e.value();
            json!({
                "TableName": r.table_name,
                "TableArn": r.table_arn,
                "BackupArn": r.backup_arn,
                "BackupName": r.backup_name,
                "BackupStatus": r.backup_status,
                "BackupType": r.backup_type,
                "BackupCreationDateTime": r.backup_creation_date_time,
                "BackupSizeBytes": r.backup_size_bytes
            })
        })
        .collect();

    summaries.truncate(limit);

    Ok(json!({ "BackupSummaries": summaries }))
}

pub fn restore_table_from_backup(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let target_name = require_str(input, "TargetTableName")?;
    let backup_arn = require_str(input, "BackupArn")?;

    let backup = state.backups.get(backup_arn).ok_or_else(|| {
        AwsError::service_not_found(
            "BackupNotFoundException",
            format!("Backup not found: {backup_arn}"),
        )
    })?;

    if state.tables.contains_key(target_name) {
        return Err(AwsError::conflict(
            "TableAlreadyExistsException",
            format!("Table already exists: {target_name}"),
        ));
    }

    let source = state.tables.get(&backup.table_name);
    let now = now_secs_f64();
    let new_arn = format!(
        "arn:aws:dynamodb:{}:{}:table/{}",
        ctx.region, ctx.account_id, target_name
    );

    let new_table = if let Some(src) = source {
        Table {
            name: target_name.to_string(),
            arn: new_arn.clone(),
            key_schema: src.key_schema.clone(),
            attribute_definitions: src.attribute_definitions.clone(),
            billing_mode: src.billing_mode.clone(),
            status: "ACTIVE".to_string(),
            created_at: now,
            gsi: src.gsi.clone(),
            lsi: src.lsi.clone(),
            stream_enabled: false,
            stream_arn: None,
            stream_view_type: None,
            stream_records: Vec::new(),
            stream_sequence: 0,
            ttl: src.ttl.clone(),
            tags: src.tags.clone(),
        }
    } else {
        Table {
            name: target_name.to_string(),
            arn: new_arn.clone(),
            key_schema: Vec::new(),
            attribute_definitions: Vec::new(),
            billing_mode: "PAY_PER_REQUEST".to_string(),
            status: "ACTIVE".to_string(),
            created_at: now,
            gsi: Vec::new(),
            lsi: Vec::new(),
            stream_enabled: false,
            stream_arn: None,
            stream_view_type: None,
            stream_records: Vec::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: std::collections::HashMap::new(),
        }
    };

    // Restored tables start empty — actual point-in-time replay isn't
    // implemented (it never was; this op was always a stub).
    let desc = crate::operations::table::table_description(&new_table, 0);
    state.tables.insert(target_name.to_string(), new_table);

    Ok(json!({
        "TableDescription": desc
    }))
}

pub fn restore_table_to_point_in_time(
    state: &DynamoState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let target_name = require_str(input, "TargetTableName")?;
    let source_name = opt_str(input, "SourceTableName");
    let source_arn = opt_str(input, "SourceTableArn");

    let source_key = if let Some(name) = source_name {
        name.to_string()
    } else if let Some(arn) = source_arn {
        arn.rsplit('/').next().unwrap_or("").to_string()
    } else {
        return Err(AwsError::bad_request(
            "ValidationException",
            "Either SourceTableName or SourceTableArn is required",
        ));
    };

    let source = state.tables.get(&source_key).ok_or_else(|| {
        AwsError::service_not_found(
            "TableNotFoundException",
            format!("Source table not found: {source_key}"),
        )
    })?;

    if state.tables.contains_key(target_name) {
        return Err(AwsError::conflict(
            "TableAlreadyExistsException",
            format!("Table already exists: {target_name}"),
        ));
    }

    let now = now_secs_f64();
    let new_arn = format!(
        "arn:aws:dynamodb:{}:{}:table/{}",
        ctx.region, ctx.account_id, target_name
    );

    let new_table = Table {
        name: target_name.to_string(),
        arn: new_arn,
        key_schema: source.key_schema.clone(),
        attribute_definitions: source.attribute_definitions.clone(),
        billing_mode: source.billing_mode.clone(),
        status: "ACTIVE".to_string(),
        created_at: now,
        gsi: source.gsi.clone(),
        lsi: source.lsi.clone(),
        stream_enabled: false,
        stream_arn: None,
        stream_view_type: None,
        stream_records: Vec::new(),
        stream_sequence: 0,
        ttl: source.ttl.clone(),
        tags: source.tags.clone(),
    };

    // Restored tables start empty — actual point-in-time replay isn't
    // implemented (it never was; this op was always a stub).
    let desc = crate::operations::table::table_description(&new_table, 0);
    state.tables.insert(target_name.to_string(), new_table);

    Ok(json!({
        "TableDescription": desc
    }))
}

pub fn update_continuous_backups(
    state: &DynamoState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;

    if !state.tables.contains_key(table_name) {
        return Err(AwsError::service_not_found(
            "TableNotFoundException",
            format!("Table '{table_name}' not found"),
        ));
    }

    let pitr_enabled = input
        .get("PointInTimeRecoverySpecification")
        .and_then(|v| v.get("PointInTimeRecoveryEnabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    state
        .pitr_enabled
        .insert(table_name.to_string(), pitr_enabled);

    let pitr_status = if pitr_enabled { "ENABLED" } else { "DISABLED" };
    let now = now_secs_f64();

    let mut pitr_desc = json!({
        "PointInTimeRecoveryStatus": pitr_status
    });
    if pitr_enabled {
        pitr_desc["EarliestRestorableDateTime"] = json!(now);
        pitr_desc["LatestRestorableDateTime"] = json!(now);
    }

    Ok(json!({
        "ContinuousBackupsDescription": {
            "ContinuousBackupsStatus": "ENABLED",
            "PointInTimeRecoveryDescription": pitr_desc
        }
    }))
}
