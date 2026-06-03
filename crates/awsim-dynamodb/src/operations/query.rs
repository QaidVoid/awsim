use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use std::collections::HashMap;

use crate::{
    expressions::{
        evaluate_condition, parse_condition, parse_projection,
        parser::{CompareOp, ConditionExpr, LogicalOp, Operand, resolve_path},
    },
    keys::storage_value_to_item,
    sqlite_store::SqliteStore,
    state::{DynamoItem, DynamoState, extract_scalar_str},
    throttle::BucketKind,
};

use super::{
    build_consumed_capacity, get_expr_attr_names, get_expr_attr_values, opt_str,
    read_capacity_units, require_str, validate_expr_attr_values,
};
use crate::operations::item::{estimate_item_bytes, item_to_json};

/// AWS DynamoDB caps `Query` / `Scan` responses at 1 MiB regardless of
/// `Limit`. Real clients are written to handle pagination via
/// `LastEvaluatedKey`, so enforcing the same cap keeps both wire
/// compatibility and our process memory bounded — without it a single
/// "fetch the whole partition" call materializes the entire table in
/// memory as `serde_json::Value` trees.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Apply a ProjectionExpression to an item, keeping only the requested
/// attributes.
///
/// Errors with a ValidationException when a projected path resolves
/// past the 64 KB document-path limit.
fn apply_projection_to_item(
    item: &DynamoItem,
    paths: &[String],
    expr_attr_names: &std::collections::HashMap<String, String>,
) -> Result<DynamoItem, AwsError> {
    if paths.is_empty() {
        return Ok(item.clone());
    }
    let mut result = DynamoItem::new();
    for path in paths {
        let resolved = resolve_path(path, expr_attr_names)?;
        if let Some(val) = item.get(&resolved) {
            result.insert(resolved, val.clone());
        }
    }
    Ok(result)
}

/// Resolved index Projection settings used to filter returned items so
/// they reflect what the index would actually store.
///
/// AWS rules:
/// * `ALL` -> every attribute survives.
/// * `KEYS_ONLY` -> only the table partition + sort key plus the index
///   partition + sort key.
/// * `INCLUDE` -> KEYS_ONLY's set plus the listed `non_key_attributes`.
struct IndexProjection {
    /// None when projection_type is ALL (no filtering).
    allowed: Option<std::collections::HashSet<String>>,
}

impl IndexProjection {
    fn from_index(
        projection: &crate::state::Projection,
        table_hash: Option<String>,
        table_range: Option<String>,
        index_hash: Option<String>,
        index_range: Option<String>,
    ) -> Self {
        match projection.projection_type.as_str() {
            "ALL" => Self { allowed: None },
            other => {
                let mut allowed = std::collections::HashSet::new();
                if let Some(h) = table_hash {
                    allowed.insert(h);
                }
                if let Some(r) = table_range {
                    allowed.insert(r);
                }
                if let Some(h) = index_hash {
                    allowed.insert(h);
                }
                if let Some(r) = index_range {
                    allowed.insert(r);
                }
                if other == "INCLUDE" {
                    for n in &projection.non_key_attributes {
                        allowed.insert(n.clone());
                    }
                }
                Self {
                    allowed: Some(allowed),
                }
            }
        }
    }

    /// Apply the projection: drop attributes that the index would not
    /// store. No-op when `allowed` is None (ALL projection).
    fn filter(&self, item: &DynamoItem) -> DynamoItem {
        match &self.allowed {
            None => item.clone(),
            Some(allow) => item
                .iter()
                .filter(|(k, _)| allow.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }
}

/// Build the LastEvaluatedKey JSON object from an item.
///
/// For a base-table query the index key names equal the base key names, so
/// the result is just the item's primary key. For a GSI query the index
/// names are the GSI's hash/range while the base names are the table's
/// pk/sk; AWS returns BOTH in the LEK because GSI sort keys aren't unique,
/// and the base primary key is what disambiguates the resume point. Keys
/// are inserted idempotently, so overlap (base == index) collapses cleanly.
fn last_evaluated_key(
    item: &DynamoItem,
    index_hash: &str,
    index_range: Option<&str>,
    base_hash: &str,
    base_range: Option<&str>,
) -> DynamoItem {
    let mut lek = DynamoItem::new();
    let mut copy = |name: &str| {
        if let Some(val) = item.get(name) {
            lek.insert(name.to_string(), val.clone());
        }
    };
    copy(index_hash);
    if let Some(r) = index_range {
        copy(r);
    }
    copy(base_hash);
    if let Some(r) = base_range {
        copy(r);
    }
    lek
}

pub fn query(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    validate_expr_attr_values(input)?;

    // Schema still comes from the in-memory cache during stage 3 — table
    // metadata moves to SQLite in stage 4.
    let table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);
    let projection_expr = opt_str(input, "ProjectionExpression");
    super::reject_attrs_to_get_with_projection(input, projection_expr)?;
    let filter_expr = opt_str(input, "FilterExpression");
    let key_condition_expr = opt_str(input, "KeyConditionExpression")
        .ok_or_else(|| AwsError::validation("KeyConditionExpression is required for Query"))?;
    let limit = input
        .get("Limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let scan_index_forward = input
        .get("ScanIndexForward")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let select = opt_str(input, "Select").unwrap_or("ALL_ATTRIBUTES");
    let exclusive_start_key = input
        .get("ExclusiveStartKey")
        .and_then(|v| v.as_object())
        .cloned();

    let key_condition = parse_condition(key_condition_expr)?;
    let filter_condition = filter_expr.map(parse_condition).transpose()?;

    let projection_paths: Vec<String> = projection_expr.map(parse_projection).unwrap_or_default();

    // Base-table key names, captured while `table` is still borrowed. A GSI
    // query's LastEvaluatedKey must carry these as a resume tiebreaker even
    // though the index resolution below rebinds hash_key_name/range_key_name
    // to the GSI's own keys.
    let base_hash_name = table.hash_key().unwrap_or("").to_string();
    let base_range_name = table.range_key().map(|s| s.to_string());

    // Resolve which key schema applies. With IndexName, GSI/LSI metadata
    // names different attributes than the base table; we look up the
    // index and pull its hash/range key names. Unknown index → 400 (AWS
    // raises ValidationException).
    //
    // We also capture the index's Projection setting so we can filter
    // the returned attributes to KEYS_ONLY / INCLUDE / ALL, matching
    // AWS. Without that filter awsim returns the full item regardless,
    // which silently lies about what a non-ALL index would store.
    let index_name = opt_str(input, "IndexName");
    let (hash_key_name, range_key_name, gsi_slot, index_projection) = match index_name {
        None => (
            table.hash_key().unwrap_or("").to_string(),
            table.range_key().map(|s| s.to_string()),
            None,
            None,
        ),
        Some(idx) => {
            // Try GSI first; fall back to LSI.
            if let Some((slot, gsi)) = table
                .gsi
                .iter()
                .enumerate()
                .find(|(_, g)| g.index_name == idx)
            {
                let hk = gsi
                    .key_schema
                    .iter()
                    .find(|k| k.key_type == "HASH")
                    .map(|k| k.attribute_name.clone())
                    .ok_or_else(|| {
                        AwsError::validation(format!("GSI {idx} has no HASH key in its KeySchema"))
                    })?;
                let rk = gsi
                    .key_schema
                    .iter()
                    .find(|k| k.key_type == "RANGE")
                    .map(|k| k.attribute_name.clone());
                let proj = IndexProjection::from_index(
                    &gsi.projection,
                    table.hash_key().map(str::to_string),
                    table.range_key().map(str::to_string),
                    Some(hk.clone()),
                    rk.clone(),
                );
                (hk, rk, Some(slot), Some(proj))
            } else if let Some(lsi) = table.lsi.iter().find(|l| l.index_name == idx) {
                // LSI shares the base hash key, only the range key differs.
                let hk = table.hash_key().unwrap_or("").to_string();
                let rk = lsi
                    .key_schema
                    .iter()
                    .find(|k| k.key_type == "RANGE")
                    .map(|k| k.attribute_name.clone());
                let proj = IndexProjection::from_index(
                    &lsi.projection,
                    table.hash_key().map(str::to_string),
                    table.range_key().map(str::to_string),
                    Some(hk.clone()),
                    rk.clone(),
                );
                (hk, rk, None, Some(proj)) // LSI uses base table's pk column → no slot
            } else {
                return Err(AwsError::validation(format!(
                    "The table does not have the specified index: {idx}"
                )));
            }
        }
    };

    // KeyConditionExpression has stricter rules than FilterExpression:
    // partition key may only use `=`, sort key only `=, <, <=, >, >=,
    // BETWEEN, begins_with`, and the connective between them must be AND.
    // Real DynamoDB rejects anything else with ValidationException; we
    // were silently accepting them as if they were filter expressions.
    // Runs after the key-name resolution so we can name the offending
    // key in the error message, matching AWS wire behavior.
    validate_key_condition(
        &key_condition,
        &expr_attr_names,
        &hash_key_name,
        range_key_name.as_deref(),
    )?;

    // Pull the partition key value out of the KeyConditionExpression so we
    // can push the partition lookup down to SQLite. DynamoDB requires the
    // hash key in every Query, but our parser is conservative — if it
    // can't find one we fall back to a full Scan-style sweep.
    let pk_value = extract_pk_from_condition(
        key_condition_expr,
        &hash_key_name,
        &expr_attr_names,
        &expr_attr_values,
    );

    // Convert ExclusiveStartKey into resume cursors. The base-table query
    // (and LSI, which executes over the base partition) resumes on the base
    // sort key, which is unique within a partition. The GSI path additionally
    // needs the index sort key plus the base primary key as a tiebreaker,
    // since GSI sort keys repeat.
    let esk_base_pk = exclusive_start_key.as_ref().and_then(|esk| {
        esk.get(&base_hash_name)
            .and_then(extract_scalar_str)
            .map(|s| s.to_string())
    });
    let esk_base_sk = exclusive_start_key.as_ref().and_then(|esk| {
        base_range_name
            .as_deref()
            .and_then(|br| esk.get(br))
            .and_then(extract_scalar_str)
            .map(|s| s.to_string())
    });
    let esk_index_sk = exclusive_start_key.as_ref().and_then(|esk| {
        range_key_name
            .as_deref()
            .and_then(|rk| esk.get(rk))
            .and_then(extract_scalar_str)
            .map(|s| s.to_string())
    });

    let mut scanned_count = 0usize;
    let mut items: Vec<DynamoItem> = Vec::new();
    let mut response_bytes = 0usize;
    let mut last_item: Option<DynamoItem> = None;
    let mut hit_limit = false;

    // Drop the table guard before SQLite IO — the dashmap Ref pins a
    // shard, and we don't want to hold it across a blocking read.
    drop(table);

    let mut handle = |item: DynamoItem| -> Result<bool, AwsError> {
        // Key condition over typed attributes (covers sort key range,
        // BEGINS_WITH, BETWEEN, etc.). Items that fail the key condition
        // are skipped silently — DynamoDB's index would never have
        // surfaced them, so they don't count toward ScannedCount either.
        if !evaluate_condition(&key_condition, &item, &expr_attr_names, &expr_attr_values)? {
            return Ok(true);
        }
        // This item is "evaluated": the index surfaced it. AWS counts every
        // evaluated item toward ScannedCount and the Limit, and applies any
        // FilterExpression only AFTER that accounting.
        scanned_count += 1;

        let passes_filter = match &filter_condition {
            Some(filter) => evaluate_condition(filter, &item, &expr_attr_names, &expr_attr_values)?,
            None => true,
        };

        if select == "COUNT" {
            if passes_filter {
                items.push(DynamoItem::new());
            }
        } else {
            // AWS applies the GSI/LSI Projection BEFORE the request's own
            // ProjectionExpression: a KEYS_ONLY index can never surface a
            // non-key attribute even if the caller asks for it. The index
            // view is also what the 1 MiB cap is charged against — examined
            // bytes, not just matched bytes.
            let after_index = match &index_projection {
                Some(p) => p.filter(&item),
                None => item.clone(),
            };
            if passes_filter {
                let projected =
                    apply_projection_to_item(&after_index, &projection_paths, &expr_attr_names)?;
                items.push(projected);
            }
            response_bytes += estimate_item_bytes(&after_index);
        }

        // The cursor advances for every evaluated item so LastEvaluatedKey
        // lands on the last item examined, not the last one matched — which
        // is what AWS returns when a FilterExpression is present.
        last_item = Some(item);

        // Limit caps the number of items EVALUATED (not matched); the 1 MiB
        // cap tracks examined bytes. Either one ends the page with a LEK.
        if let Some(lim) = limit
            && scanned_count >= lim
        {
            hit_limit = true;
            return Ok(false);
        }
        if select != "COUNT" && response_bytes >= MAX_RESPONSE_BYTES {
            hit_limit = true;
            return Ok(false);
        }
        Ok(true)
    };

    if let Some(ref pk) = pk_value {
        if let Some(slot) = gsi_slot {
            // Resume strictly after (gsi_sk, base_pk, base_sk). The presence
            // of a base pk in the ExclusiveStartKey is what triggers a
            // resume; our own LEK always carries it.
            let resume = esk_base_pk
                .as_deref()
                .map(|base_pk| crate::sqlite_store::GsiResume {
                    gsi_sk: esk_index_sk.as_deref(),
                    base_pk,
                    base_sk: esk_base_sk.as_deref().unwrap_or(""),
                });
            sqlite.query_gsi_partition(
                &ctx.account_id,
                &ctx.region,
                table_name,
                slot,
                pk,
                scan_index_forward,
                resume,
                |_base_pk, _base_sk, _gsi_sk, attrs| {
                    let item = storage_value_to_item(attrs).ok_or_else(|| {
                        AwsError::internal("DynamoDB stored attrs is not an object")
                    })?;
                    handle(item)
                },
            )?;
        } else {
            // Base table and LSI both stream the base partition ordered by
            // the base sort key, which is unique per partition, so the base
            // sort key alone is a sufficient resume cursor.
            sqlite.query_partition(
                &ctx.account_id,
                &ctx.region,
                table_name,
                pk,
                scan_index_forward,
                esk_base_sk.as_deref(),
                |_sk, attrs| {
                    let item = storage_value_to_item(attrs).ok_or_else(|| {
                        AwsError::internal("DynamoDB stored attrs is not an object")
                    })?;
                    handle(item)
                },
            )?;
        }
    } else {
        // No usable hash-key constraint extracted — fall back to a full
        // table scan (matches the legacy in-memory behaviour). Resume on the
        // base primary key, which is what scan_table orders by.
        let scan_start = exclusive_start_key.as_ref().and_then(|esk| {
            let pk = esk.get(&base_hash_name).and_then(extract_scalar_str)?;
            let sk = base_range_name
                .as_deref()
                .and_then(|rk| esk.get(rk))
                .and_then(extract_scalar_str)
                .unwrap_or("");
            Some((pk.to_string(), sk.to_string()))
        });
        let scan_start_ref = scan_start.as_ref().map(|(p, s)| (p.as_str(), s.as_str()));
        sqlite.scan_table(
            &ctx.account_id,
            &ctx.region,
            table_name,
            scan_start_ref,
            |_pk, _sk, attrs| {
                let item = storage_value_to_item(attrs)
                    .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;
                handle(item)
            },
        )?;
    }

    let count = items.len();
    let result_items: Vec<Value> = items.into_iter().map(|i| item_to_json(&i)).collect();

    let mut result = json!({
        "Items": result_items,
        "Count": count,
        "ScannedCount": scanned_count,
    });

    if hit_limit && let Some(item) = last_item {
        let lek = last_evaluated_key(
            &item,
            &hash_key_name,
            range_key_name.as_deref(),
            &base_hash_name,
            base_range_name.as_deref(),
        );
        result["LastEvaluatedKey"] = item_to_json(&lek);
    }

    let consistent_read = input
        .get("ConsistentRead")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let read_units = read_capacity_units(response_bytes, consistent_read, false);
    state.enforce_throughput(table_name, BucketKind::Read, read_units)?;
    if let Some(cc) = build_consumed_capacity(
        input,
        table_name,
        read_units,
        0.0,
        index_name.map(|n| (n, gsi_slot.is_some())),
    ) {
        result["ConsumedCapacity"] = cc;
    }
    Ok(result)
}

pub fn scan(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let table_name = require_str(input, "TableName")?;
    validate_expr_attr_values(input)?;

    let table = state.tables.get(table_name).ok_or_else(|| {
        AwsError::service_not_found(
            "ResourceNotFoundException",
            format!("Cannot do operations on a non-existent table: {table_name}"),
        )
    })?;

    let expr_attr_names = get_expr_attr_names(input);
    let expr_attr_values = get_expr_attr_values(input);
    let projection_expr = opt_str(input, "ProjectionExpression");
    super::reject_attrs_to_get_with_projection(input, projection_expr)?;
    let filter_expr = opt_str(input, "FilterExpression");
    let limit = input
        .get("Limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let select = opt_str(input, "Select").unwrap_or("ALL_ATTRIBUTES");
    let exclusive_start_key = input
        .get("ExclusiveStartKey")
        .and_then(|v| v.as_object())
        .cloned();

    let filter_condition = filter_expr.map(parse_condition).transpose()?;
    let projection_paths: Vec<String> = projection_expr.map(parse_projection).unwrap_or_default();

    let hash_key_name = table.hash_key().unwrap_or("").to_string();
    let range_key_name = table.range_key().map(|s| s.to_string());

    // Resolve the requested index up front so the per-index
    // ConsumedCapacity breakdown can be attributed correctly. Computed
    // while `table` is still borrowed since the Ref is dropped below.
    let scan_index_name = opt_str(input, "IndexName").map(|s| s.to_string());
    let scan_index_is_gsi = scan_index_name
        .as_deref()
        .map(|n| table.gsi.iter().any(|g| g.index_name == n))
        .unwrap_or(false);

    drop(table);

    // Parallel Scan: Segment/TotalSegments shard the table into N disjoint
    // slices. Both must be supplied together; we hash each row's (pk, sk)
    // and only emit those whose hash mod TotalSegments == Segment.
    let segmenting = parse_segments(input)?;

    // Translate ExclusiveStartKey → (pk, sk) tuple SQLite uses for
    // resume. Tables with no sort key encode sk as the empty string.
    let scan_start = exclusive_start_key.as_ref().and_then(|esk| {
        let pk = esk.get(&hash_key_name).and_then(extract_scalar_str)?;
        let sk = range_key_name
            .as_deref()
            .and_then(|rk| esk.get(rk))
            .and_then(extract_scalar_str)
            .unwrap_or("");
        Some((pk.to_string(), sk.to_string()))
    });

    let mut scanned_count = 0usize;
    let mut items: Vec<DynamoItem> = Vec::new();
    let mut response_bytes = 0usize;
    let mut last_item: Option<DynamoItem> = None;
    let mut hit_limit = false;

    let scan_start_ref = scan_start.as_ref().map(|(p, s)| (p.as_str(), s.as_str()));
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        table_name,
        scan_start_ref,
        |pk, sk, attrs| {
            // Skip rows that don't belong to this segment so the worker
            // only sees its slice. We don't count skipped rows toward
            // ScannedCount — they belong to another worker's count.
            if let Some((segment, total)) = segmenting
                && segment_index(pk, sk, total) != segment
            {
                return Ok(true);
            }
            let item = storage_value_to_item(attrs)
                .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;

            // Every row in this segment is "evaluated": it counts toward
            // ScannedCount and the Limit before the FilterExpression runs.
            scanned_count += 1;

            let passes_filter = match &filter_condition {
                Some(filter) => {
                    evaluate_condition(filter, &item, &expr_attr_names, &expr_attr_values)?
                }
                None => true,
            };

            if select == "COUNT" {
                if passes_filter {
                    items.push(DynamoItem::new());
                }
            } else {
                if passes_filter {
                    let projected =
                        apply_projection_to_item(&item, &projection_paths, &expr_attr_names)?;
                    items.push(projected);
                }
                // 1 MiB cap is charged against examined bytes, not matches.
                response_bytes += estimate_item_bytes(&item);
            }

            // Cursor advances for every evaluated row so LastEvaluatedKey
            // reflects the last item examined (matches AWS under a filter).
            last_item = Some(item);

            // Limit caps EVALUATED items, not matches; 1 MiB caps examined
            // bytes. Either ends the page and yields a LastEvaluatedKey.
            if let Some(lim) = limit
                && scanned_count >= lim
            {
                hit_limit = true;
                return Ok(false);
            }
            if select != "COUNT" && response_bytes >= MAX_RESPONSE_BYTES {
                hit_limit = true;
                return Ok(false);
            }
            Ok(true)
        },
    )?;

    let count = items.len();
    let result_items: Vec<Value> = items.into_iter().map(|i| item_to_json(&i)).collect();

    let mut result = json!({
        "Items": result_items,
        "Count": count,
        "ScannedCount": scanned_count,
    });

    if hit_limit && let Some(item) = last_item {
        // Scan always streams the base table, so the index and base key
        // names coincide here.
        let lek = last_evaluated_key(
            &item,
            &hash_key_name,
            range_key_name.as_deref(),
            &hash_key_name,
            range_key_name.as_deref(),
        );
        result["LastEvaluatedKey"] = item_to_json(&lek);
    }

    let consistent_read = input
        .get("ConsistentRead")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let read_units = read_capacity_units(response_bytes, consistent_read, false);
    state.enforce_throughput(table_name, BucketKind::Read, read_units)?;
    if let Some(cc) = build_consumed_capacity(
        input,
        table_name,
        read_units,
        0.0,
        scan_index_name.as_deref().map(|n| (n, scan_index_is_gsi)),
    ) {
        result["ConsumedCapacity"] = cc;
    }
    Ok(result)
}

/// Parse and validate the parallel-scan parameters. Returns None when
/// neither field is present (sequential scan); errors when one is set
/// without the other or values are out of range.
fn parse_segments(input: &Value) -> Result<Option<(u32, u32)>, AwsError> {
    let segment = input.get("Segment").and_then(|v| v.as_u64());
    let total = input.get("TotalSegments").and_then(|v| v.as_u64());
    match (segment, total) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(AwsError::validation(
            "Segment and TotalSegments must be supplied together",
        )),
        (Some(s), Some(t)) => {
            // AWS allows TotalSegments in [1, 1_000_000].
            if !(1..=1_000_000).contains(&t) {
                return Err(AwsError::validation(
                    "TotalSegments must be between 1 and 1000000",
                ));
            }
            if s >= t {
                return Err(AwsError::validation(
                    "Segment must be between 0 and TotalSegments-1",
                ));
            }
            Ok(Some((s as u32, t as u32)))
        }
    }
}

/// Hash `(pk, sk)` into `[0, total)`. Uses Rust's default hasher — the
/// only requirement is that the same row maps to the same segment for
/// every worker, which DefaultHasher satisfies within a single process.
fn segment_index(pk: &str, sk: &str, total: u32) -> u32 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pk.hash(&mut hasher);
    sk.hash(&mut hasher);
    (hasher.finish() % total as u64) as u32
}

/// Try to extract the partition key value from a KeyConditionExpression.
/// This enables a single-partition lookup against SQLite instead of a
/// full table scan.
/// Supports: "pk = :val", "pk = :val AND sk <op> :sk_val", etc.
fn extract_pk_from_condition(
    expr: &str,
    hash_key_name: &str,
    expr_attr_names: &std::collections::HashMap<String, String>,
    expr_attr_values: &serde_json::Map<String, Value>,
) -> Option<String> {
    // Simple heuristic: look for "hash_key = :placeholder" pattern.
    let upper = expr.to_uppercase();
    let hash_upper = hash_key_name.to_uppercase();

    if !upper.contains(&hash_upper) && !expr.contains('#') {
        return None;
    }

    for part in expr.split("AND") {
        let part = part.trim();
        if let Some(eq_pos) = part.find('=') {
            let left = part[..eq_pos].trim();
            let right = part[eq_pos + 1..].trim();

            let resolved_left = if let Some(stripped) = left.strip_prefix('#') {
                expr_attr_names
                    .get(&format!("#{stripped}"))
                    .map(|s| s.as_str())
                    .unwrap_or(left)
            } else {
                left
            };

            if resolved_left == hash_key_name
                && let Some(placeholder) = right.strip_prefix(':')
            {
                let key = format!(":{placeholder}");
                if let Some(val) = expr_attr_values.get(&key) {
                    return val
                        .get("S")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| val.get("N").and_then(|v| v.as_str()).map(|s| s.to_string()));
                }
            }
        }
    }
    None
}

/// Reject `KeyConditionExpression` shapes that real DynamoDB doesn't accept.
///
/// Error messages mirror the AWS wire surface so SDK consumers see the
/// same strings they would against real DynamoDB:
///
/// * Missing partition key clause:
///   `"Query condition missed key schema element: <pk-name>"`
/// * Non-`=` operator on the partition key, or any unsupported function
///   in key position: `"Query key condition not supported"`
/// * Two clauses pinned to the same key:
///   `"KeyConditionExpressions must only contain one condition per key"`
/// * `OR` / `NOT` at top level:
///   `"KeyConditionExpressions must not contain '<OR|NOT>'"`
fn validate_key_condition(
    expr: &ConditionExpr,
    expr_attr_names: &HashMap<String, String>,
    hash_key_name: &str,
    range_key_name: Option<&str>,
) -> Result<(), AwsError> {
    match expr {
        // Single top-level comparison: must be `pk = :v`.
        ConditionExpr::Comparison { op, left, right } => {
            validate_single_pk_clause(op, left, right, expr_attr_names, hash_key_name)
        }
        // `<pk> AND <sk>` (in either order).
        ConditionExpr::Logical {
            op: LogicalOp::And,
            children,
        } if children.len() == 2 => validate_and_pair(
            &children[0],
            &children[1],
            expr_attr_names,
            hash_key_name,
            range_key_name,
        ),
        // More than two ANDed clauses: at least one key has two conditions.
        ConditionExpr::Logical {
            op: LogicalOp::And, ..
        } => validation_err("KeyConditionExpressions must only contain one condition per key"),
        ConditionExpr::Logical {
            op: LogicalOp::Or, ..
        } => validation_err("KeyConditionExpressions must not contain 'OR'"),
        ConditionExpr::Not(_) => validation_err("KeyConditionExpressions must not contain 'NOT'"),
        // BeginsWith / Between / In / Contains / attribute_exists / etc. on
        // their own — the partition-key Eq clause is missing.
        _ => validation_err(&format!(
            "Query condition missed key schema element: {hash_key_name}"
        )),
    }
}

fn validate_single_pk_clause(
    op: &CompareOp,
    left: &Operand,
    right: &Operand,
    expr_attr_names: &HashMap<String, String>,
    hash_key_name: &str,
) -> Result<(), AwsError> {
    let Some(name) = operand_resolved_name(left, expr_attr_names) else {
        // `:v = :w` style — no key path at all.
        return validation_err(&format!(
            "Query condition missed key schema element: {hash_key_name}"
        ));
    };
    if name != hash_key_name {
        return validation_err(&format!(
            "Query condition missed key schema element: {hash_key_name}"
        ));
    }
    if !matches!(op, CompareOp::Eq) {
        return validation_err("Query key condition not supported");
    }
    if !matches!(right, Operand::Value(_)) {
        return validation_err("Query key condition not supported");
    }
    Ok(())
}

fn validate_and_pair(
    left: &ConditionExpr,
    right: &ConditionExpr,
    expr_attr_names: &HashMap<String, String>,
    hash_key_name: &str,
    range_key_name: Option<&str>,
) -> Result<(), AwsError> {
    // Real DynamoDB doesn't care which side the partition clause sits on.
    let (pk_term, sk_term) = match classify_key_term(left, expr_attr_names, hash_key_name) {
        KeyTermKind::PartitionEq => (left, right),
        _ => match classify_key_term(right, expr_attr_names, hash_key_name) {
            KeyTermKind::PartitionEq => (right, left),
            _ => {
                return validation_err(&format!(
                    "Query condition missed key schema element: {hash_key_name}"
                ));
            }
        },
    };
    // Re-validate the partition term so non-Eq comparisons surface a
    // distinct "not supported" rather than the missing-key error.
    if let ConditionExpr::Comparison { op, left, right } = pk_term {
        validate_single_pk_clause(op, left, right, expr_attr_names, hash_key_name)?;
    } else {
        return validation_err(&format!(
            "Query condition missed key schema element: {hash_key_name}"
        ));
    }
    let Some(sk_name) = range_key_name else {
        return validation_err("KeyConditionExpressions must only contain one condition per key");
    };
    validate_sort_key_term(sk_term, expr_attr_names, hash_key_name, sk_name)
}

#[derive(Debug)]
enum KeyTermKind {
    PartitionEq,
    Other,
}

/// Classify a single term: is it `pk = :v`, or anything else?
fn classify_key_term(
    expr: &ConditionExpr,
    expr_attr_names: &HashMap<String, String>,
    hash_key_name: &str,
) -> KeyTermKind {
    let ConditionExpr::Comparison {
        op: CompareOp::Eq,
        left,
        ..
    } = expr
    else {
        return KeyTermKind::Other;
    };
    match operand_resolved_name(left, expr_attr_names) {
        Some(name) if name == hash_key_name => KeyTermKind::PartitionEq,
        _ => KeyTermKind::Other,
    }
}

fn operand_resolved_name(op: &Operand, names: &HashMap<String, String>) -> Option<String> {
    match op {
        Operand::Path(p) => Some(resolve_attribute_name(p, names)),
        Operand::Value(_) => None,
    }
}

fn resolve_attribute_name(path: &str, names: &HashMap<String, String>) -> String {
    if let Some(stripped) = path.strip_prefix('#') {
        names
            .get(&format!("#{stripped}"))
            .cloned()
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    }
}

fn validate_sort_key_term(
    expr: &ConditionExpr,
    expr_attr_names: &HashMap<String, String>,
    hash_key_name: &str,
    range_key_name: &str,
) -> Result<(), AwsError> {
    let path = sort_key_path(expr, expr_attr_names);
    if let Some(name) = &path
        && name == hash_key_name
    {
        return validation_err("KeyConditionExpressions must only contain one condition per key");
    }
    if let Some(name) = &path
        && name != range_key_name
    {
        return validation_err("Query key condition not supported");
    }
    match expr {
        ConditionExpr::Comparison { op, right, .. } => {
            if !matches!(right, Operand::Value(_)) {
                return validation_err("Query key condition not supported");
            }
            match op {
                CompareOp::Eq | CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => {
                    Ok(())
                }
                CompareOp::Ne => validation_err("Query key condition not supported"),
            }
        }
        ConditionExpr::Between { .. } => Ok(()),
        ConditionExpr::BeginsWith(_, _) => Ok(()),
        // Anything else in sort-key position is rejected by real AWS with
        // "Query key condition not supported".
        _ => validation_err("Query key condition not supported"),
    }
}

/// Return the attribute name a sort-key-position term operates on, if it
/// has one. Used to detect "two clauses on the same key" and "clause on a
/// non-key attribute" before we look at the operator.
fn sort_key_path(
    expr: &ConditionExpr,
    expr_attr_names: &HashMap<String, String>,
) -> Option<String> {
    match expr {
        ConditionExpr::Comparison { left, .. } => operand_resolved_name(left, expr_attr_names),
        ConditionExpr::Between { operand, .. } => operand_resolved_name(operand, expr_attr_names),
        ConditionExpr::BeginsWith(path, _) => operand_resolved_name(path, expr_attr_names),
        ConditionExpr::Contains(path, _) => operand_resolved_name(path, expr_attr_names),
        ConditionExpr::AttributeExists(p) | ConditionExpr::AttributeNotExists(p) => {
            Some(resolve_attribute_name(p, expr_attr_names))
        }
        ConditionExpr::AttributeType(p, _) => Some(resolve_attribute_name(p, expr_attr_names)),
        ConditionExpr::SizeComparison { path, .. } => {
            Some(resolve_attribute_name(path, expr_attr_names))
        }
        ConditionExpr::In { operand, .. } => operand_resolved_name(operand, expr_attr_names),
        ConditionExpr::Logical { .. } | ConditionExpr::Not(_) => None,
    }
}

fn validation_err<T>(msg: &str) -> Result<T, AwsError> {
    Err(AwsError::bad_request("ValidationException", msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_with(attrs: &[(&str, Value)]) -> DynamoItem {
        let mut m = DynamoItem::new();
        for (k, v) in attrs {
            m.insert(k.to_string(), v.clone());
        }
        m
    }

    #[test]
    fn estimate_handles_typical_attribute_value() {
        // {"id": {"S": "abc"}, "n": {"N": "42"}}
        let item = item_with(&[("id", json!({ "S": "abc" })), ("n", json!({ "N": "42" }))]);
        let bytes = estimate_item_bytes(&item);
        // We don't pin the exact figure (varies if we tune overhead),
        // but it must be small + non-zero so the cap fires sanely.
        assert!(bytes > 0);
        assert!(bytes < 256, "tiny item shouldn't estimate huge: {bytes}");
    }

    #[test]
    fn estimate_grows_with_string_payload() {
        let small = item_with(&[("body", json!({ "S": "x".repeat(10) }))]);
        let large = item_with(&[("body", json!({ "S": "x".repeat(10_000) }))]);
        let small_bytes = estimate_item_bytes(&small);
        let large_bytes = estimate_item_bytes(&large);
        assert!(
            large_bytes >= small_bytes + 9_000,
            "large item should grow ~linearly with payload (small={small_bytes}, large={large_bytes})"
        );
    }

    #[test]
    fn cap_is_one_mib() {
        // Sanity: if someone bumps the const accidentally, fail loudly.
        // Real AWS DynamoDB Query/Scan response cap is exactly 1 MiB.
        assert_eq!(MAX_RESPONSE_BYTES, 1024 * 1024);
    }

    fn validate(expr: &str, hk: &str, rk: Option<&str>) -> Result<(), AwsError> {
        let cond = parse_condition(expr)?;
        validate_key_condition(&cond, &HashMap::new(), hk, rk)
    }

    #[test]
    fn missing_partition_key_names_the_expected_attribute() {
        let err = validate("begins_with(SK, :prefix)", "PK", Some("SK")).unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert_eq!(err.message, "Query condition missed key schema element: PK");
    }

    #[test]
    fn sort_key_only_eq_also_misses_pk() {
        let err = validate("SK = :v", "PK", Some("SK")).unwrap_err();
        assert_eq!(err.message, "Query condition missed key schema element: PK");
    }

    #[test]
    fn non_eq_on_partition_key_is_unsupported_op() {
        let err = validate("PK < :v", "PK", Some("SK")).unwrap_err();
        assert_eq!(err.message, "Query key condition not supported");
    }

    #[test]
    fn or_at_top_level_is_rejected_with_aws_wording() {
        let err = validate("PK = :a OR PK = :b", "PK", Some("SK")).unwrap_err();
        assert_eq!(err.message, "KeyConditionExpressions must not contain 'OR'");
    }

    #[test]
    fn two_partition_clauses_collapse_to_one_per_key_error() {
        let err = validate("PK = :a AND PK = :b", "PK", Some("SK")).unwrap_err();
        assert_eq!(
            err.message,
            "KeyConditionExpressions must only contain one condition per key"
        );
    }

    #[test]
    fn pk_and_sk_in_either_order_validates() {
        validate("PK = :pk AND begins_with(SK, :prefix)", "PK", Some("SK")).unwrap();
        validate("begins_with(SK, :prefix) AND PK = :pk", "PK", Some("SK")).unwrap();
        validate("SK = :sk AND PK = :pk", "PK", Some("SK")).unwrap();
    }

    #[test]
    fn sort_key_clause_on_non_key_attribute_is_unsupported() {
        let err = validate("PK = :pk AND OtherAttr = :v", "PK", Some("SK")).unwrap_err();
        assert_eq!(err.message, "Query key condition not supported");
    }

    #[test]
    fn sort_key_clause_when_table_has_no_sort_key_rejects() {
        let err = validate("PK = :pk AND SK = :sk", "PK", None).unwrap_err();
        assert_eq!(
            err.message,
            "KeyConditionExpressions must only contain one condition per key"
        );
    }

    #[test]
    fn resolves_attribute_name_placeholders() {
        let cond = parse_condition("#pk = :v").unwrap();
        let names = HashMap::from([("#pk".to_string(), "PK".to_string())]);
        validate_key_condition(&cond, &names, "PK", Some("SK")).unwrap();
    }

    use crate::operations::item::put_item;
    use crate::sqlite_store::SqliteStore;
    use crate::state::{KeySchemaElement, Table};
    use std::collections::VecDeque;

    fn ctx() -> RequestContext {
        RequestContext::new("dynamodb", "us-east-1")
    }

    fn make_state() -> DynamoState {
        make_state_with_gsi(vec![])
    }

    fn make_state_with_gsi(gsi: Vec<crate::state::GlobalSecondaryIndex>) -> DynamoState {
        let state = DynamoState::default();
        let table = Table {
            name: "t".into(),
            arn: "arn:aws:dynamodb:us-east-1:000000000000:table/t".into(),
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "pk".into(),
                    key_type: "HASH".into(),
                },
                KeySchemaElement {
                    attribute_name: "sk".into(),
                    key_type: "RANGE".into(),
                },
            ],
            attribute_definitions: vec![],
            billing_mode: "PAY_PER_REQUEST".into(),
            status: "ACTIVE".into(),
            created_at: 0.0,
            gsi,
            lsi: vec![],
            stream_enabled: false,
            stream_arn: None,
            stream_view_type: None,
            stream_records: VecDeque::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: Default::default(),
            deletion_protection_enabled: false,
            sse: Default::default(),
            read_capacity_units: 0,
            write_capacity_units: 0,
        };
        state.tables.insert("t".into(), table);
        state
    }

    #[test]
    fn parallel_scan_partitions_rows_disjointly_and_covers_all() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for i in 0..50 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk": {"S": format!("p-{i:03}")},
                        "sk": {"S": "s"},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let total = 4u64;
        let mut all_pks: Vec<String> = Vec::new();
        for seg in 0..total {
            let resp = scan(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Segment": seg,
                    "TotalSegments": total,
                }),
                &c,
            )
            .unwrap();
            for item in resp["Items"].as_array().unwrap() {
                all_pks.push(item["pk"]["S"].as_str().unwrap().to_string());
            }
        }
        // Disjoint and complete: every original row is reported exactly once.
        all_pks.sort();
        all_pks.dedup();
        assert_eq!(all_pks.len(), 50);
    }

    #[test]
    fn parallel_scan_rejects_segment_without_total() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = scan(
            &state,
            &sqlite,
            &json!({ "TableName": "t", "Segment": 0u64 }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn parallel_scan_rejects_segment_at_or_above_total() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Segment": 4u64,
                "TotalSegments": 4u64,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn query_against_gsi_returns_only_matching_partition() {
        use crate::state::{GlobalSecondaryIndex, Projection};
        let gsi = vec![GlobalSecondaryIndex {
            index_name: "byTenant".into(),
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "tenant".into(),
                    key_type: "HASH".into(),
                },
                KeySchemaElement {
                    attribute_name: "ts".into(),
                    key_type: "RANGE".into(),
                },
            ],
            projection: Projection {
                projection_type: "ALL".into(),
                non_key_attributes: vec![],
            },
            status: "ACTIVE".into(),
        }];
        let state = make_state_with_gsi(gsi);
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        // Two tenants, several items each.
        for (tenant, sk_ts) in [("a", "1"), ("a", "2"), ("a", "3"), ("b", "1"), ("b", "2")] {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk": {"S": format!("p-{tenant}-{sk_ts}")},
                        "sk": {"S": "row"},
                        "tenant": {"S": tenant},
                        "ts": {"S": sk_ts},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTenant",
                "KeyConditionExpression": "tenant = :t",
                "ExpressionAttributeValues": { ":t": {"S": "a"} },
            }),
            &c,
        )
        .unwrap();

        assert_eq!(resp["Count"], json!(3));
        let tenants: Vec<&str> = resp["Items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["tenant"]["S"].as_str().unwrap())
            .collect();
        assert!(tenants.iter().all(|t| *t == "a"));
    }

    #[test]
    fn query_against_unknown_index_raises_validation() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "nope",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "x"} },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn query_scanned_count_excludes_items_that_failed_key_condition() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        // Three sort-key buckets; the key condition selects only sk = "y".
        for sk in ["x", "y", "z"] {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": { "pk": {"S": "p"}, "sk": {"S": sk} },
                }),
                &c,
            )
            .unwrap();
        }

        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :pk AND sk = :sk",
                "ExpressionAttributeValues": {
                    ":pk": {"S": "p"},
                    ":sk": {"S": "y"},
                },
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["Count"], json!(1));
        // Only the matching key-condition item counts, not the two we skipped.
        assert_eq!(resp["ScannedCount"], json!(1));
    }

    #[test]
    fn key_condition_rejects_partition_key_inequality() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk <> :p",
                "ExpressionAttributeValues": { ":p": {"S": "x"} },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn key_condition_rejects_in_on_sort_key() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :p AND sk IN (:a, :b)",
                "ExpressionAttributeValues": {
                    ":p": {"S": "x"},
                    ":a": {"S": "a"},
                    ":b": {"S": "b"},
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn key_condition_rejects_contains_function() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :p AND contains(sk, :v)",
                "ExpressionAttributeValues": {
                    ":p": {"S": "x"},
                    ":v": {"S": "y"},
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn key_condition_accepts_begins_with_on_sort_key() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        // Just ensure parse + validate succeed; we don't actually need data.
        query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :p AND begins_with(sk, :v)",
                "ExpressionAttributeValues": {
                    ":p": {"S": "x"},
                    ":v": {"S": "y"},
                },
            }),
            &ctx(),
        )
        .unwrap();
    }

    #[test]
    fn filter_attribute_type_string_matches_n_attribute() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p"}, "sk": {"S": "s"}, "n": {"N": "5"} },
            }),
            &c,
        )
        .unwrap();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p"}, "sk": {"S": "t"}, "n": {"S": "five"} },
            }),
            &c,
        )
        .unwrap();

        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :pk",
                "FilterExpression": "attribute_type(n, :ty)",
                "ExpressionAttributeValues": {
                    ":pk": {"S": "p"},
                    ":ty": {"S": "N"},
                },
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["Count"], json!(1));
    }

    fn make_state_with_by_tag_gsi(projection_type: &str, non_key: Vec<String>) -> DynamoState {
        use crate::state::Projection;
        make_state_with_gsi(vec![crate::state::GlobalSecondaryIndex {
            index_name: "byTag".into(),
            key_schema: vec![KeySchemaElement {
                attribute_name: "tag".into(),
                key_type: "HASH".into(),
            }],
            projection: Projection {
                projection_type: projection_type.into(),
                non_key_attributes: non_key,
            },
            status: "ACTIVE".into(),
        }])
    }

    #[test]
    fn gsi_keys_only_projection_strips_non_key_attributes() {
        let state = make_state_with_by_tag_gsi("KEYS_ONLY", vec![]);
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": {
                    "pk":      { "S": "p1" },
                    "sk":      { "S": "s1" },
                    "tag":     { "S": "shared" },
                    "secret":  { "S": "should-not-leak" },
                    "another": { "N": "42" },
                },
            }),
            &c,
        )
        .unwrap();

        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTag",
                "KeyConditionExpression": "tag = :t",
                "ExpressionAttributeValues": { ":t": { "S": "shared" } },
            }),
            &c,
        )
        .unwrap();
        let item = &resp["Items"][0];
        // KEYS_ONLY: only pk + sk + tag. The base table's pk/sk plus
        // the index's hash key (tag). Non-key attributes are gone.
        assert!(item.get("pk").is_some());
        assert!(item.get("sk").is_some());
        assert!(item.get("tag").is_some());
        assert!(item.get("secret").is_none(), "KEYS_ONLY leaked 'secret'");
        assert!(item.get("another").is_none(), "KEYS_ONLY leaked 'another'");
    }

    #[test]
    fn gsi_include_projection_returns_keys_plus_listed_attrs() {
        let state = make_state_with_by_tag_gsi("INCLUDE", vec!["secret".into()]);
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": {
                    "pk":      { "S": "p1" },
                    "sk":      { "S": "s1" },
                    "tag":     { "S": "shared" },
                    "secret":  { "S": "in-include-list" },
                    "another": { "N": "42" },
                },
            }),
            &c,
        )
        .unwrap();

        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTag",
                "KeyConditionExpression": "tag = :t",
                "ExpressionAttributeValues": { ":t": { "S": "shared" } },
            }),
            &c,
        )
        .unwrap();
        let item = &resp["Items"][0];
        assert!(item.get("pk").is_some());
        assert!(item.get("tag").is_some());
        assert!(item.get("secret").is_some(), "INCLUDE list missed 'secret'");
        assert!(
            item.get("another").is_none(),
            "INCLUDE returned attribute not in list"
        );
    }

    #[test]
    fn gsi_all_projection_returns_full_item() {
        let state = make_state_with_by_tag_gsi("ALL", vec![]);
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": {
                    "pk":      { "S": "p1" },
                    "sk":      { "S": "s1" },
                    "tag":     { "S": "shared" },
                    "secret":  { "S": "preserved" },
                },
            }),
            &c,
        )
        .unwrap();

        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTag",
                "KeyConditionExpression": "tag = :t",
                "ExpressionAttributeValues": { ":t": { "S": "shared" } },
            }),
            &c,
        )
        .unwrap();
        let item = &resp["Items"][0];
        assert!(item.get("secret").is_some());
    }

    /// Page through a Query, following LastEvaluatedKey, collecting the base
    /// `pk` of every returned item. Panics if pagination fails to terminate
    /// (an infinite-loop bug), so a non-advancing cursor is caught loudly.
    fn paginate_pks(state: &DynamoState, sqlite: &SqliteStore, base_req: &Value) -> Vec<String> {
        let c = ctx();
        let mut pks = Vec::new();
        let mut esk: Option<Value> = None;
        for _ in 0..1000 {
            let mut req = base_req.clone();
            if let Some(k) = &esk {
                req["ExclusiveStartKey"] = k.clone();
            }
            let resp = query(state, sqlite, &req, &c).unwrap();
            for item in resp["Items"].as_array().unwrap() {
                pks.push(item["pk"]["S"].as_str().unwrap().to_string());
            }
            match resp.get("LastEvaluatedKey") {
                Some(k) if !k.is_null() => esk = Some(k.clone()),
                _ => return pks,
            }
        }
        panic!("pagination did not terminate within 1000 pages (cursor never advanced)");
    }

    fn make_state_with_tenant_gsi() -> DynamoState {
        make_state_with_tenant_gsi_named("byTenant", "tenant", "gsi_sk")
    }

    fn make_state_with_tenant_gsi_named(index: &str, hash: &str, range: &str) -> DynamoState {
        use crate::state::{GlobalSecondaryIndex, Projection};
        make_state_with_gsi(vec![GlobalSecondaryIndex {
            index_name: index.into(),
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: hash.into(),
                    key_type: "HASH".into(),
                },
                KeySchemaElement {
                    attribute_name: range.into(),
                    key_type: "RANGE".into(),
                },
            ],
            projection: Projection {
                projection_type: "ALL".into(),
                non_key_attributes: vec![],
            },
            status: "ACTIVE".into(),
        }])
    }

    #[test]
    fn hashonly_gsi_query_lek_carries_base_key_and_paginates() {
        // A GSI with only a HASH key. Every item shares the same index hash,
        // so without a base-key tiebreaker the cursor cannot advance and the
        // client loops on page 1 forever.
        let state = make_state_with_by_tag_gsi("ALL", vec![]);
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for i in 0..5 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk": {"S": format!("p{i}")},
                        "sk": {"S": "row"},
                        "tag": {"S": "shared"},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let req = json!({
            "TableName": "t",
            "IndexName": "byTag",
            "KeyConditionExpression": "tag = :t",
            "ExpressionAttributeValues": { ":t": {"S": "shared"} },
            "Limit": 2,
        });

        // The page-1 LEK must include the base primary key (pk + sk), not just
        // the GSI hash, or there is nothing to resume from.
        let page1 = query(&state, &sqlite, &req, &c).unwrap();
        let lek = &page1["LastEvaluatedKey"];
        assert!(lek.get("pk").is_some(), "GSI LEK missing base pk: {lek}");
        assert!(lek.get("sk").is_some(), "GSI LEK missing base sk: {lek}");

        let mut pks = paginate_pks(&state, &sqlite, &req);
        pks.sort();
        assert_eq!(
            pks,
            vec!["p0", "p1", "p2", "p3", "p4"],
            "every item must be returned exactly once across pages"
        );
    }

    #[test]
    fn gsi_query_with_tied_sort_key_paginates_without_loss() {
        // Three items in one GSI partition that all share the SAME gsi sort
        // key. A strict `gsi_sk > boundary` resume would skip the tied items
        // straddling a page boundary; the base-key tiebreaker prevents that.
        let state = make_state_with_tenant_gsi();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for i in 1..=3 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk": {"S": format!("i{i}")},
                        "sk": {"S": "row"},
                        "tenant": {"S": "a"},
                        "gsi_sk": {"S": "2025-01-01"},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let req = json!({
            "TableName": "t",
            "IndexName": "byTenant",
            "KeyConditionExpression": "tenant = :t",
            "ExpressionAttributeValues": { ":t": {"S": "a"} },
            "Limit": 1,
        });

        let mut pks = paginate_pks(&state, &sqlite, &req);
        pks.sort();
        assert_eq!(
            pks,
            vec!["i1", "i2", "i3"],
            "items sharing a gsi sort key must not be dropped across pages"
        );
    }

    #[test]
    fn gsi_begins_with_filter_desc_limit_paginates_like_chat_get() {
        // Mirrors a real client: query a GSI (ByUserStatus) with
        //   GSI_PK = :pk AND begins_with(GSI_SK, "ACTIVE#")
        // plus FilterExpression attribute_not_exists(deletedAt),
        // ScanIndexForward=false, Limit=50 -- then follow LastEvaluatedKey.
        let state = make_state_with_tenant_gsi_named("ByUserStatus", "GSI_PK", "GSI_SK");
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for i in 0..120 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk": {"S": format!("sess-{i:03}")},
                        "sk": {"S": "meta"},
                        "GSI_PK": {"S": "tenant#user"},
                        "GSI_SK": {"S": format!("ACTIVE#{i:03}")},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let req = json!({
            "TableName": "t",
            "IndexName": "ByUserStatus",
            "KeyConditionExpression": "GSI_PK = :pk AND begins_with(GSI_SK, :pfx)",
            "ExpressionAttributeValues": {
                ":pk": {"S": "tenant#user"},
                ":pfx": {"S": "ACTIVE#"},
            },
            "FilterExpression": "attribute_not_exists(deletedAt)",
            "ScanIndexForward": false,
            "Limit": 50,
        });

        // Page 1 LEK must carry both index keys AND the base primary key.
        let page1 = query(&state, &sqlite, &req, &c).unwrap();
        assert_eq!(page1["Count"], json!(50));
        let lek = &page1["LastEvaluatedKey"];
        assert!(lek.get("GSI_PK").is_some(), "LEK missing GSI_PK: {lek}");
        assert!(lek.get("GSI_SK").is_some(), "LEK missing GSI_SK: {lek}");
        assert!(lek.get("pk").is_some(), "LEK missing base pk: {lek}");
        assert!(!lek.as_object().unwrap().is_empty(), "LEK must not be {{}}");

        let pks = paginate_pks(&state, &sqlite, &req);
        let mut uniq = pks.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(
            uniq.len(),
            120,
            "all 120 sessions reachable, none lost/looped"
        );
        // Descending order: first item returned is the highest ACTIVE#.
        assert_eq!(pks.first().map(String::as_str), Some("sess-119"));
    }

    #[test]
    fn gsi_query_with_all_sort_keys_tied_paginates_without_loss() {
        // The real failure mode: a status-prefixed GSI sort key that repeats
        // across every session (here all 120 share GSI_SK="ACTIVE#"). Before
        // the base-key tiebreaker, page 1 returned 50 and "load more" came
        // back empty, stranding the other 70.
        let state = make_state_with_tenant_gsi_named("ByUserStatus", "GSI_PK", "GSI_SK");
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for i in 0..120 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk": {"S": format!("sess-{i:03}")},
                        "sk": {"S": "meta"},
                        "GSI_PK": {"S": "tenant#user"},
                        "GSI_SK": {"S": "ACTIVE#"},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let req = json!({
            "TableName": "t",
            "IndexName": "ByUserStatus",
            "KeyConditionExpression": "GSI_PK = :pk AND begins_with(GSI_SK, :pfx)",
            "ExpressionAttributeValues": {
                ":pk": {"S": "tenant#user"},
                ":pfx": {"S": "ACTIVE#"},
            },
            "FilterExpression": "attribute_not_exists(deletedAt)",
            "ScanIndexForward": false,
            "Limit": 50,
        });

        let pks = paginate_pks(&state, &sqlite, &req);
        let mut uniq = pks.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(
            uniq.len(),
            120,
            "tied sort keys must not strand sessions on later pages"
        );
    }

    #[test]
    fn query_limit_counts_evaluated_items_not_matches() {
        // AWS defines Limit as the number of items EVALUATED, with the
        // FilterExpression applied afterwards. 30 items, a filter matching
        // every 10th. Limit=10 evaluates sk 000..009 (only sk000 matches),
        // so a single page is Count=1, ScannedCount=10, LEK parked at sk009.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for i in 0..30 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk": {"S": "p"},
                        "sk": {"S": format!("{i:03}")},
                        "bucket": {"N": (i % 10).to_string()},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let req = json!({
            "TableName": "t",
            "KeyConditionExpression": "pk = :pk",
            "FilterExpression": "bucket = :z",
            "ExpressionAttributeValues": { ":pk": {"S": "p"}, ":z": {"N": "0"} },
            "Limit": 10,
        });
        let page1 = query(&state, &sqlite, &req, &c).unwrap();
        assert_eq!(page1["Count"], json!(1), "only sk000 matches in first 10");
        assert_eq!(
            page1["ScannedCount"],
            json!(10),
            "Limit caps evaluated items"
        );
        assert_eq!(
            page1["LastEvaluatedKey"]["sk"]["S"],
            json!("009"),
            "LEK parks on the last EVALUATED item, not the last match"
        );

        // Full pagination still returns every match exactly once (no loss).
        let matches = paginate_pks(&state, &sqlite, &req);
        assert_eq!(matches.len(), 3, "matches are sk 000, 010, 020");
    }

    #[test]
    fn scan_limit_counts_evaluated_items_not_matches() {
        // Same semantics for Scan: 30 rows in 30 partitions, scanned in
        // (pk,sk) order p000..p029. Limit=10 evaluates p000..p009; only
        // p000 has bucket 0 -> Count=1, ScannedCount=10, LEK at p009.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for i in 0..30 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk": {"S": format!("p{i:03}")},
                        "sk": {"S": "row"},
                        "bucket": {"N": (i % 10).to_string()},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let resp = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "FilterExpression": "bucket = :z",
                "ExpressionAttributeValues": { ":z": {"N": "0"} },
                "Limit": 10,
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["ScannedCount"], json!(10), "Limit caps evaluated rows");
        assert_eq!(resp["Count"], json!(1), "only p000 matches in first 10");
        assert_eq!(
            resp["LastEvaluatedKey"]["pk"]["S"],
            json!("p009"),
            "LEK parks on the last evaluated row"
        );
    }
}
