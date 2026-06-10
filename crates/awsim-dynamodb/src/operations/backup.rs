use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext, arn};
use serde_json::{Value, json};

use crate::sqlite_store::SqliteStore;
use crate::state::{BackupItem, BackupRecord, DynamoState, Table};

use super::{opt_str, require_str};

fn now_secs_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn create_backup(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    let backup_name = require_str(input, "BackupName")?;

    let (table_arn, schema_snapshot) = {
        let t = state.tables.get(table_name).ok_or_else(|| {
            AwsError::service_not_found(
                "TableNotFoundException",
                format!("Table '{table_name}' not found"),
            )
        })?;
        // Clone the full schema so a future restore can rebuild the
        // table even if the original gets deleted in between.
        (t.arn.clone(), Some(t.value().clone()))
    };

    // Snapshot every item via the same SqliteStore the CRUD path
    // uses. Cheap O(N) read; bounded by table size. We deliberately
    // serialise rather than streaming to S3 — `--data-dir` snapshots
    // pick up the full backup record including these items.
    let mut items: Vec<BackupItem> = Vec::new();
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        table_name,
        None,
        |pk, sk, attrs| {
            items.push(BackupItem {
                pk: pk.to_string(),
                sk: sk.to_string(),
                attrs,
            });
            Ok(true)
        },
    )?;
    let backup_size_bytes = serde_json::to_vec(&items)
        .map(|v| v.len() as u64)
        .unwrap_or(0);

    let now = now_secs_f64();
    let backup_arn = arn::build(
        ctx,
        "dynamodb",
        format!("table/{table_name}/backup/{now:016.0}"),
    );

    let record = BackupRecord {
        backup_arn: backup_arn.clone(),
        backup_name: backup_name.to_string(),
        table_name: table_name.to_string(),
        table_arn,
        backup_status: "AVAILABLE".to_string(),
        backup_type: "USER".to_string(),
        backup_creation_date_time: now,
        backup_size_bytes,
        schema_snapshot,
        items,
    };

    state.backups.insert(backup_arn.clone(), record);

    Ok(json!({
        "BackupDetails": {
            "BackupArn": backup_arn,
            "BackupName": backup_name,
            "BackupStatus": "AVAILABLE",
            "BackupType": "USER",
            "BackupCreationDateTime": now,
            "BackupSizeBytes": backup_size_bytes
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

    let r = record.ok_or_else(|| {
        AwsError::service_not_found(
            "com.amazon.coral.service#BackupNotFoundException",
            format!("Backup not found: {backup_arn}"),
        )
    })?;

    Ok(json!({
        "BackupDescription": {
            "BackupDetails": {
                "BackupArn": backup_arn,
                "BackupName": r.backup_name,
                "BackupStatus": "DELETED",
                "BackupType": "USER",
                "BackupCreationDateTime": r.backup_creation_date_time,
                "BackupSizeBytes": 0
            },
            "SourceTableDetails": {
                "TableName": r.table_name,
                "TableArn": r.table_arn,
                "TableId": "00000000-0000-0000-0000-000000000000",
                "TableSizeBytes": 0,
                "ItemCount": 0,
                "KeySchema": [],
                "TableCreationDateTime": r.backup_creation_date_time,
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
    let exclusive_start = opt_str(input, "ExclusiveStartBackupArn");

    // Pair each summary with its BackupArn and order by it, so pagination
    // has a stable cursor (state.backups is an unordered DashMap).
    let mut summaries: Vec<(String, Value)> = state
        .backups
        .iter()
        .filter(|e| {
            table_filter
                .map(|t| e.value().table_name == t)
                .unwrap_or(true)
        })
        .map(|e| {
            let r = e.value();
            let value = json!({
                "TableName": r.table_name,
                "TableArn": r.table_arn,
                "BackupArn": r.backup_arn,
                "BackupName": r.backup_name,
                "BackupStatus": r.backup_status,
                "BackupType": r.backup_type,
                "BackupCreationDateTime": r.backup_creation_date_time,
                "BackupSizeBytes": r.backup_size_bytes
            });
            (r.backup_arn.clone(), value)
        })
        .collect();
    summaries.sort_by(|a, b| a.0.cmp(&b.0));

    // Resume strictly after ExclusiveStartBackupArn.
    let start_idx = exclusive_start
        .and_then(|start| summaries.iter().position(|(arn, _)| arn == start))
        .map(|i| i + 1)
        .unwrap_or(0);

    let page: Vec<&(String, Value)> = summaries[start_idx..].iter().take(limit).collect();
    // Emit a continuation token only when more results remain.
    let last = if page.len() == limit && start_idx + limit < summaries.len() {
        page.last().map(|(arn, _)| arn.clone())
    } else {
        None
    };

    let mut result = json!({
        "BackupSummaries": page.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
    });
    if let Some(arn) = last {
        result["LastEvaluatedBackupArn"] = json!(arn);
    }
    Ok(result)
}

pub fn restore_table_from_backup(
    state: &DynamoState,
    sqlite: &SqliteStore,
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
        return Err(AwsError::bad_request(
            "TableAlreadyExistsException",
            format!("Table already exists: {target_name}"),
        ));
    }

    let now = now_secs_f64();
    let new_arn = arn::build(ctx, "dynamodb", format!("table/{target_name}"));

    // Schema preference order:
    //   1. Schema snapshot captured at backup time — survives even
    //      after the source table is deleted.
    //   2. Live source table (if backup pre-dates schema snapshots).
    //   3. Empty stub (last-ditch — backup is malformed).
    let new_table = if let Some(snap) = backup.schema_snapshot.as_ref() {
        let mut t = snap.clone();
        t.name = target_name.to_string();
        t.arn = new_arn.clone();
        t.created_at = now;
        // Streams + records start fresh on a restored table to match
        // AWS behaviour.
        t.stream_enabled = false;
        t.stream_arn = None;
        t.stream_view_type = None;
        t.stream_records = VecDeque::new();
        t.stream_sequence = 0;
        t
    } else if let Some(src) = state.tables.get(&backup.table_name) {
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
            stream_records: VecDeque::new(),
            stream_sequence: 0,
            ttl: src.ttl.clone(),
            tags: src.tags.clone(),
            deletion_protection_enabled: src.deletion_protection_enabled,
            sse: src.sse.clone(),
            read_capacity_units: src.read_capacity_units,
            write_capacity_units: src.write_capacity_units,
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
            stream_records: VecDeque::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: std::collections::HashMap::new(),
            deletion_protection_enabled: false,
            sse: Default::default(),
            read_capacity_units: 0,
            write_capacity_units: 0,
        }
    };

    // Mirror the schema to SQLite so item writes via the new table
    // have the right `(account, region, table)` namespace established.
    let schema_value = serde_json::to_value(&new_table)
        .map_err(|e| AwsError::internal(format!("DynamoDB schema serialize failed: {e}")))?;
    sqlite.put_table_schema(&ctx.account_id, &ctx.region, target_name, &schema_value)?;

    // Replay every captured item, recomputing each row's GSI key
    // columns from the destination table's schema so restored items
    // are immediately visible to GSI queries. We don't fall back to
    // empty slots even when the destination has no GSIs - the
    // computed slice is just all-None in that case, which is exactly
    // what `empty_gsi` would have been.
    for item in &backup.items {
        let dyn_item = crate::keys::storage_value_to_item(item.attrs.clone());
        let gsi_keys = match dyn_item
            .as_ref()
            .and_then(|i| crate::keys::extract_item_keys(&new_table, i))
        {
            Some(k) => k.gsi,
            None => Default::default(),
        };
        sqlite.put_item(
            &ctx.account_id,
            &ctx.region,
            target_name,
            &item.pk,
            &item.sk,
            &item.attrs,
            &gsi_keys,
        )?;
    }
    let restored_count = backup.items.len() as u64;

    let desc = crate::operations::table::table_description(&new_table, restored_count);
    state.tables.insert(target_name.to_string(), new_table);

    Ok(json!({
        "TableDescription": desc
    }))
}

pub fn restore_table_to_point_in_time(
    state: &DynamoState,
    sqlite: &SqliteStore,
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
        return Err(AwsError::bad_request(
            "TableAlreadyExistsException",
            format!("Table already exists: {target_name}"),
        ));
    }

    let now = now_secs_f64();
    let new_arn = arn::build(ctx, "dynamodb", format!("table/{target_name}"));

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
        stream_records: VecDeque::new(),
        stream_sequence: 0,
        ttl: source.ttl.clone(),
        tags: source.tags.clone(),
        deletion_protection_enabled: source.deletion_protection_enabled,
        sse: source.sse.clone(),
        read_capacity_units: source.read_capacity_units,
        write_capacity_units: source.write_capacity_units,
    };
    drop(source);

    // Mirror the schema to SQLite so the restored table's
    // `(account, region, table)` namespace exists before item writes.
    let schema_value = serde_json::to_value(&new_table)
        .map_err(|e| AwsError::internal(format!("DynamoDB schema serialize failed: {e}")))?;
    sqlite.put_table_schema(&ctx.account_id, &ctx.region, target_name, &schema_value)?;

    // Copy the source table's current items into the restored table. True
    // point-in-time recovery would replay a change log to a chosen instant;
    // without one, awsim restores the latest snapshot, which is the closest
    // faithful behavior. Collect rows first so the read iterator isn't held
    // across the writes.
    let mut rows: Vec<(String, String, Value)> = Vec::new();
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        &source_key,
        None,
        |pk, sk, attrs| {
            rows.push((pk.to_string(), sk.to_string(), attrs));
            Ok(true)
        },
    )?;
    let restored_count = rows.len() as u64;
    for (pk, sk, attrs) in rows {
        let gsi_keys = crate::keys::storage_value_to_item(attrs.clone())
            .and_then(|item| crate::keys::extract_item_keys(&new_table, &item))
            .map(|k| k.gsi)
            .unwrap_or_default();
        sqlite.put_item(
            &ctx.account_id,
            &ctx.region,
            target_name,
            &pk,
            &sk,
            &attrs,
            &gsi_keys,
        )?;
    }

    let desc = crate::operations::table::table_description(&new_table, restored_count);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::item::put_item;
    use crate::operations::query::query;
    use crate::operations::table::create_table;
    use crate::sqlite_store::SqliteStore;

    fn ctx() -> RequestContext {
        RequestContext::new("dynamodb", "us-east-1")
    }

    #[test]
    fn restore_preserves_gsi_keys_so_restored_items_show_up_in_index_queries() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        // Create a table with one GSI keyed on `tag`.
        create_table(
            &state,
            &sqlite,
            &json!({
                "TableName": "src",
                "KeySchema": [
                    { "AttributeName": "pk", "KeyType": "HASH" }
                ],
                "AttributeDefinitions": [
                    { "AttributeName": "pk",  "AttributeType": "S" },
                    { "AttributeName": "tag", "AttributeType": "S" }
                ],
                "BillingMode": "PAY_PER_REQUEST",
                "GlobalSecondaryIndexes": [{
                    "IndexName": "byTag",
                    "KeySchema": [{ "AttributeName": "tag", "KeyType": "HASH" }],
                    "Projection": { "ProjectionType": "ALL" }
                }]
            }),
            &c,
        )
        .unwrap();

        // Seed an item that materialises into the GSI.
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "src",
                "Item": {
                    "pk":  { "S": "p1" },
                    "tag": { "S": "shared" }
                }
            }),
            &c,
        )
        .unwrap();

        // Snapshot the table, then restore into a new name.
        create_backup(
            &state,
            &sqlite,
            &json!({ "TableName": "src", "BackupName": "snap" }),
            &c,
        )
        .unwrap();
        // Restore needs a BackupArn; pull it from the registry.
        let backup_arn = state
            .backups
            .iter()
            .next()
            .map(|e| e.value().backup_arn.clone())
            .expect("backup recorded");
        restore_table_from_backup(
            &state,
            &sqlite,
            &json!({
                "BackupArn": backup_arn,
                "TargetTableName": "dst"
            }),
            &c,
        )
        .unwrap();

        // The restored copy must still answer GSI queries; before this
        // fix the items came back without their gsi1_* columns set, so
        // a Query on the index returned zero rows.
        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "dst",
                "IndexName": "byTag",
                "KeyConditionExpression": "tag = :t",
                "ExpressionAttributeValues": { ":t": { "S": "shared" } }
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["Count"], json!(1));
    }

    #[test]
    fn restore_to_point_in_time_copies_current_items() {
        let state = DynamoState::default();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        create_table(
            &state,
            &sqlite,
            &json!({
                "TableName": "src",
                "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
                "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
                "BillingMode": "PAY_PER_REQUEST"
            }),
            &c,
        )
        .unwrap();
        for i in 0..3 {
            put_item(
                &state,
                &sqlite,
                &json!({ "TableName": "src", "Item": { "pk": { "S": format!("p{i}") } } }),
                &c,
            )
            .unwrap();
        }

        let resp = restore_table_to_point_in_time(
            &state,
            &sqlite,
            &json!({ "SourceTableName": "src", "TargetTableName": "dst" }),
            &c,
        )
        .unwrap();
        // The restored table reports the copied item count, not zero.
        assert_eq!(resp["TableDescription"]["ItemCount"], json!(3));

        // And the items are actually queryable on the restored table.
        let got = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "dst",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "p1" } }
            }),
            &c,
        )
        .unwrap();
        assert_eq!(got["Count"], json!(1));
    }

    fn backup_record(arn: &str) -> BackupRecord {
        BackupRecord {
            backup_arn: arn.to_string(),
            backup_name: "b".to_string(),
            table_name: "t".to_string(),
            table_arn: "arn:aws:dynamodb:us-east-1:000000000000:table/t".to_string(),
            backup_status: "AVAILABLE".to_string(),
            backup_type: "USER".to_string(),
            backup_creation_date_time: 0.0,
            backup_size_bytes: 0,
            schema_snapshot: None,
            items: vec![],
        }
    }

    #[test]
    fn list_backups_paginates_with_exclusive_start() {
        // More backups than the page size: every one must be reachable via
        // LastEvaluatedBackupArn, exactly once, with no silent truncation.
        let state = DynamoState::default();
        for i in 0..5 {
            let arn = format!("arn:aws:dynamodb:us-east-1:000000000000:table/t/backup/{i:03}");
            state.backups.insert(arn.clone(), backup_record(&arn));
        }
        let c = ctx();

        let mut seen = Vec::new();
        let mut start: Option<String> = None;
        for _ in 0..10 {
            let mut req = json!({ "Limit": 2 });
            if let Some(s) = &start {
                req["ExclusiveStartBackupArn"] = json!(s);
            }
            let resp = list_backups(&state, &req, &c).unwrap();
            for b in resp["BackupSummaries"].as_array().unwrap() {
                seen.push(b["BackupArn"].as_str().unwrap().to_string());
            }
            match resp.get("LastEvaluatedBackupArn") {
                Some(v) if !v.is_null() => start = Some(v.as_str().unwrap().to_string()),
                _ => break,
            }
        }

        assert_eq!(seen.len(), 5, "every backup listed once, no dup/loss/loop");
        let mut uniq = seen.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 5);
    }
}
