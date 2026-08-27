use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use std::collections::HashMap;

use crate::{
    expressions::{
        evaluate_condition, parse_condition, parse_projection,
        parser::{CompareOp, ConditionExpr, LogicalOp, Operand, resolve_path},
    },
    keys::storage_value_to_item,
    sqlite_store::{SkBound, SqliteStore},
    state::{DynamoItem, DynamoState},
    throttle::BucketKind,
};

use super::{
    build_consumed_capacity, get_expr_attr_names, get_expr_attr_values, opt_str,
    read_capacity_units, require_str, validate_expr_attr_values,
};
use crate::operations::item::{estimate_item_bytes, estimate_value_bytes, item_to_json};

use std::borrow::Cow;

/// Split an index key schema into its `(hash, range)` attribute names.
fn index_key_names(schema: &[crate::state::KeySchemaElement]) -> (Option<String>, Option<String>) {
    let find = |kind: &str| {
        schema
            .iter()
            .find(|k| k.key_type == kind)
            .map(|k| k.attribute_name.clone())
    };
    (find("HASH"), find("RANGE"))
}

/// Derive an index-friendly `sk` range from a Query's key condition.
///
/// Without this, a Query fetches and JSON-decodes every item in the
/// partition and then discards non-matching ones in Rust, making a
/// 10-item read cost O(partition). Pushing the sort-key range into the
/// SQL `WHERE` turns it into an index seek.
///
/// Returns `None` whenever the clause cannot be translated *exactly*,
/// because the bound only narrows what SQLite returns while the real
/// answer still comes from [`evaluate_condition`]. A too-wide bound is
/// merely slower; a too-narrow one would drop matching items.
///
/// Values are translated through [`crate::keys::storage_key`], the same
/// encoder writes use, so the bound is expressed in exactly the form the
/// column holds.
pub(crate) fn sk_bound_from_condition(
    cond: &ConditionExpr,
    sk_name: &str,
    expr_attr_names: &HashMap<String, String>,
    expr_attr_values: &serde_json::Map<String, Value>,
) -> Option<SkBound> {
    // Every scalar key type is now stored in a form whose byte order
    // matches DynamoDB's, so all three can be compared directly against
    // the stored column. Non-scalar placeholders get no bound.
    let bound_value = |op: &Operand| -> Option<String> {
        let Operand::Value(name) = op else {
            return None;
        };
        let v = expr_attr_values.get(name)?;
        if v.get("S").is_some() || v.get("N").is_some() || v.get("B").is_some() {
            crate::keys::storage_key(v)
        } else {
            None
        }
    };
    // `begins_with` is a string operation; a prefix range is meaningless
    // over an encoded number.
    let s_value = |op: &Operand| -> Option<String> {
        match op {
            Operand::Value(name) => expr_attr_values
                .get(name)
                .and_then(|v| v.get("S"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            Operand::Path(_) => None,
        }
    };
    let is_sk = |op: &Operand| -> bool {
        match op {
            Operand::Path(p) => resolve_path(p, expr_attr_names)
                .map(|r| r == sk_name)
                .unwrap_or(false),
            Operand::Value(_) => false,
        }
    };

    match cond {
        // A KeyConditionExpression is `pk = :v AND <sk clause>`, so walk
        // the AND children and take the first translatable sort-key one.
        ConditionExpr::Logical {
            op: LogicalOp::And,
            children,
        } => children
            .iter()
            .find_map(|c| sk_bound_from_condition(c, sk_name, expr_attr_names, expr_attr_values)),
        ConditionExpr::Comparison { left, op, right } if is_sk(left) => {
            let v = bound_value(right)?;
            Some(match op {
                CompareOp::Eq => SkBound {
                    lower: Some((v.clone(), true)),
                    upper: Some((v, true)),
                },
                CompareOp::Lt => SkBound {
                    upper: Some((v, false)),
                    ..Default::default()
                },
                CompareOp::Le => SkBound {
                    upper: Some((v, true)),
                    ..Default::default()
                },
                CompareOp::Gt => SkBound {
                    lower: Some((v, false)),
                    ..Default::default()
                },
                CompareOp::Ge => SkBound {
                    lower: Some((v, true)),
                    ..Default::default()
                },
                // `<>` is not a valid sort-key operator and does not
                // describe a range anyway.
                CompareOp::Ne => return None,
            })
        }
        ConditionExpr::Between { operand, low, high } if is_sk(operand) => Some(SkBound {
            lower: Some((s_value(low)?, true)),
            upper: Some((s_value(high)?, true)),
        }),
        ConditionExpr::BeginsWith(path, prefix) if is_sk(path) => {
            let p = s_value(prefix)?;
            // Every string with prefix `p` sorts in [p, next_prefix(p)).
            // If no successor exists the lower bound alone is still a
            // correct, if wider, narrowing.
            Some(SkBound {
                upper: next_prefix(&p).map(|u| (u, false)),
                lower: Some((p, true)),
            })
        }
        _ => None,
    }
}

/// Smallest string strictly greater than every string starting with
/// `prefix`, for use as an exclusive upper bound.
///
/// SQLite compares TEXT byte-wise, and UTF-8 byte order matches code
/// point order, so incrementing the last code point is sound. Returns
/// `None` when the prefix is all-maximal and no successor exists.
fn next_prefix(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        // Step over the UTF-16 surrogate gap, which holds no scalars.
        let next = match last as u32 + 1 {
            0xD800 => 0xE000,
            n => n,
        };
        if let Some(c) = char::from_u32(next) {
            chars.push(c);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

/// AWS DynamoDB caps `Query` / `Scan` responses at 1 MiB regardless of
/// `Limit`. Real clients are written to handle pagination via
/// `LastEvaluatedKey`, so enforcing the same cap keeps both wire
/// compatibility and our process memory bounded. Without it a single
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

    /// Whether the index stores this attribute.
    fn allows(&self, name: &str) -> bool {
        self.allowed.as_ref().is_none_or(|a| a.contains(name))
    }

    /// Apply the projection: drop attributes that the index would not
    /// store. An ALL projection borrows, since it drops nothing. ALL is
    /// the default and the common case, so cloning here would clone every
    /// examined item on most index reads.
    fn filter<'a>(&self, item: &'a DynamoItem) -> Cow<'a, DynamoItem> {
        match &self.allowed {
            None => Cow::Borrowed(item),
            Some(allow) => Cow::Owned(
                item.iter()
                    .filter(|(k, _)| allow.contains(k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
        }
    }
}

/// Size an item as the index stores it, without building the projected copy.
///
/// The 1 MiB page cap is charged against the index view, and materialising
/// that view purely to measure it is what made a filtered index read clone
/// every row it examined. Mirrors [`estimate_item_bytes`] over the surviving
/// attributes.
fn estimate_projected_item_bytes(item: &DynamoItem, view: Option<&IndexProjection>) -> usize {
    let Some(p) = view else {
        return estimate_item_bytes(item);
    };
    let mut total = 0usize;
    let mut count = 0usize;
    for (name, value) in item {
        if !p.allows(name) {
            continue;
        }
        total += name.len() + estimate_value_bytes(value);
        count += 1;
    }
    total + count * 4 + 2
}

/// Everything a Query or Scan needs to know about the index it targets.
///
/// Resolved once and shared by both read paths. Two copies of this logic is
/// how Query came to reject an unknown `IndexName` while Scan silently swept
/// the base table instead.
struct ResolvedIndex {
    /// Key names the key condition and the resume cursor are expressed in.
    /// The base table's keys when no index is named.
    hash_key: String,
    range_key: Option<String>,
    /// Storage slot backing a GSI. None for the base table and for an LSI,
    /// which is served out of the base partition.
    gsi_slot: Option<usize>,
    /// What the index stores. None when no index is named.
    projection: Option<IndexProjection>,
    /// Whether `projection` also governs what the read sees.
    ///
    /// True for a GSI: a KEYS_ONLY GSI can never surface a non-key attribute,
    /// so neither the filter nor the response may see one. False for an LSI,
    /// which lives in the base item's partition and so fetches non-projected
    /// attributes back from the base item rather than hiding them.
    projection_is_view: bool,
    /// Attribute an item must carry to appear in the index at all.
    membership_attr: Option<String>,
}

impl ResolvedIndex {
    /// The view the read filters and measures through, or None when it sees
    /// the whole base item.
    fn view(&self) -> Option<&IndexProjection> {
        self.projection.as_ref().filter(|_| self.projection_is_view)
    }
}

/// Resolve the target of a read, rejecting an index the table does not have.
fn resolve_index(
    table: &crate::state::Table,
    index_name: Option<&str>,
) -> Result<ResolvedIndex, AwsError> {
    let base_hash = table.hash_key().unwrap_or("").to_string();
    let base_range = table.range_key().map(str::to_string);

    let Some(idx) = index_name else {
        return Ok(ResolvedIndex {
            hash_key: base_hash,
            range_key: base_range,
            gsi_slot: None,
            projection: None,
            projection_is_view: false,
            membership_attr: None,
        });
    };

    if let Some((slot, gsi)) = table
        .gsi
        .iter()
        .enumerate()
        .find(|(_, g)| g.index_name == idx)
    {
        let (hash, range) = index_key_names(&gsi.key_schema);
        let hash = hash.ok_or_else(|| {
            AwsError::validation(format!("GSI {idx} has no HASH key in its KeySchema"))
        })?;
        let projection = IndexProjection::from_index(
            &gsi.projection,
            Some(base_hash),
            base_range,
            Some(hash.clone()),
            range.clone(),
        );
        return Ok(ResolvedIndex {
            membership_attr: Some(hash.clone()),
            hash_key: hash,
            range_key: range,
            gsi_slot: Some(slot).filter(|s| *s < crate::sqlite_store::MAX_GSI_SLOTS),
            projection: Some(projection),
            projection_is_view: true,
        });
    }

    if let Some(lsi) = table.lsi.iter().find(|l| l.index_name == idx) {
        let range = index_key_names(&lsi.key_schema).1;
        let projection = IndexProjection::from_index(
            &lsi.projection,
            Some(base_hash.clone()),
            base_range,
            Some(base_hash.clone()),
            range.clone(),
        );
        return Ok(ResolvedIndex {
            hash_key: base_hash,
            membership_attr: range.clone(),
            range_key: range,
            gsi_slot: None,
            projection: Some(projection),
            projection_is_view: false,
        });
    }

    Err(AwsError::validation(format!(
        "The table does not have the specified index: {idx}"
    )))
}

/// Resolve `Select` into the narrowing it asks of the response, rejecting the
/// combinations AWS rejects.
///
/// A GSI read is already confined to what the index projects by its view, so
/// `ALL_PROJECTED_ATTRIBUTES` only has work to do on an LSI, which otherwise
/// sees the whole base item. `verb` names the operation in the error message
/// the way AWS does.
fn resolve_select<'a>(
    select: &str,
    index: &'a ResolvedIndex,
    on_index: bool,
    has_projection: bool,
    verb: &str,
) -> Result<Option<&'a IndexProjection>, AwsError> {
    match select {
        "ALL_ATTRIBUTES" | "COUNT" => Ok(None),
        "ALL_PROJECTED_ATTRIBUTES" => {
            if !on_index {
                return Err(AwsError::validation(format!(
                    "ALL_PROJECTED_ATTRIBUTES can be used only when {verb} using an Index"
                )));
            }
            Ok(index.projection.as_ref().filter(|_| index.view().is_none()))
        }
        "SPECIFIC_ATTRIBUTES" => {
            if !has_projection {
                return Err(AwsError::validation(
                    "Cannot use Select=SPECIFIC_ATTRIBUTES without specifying \
                     ProjectionExpression or AttributesToGet",
                ));
            }
            Ok(None)
        }
        other => Err(AwsError::validation(format!(
            "1 validation error detected: Value '{other}' at 'select' failed to \
             satisfy constraint: Member must satisfy enum value set: \
             [SPECIFIC_ATTRIBUTES, COUNT, ALL_ATTRIBUTES, ALL_PROJECTED_ATTRIBUTES]"
        ))),
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

/// Validate a caller-supplied `ExclusiveStartKey` against the key schema the
/// operation resumes on. Without this the simulator would silently treat a
/// malformed cursor as a request to start from the beginning, masking client
/// bugs that fail against real AWS. `required` is the set of key attribute
/// names the resume needs (the index keys plus the base primary key). Empty
/// names and duplicates are ignored.
///
/// DynamoDB checks the cursor's shape before its contents, and the failures
/// carry different messages. A wrong attribute *count*, which is what an empty
/// `{}` and a cursor with extra attributes both are, is rejected on size
/// alone. Only a correctly sized cursor is checked attribute by attribute, and
/// only a cursor naming the right attributes has its values validated.
///
/// `required` pairs each key attribute name with whether it is a hash key,
/// which is what decides its size limit, and with the index it is a key of.
/// The value rules are the ones the write paths enforce, so a key that can be
/// stored is always a key that can be handed back as a cursor, and a violation
/// reads the same either way. An attribute that is both an index key and a
/// base key is reported as the base key, which is what it is.
fn validate_exclusive_start_key(
    esk: &serde_json::Map<String, Value>,
    required: &[(&str, bool, Option<&str>)],
) -> Result<(), AwsError> {
    let mut expected: Vec<(&str, bool, Option<&str>)> = required
        .iter()
        .copied()
        .filter(|(n, _, _)| !n.is_empty())
        .collect();
    expected.sort_unstable();
    expected.dedup_by_key(|(n, _, _)| *n);

    if esk.len() != expected.len() {
        return Err(AwsError::validation(
            "Exclusive Start Key must have same size as table's key schema",
        ));
    }
    for (name, is_hash, index) in &expected {
        let Some(value) = esk.get(*name) else {
            return Err(AwsError::validation("The provided starting key is invalid"));
        };
        super::item::validate_key_value(name, value, *is_hash, *index)?;
    }
    Ok(())
}

pub fn query(
    state: &DynamoState,
    sqlite: &SqliteStore,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let input = super::legacy::rewrite(input)?;
    let input = input.as_ref();
    let table_name = require_str(input, "TableName")?;
    validate_expr_attr_values(input)?;

    // Schema still comes from the in-memory cache during stage 3. Table
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
    let limit = parse_limit(input)?;
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

    let projection_paths: Vec<String> = projection_expr
        .map(parse_projection)
        .transpose()?
        .unwrap_or_default();

    // Base-table key names, captured while `table` is still borrowed. A GSI
    // query's LastEvaluatedKey must carry these as a resume tiebreaker even
    // though the index resolution below rebinds hash_key_name/range_key_name
    // to the GSI's own keys.
    let base_hash_name = table.hash_key().unwrap_or("").to_string();
    let base_range_name = table.range_key().map(|s| s.to_string());

    // Resolve which key schema applies. With IndexName, GSI/LSI metadata
    // names different attributes than the base table.
    let index_name = opt_str(input, "IndexName");
    let index = resolve_index(&table, index_name)?;
    let hash_key_name = index.hash_key.clone();
    let range_key_name = index.range_key.clone();
    let gsi_slot = index.gsi_slot;
    let select_view = resolve_select(
        select,
        &index,
        index_name.is_some(),
        !projection_paths.is_empty(),
        "Querying",
    )?;

    // Strongly consistent reads are not supported on GSIs: a GSI lags the
    // base table, so AWS rejects ConsistentRead=true on an index query with
    // ValidationException rather than silently serving stale-but-consistent
    // data. (Our storage is synchronous, but we honor the contract anyway.)
    if gsi_slot.is_some()
        && input
            .get("ConsistentRead")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    {
        return Err(AwsError::validation(
            "Consistent reads are not supported on global secondary indexes",
        ));
    }

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

    // A Query FilterExpression may not reference key attributes (they go in
    // the KeyConditionExpression). When querying an index, the index's own
    // keys are the ones that are off-limits.
    if let Some(filter) = &filter_condition {
        validate_filter_not_on_keys(
            filter,
            &expr_attr_names,
            &hash_key_name,
            range_key_name.as_deref(),
        )?;
    }

    // Pull the partition key value out of the KeyConditionExpression so we
    // can push the partition lookup down to SQLite. DynamoDB requires the
    // hash key in every Query, but our parser is conservative. If it
    // can't find one we fall back to a full Scan-style sweep.
    let pk_value = extract_pk_from_condition(
        key_condition_expr,
        &hash_key_name,
        &expr_attr_names,
        &expr_attr_values,
    );

    // A supplied ExclusiveStartKey must carry the full key schema the resume
    // needs (index hash/range plus the base primary key). Reject `{}` and
    // partial cursors the way real DynamoDB does, rather than silently
    // restarting from the beginning.
    if let Some(esk) = exclusive_start_key.as_ref() {
        validate_exclusive_start_key(
            esk,
            &[
                (&hash_key_name, true, index_name),
                (range_key_name.as_deref().unwrap_or(""), false, index_name),
                (&base_hash_name, true, None),
                (base_range_name.as_deref().unwrap_or(""), false, None),
            ],
        )?;
    }

    // Convert ExclusiveStartKey into resume cursors. The base-table query
    // (and LSI, which executes over the base partition) resumes on the base
    // sort key, which is unique within a partition. The GSI path additionally
    // needs the index sort key plus the base primary key as a tiebreaker,
    // since GSI sort keys repeat.
    let esk_base_pk = exclusive_start_key.as_ref().and_then(|esk| {
        esk.get(&base_hash_name)
            .and_then(crate::keys::storage_key)
            .map(|s| s.to_string())
    });
    let esk_base_sk = exclusive_start_key.as_ref().and_then(|esk| {
        base_range_name
            .as_deref()
            .and_then(|br| esk.get(br))
            .and_then(crate::keys::storage_key)
            .map(|s| s.to_string())
    });
    let esk_index_sk = exclusive_start_key.as_ref().and_then(|esk| {
        range_key_name
            .as_deref()
            .and_then(|rk| esk.get(rk))
            .and_then(crate::keys::storage_key)
            .map(|s| s.to_string())
    });

    // Sort-key pushdown, routed to whichever column actually holds the
    // sort key this query constrains:
    //
    //   * base table (no IndexName) -> the `sk` column
    //   * GSI                       -> that slot's `gsi{n}_sk` column
    //   * LSI                       -> nothing
    //
    // The LSI case is the trap: it constrains its own range attribute
    // while still streaming the base partition ordered by the *base*
    // sort key, so a bound would filter the wrong column and silently
    // drop matching items. `gsi_slot` is `None` for an LSI, which is
    // what distinguishes it from a base-table query here.
    let sk_pushdown = match (index_name, gsi_slot, range_key_name.as_deref()) {
        (None, _, Some(sk_name)) | (Some(_), Some(_), Some(sk_name)) => {
            sk_bound_from_condition(&key_condition, sk_name, &expr_attr_names, &expr_attr_values)
                .filter(|b| !b.is_unbounded())
        }
        _ => None,
    };

    let mut scanned_count = 0usize;
    let mut matched_count = 0usize;
    let mut items: Vec<DynamoItem> = Vec::new();
    let mut response_bytes = 0usize;
    let mut last_item: Option<DynamoItem> = None;
    let mut hit_limit = false;

    // Drop the table guard before SQLite IO. The dashmap Ref pins a
    // shard, and we don't want to hold it across a blocking read.
    drop(table);

    let view = index.view();
    let returns_items = select != "COUNT";

    let mut handle = |item: DynamoItem| -> Result<bool, AwsError> {
        // Key condition over typed attributes (covers sort key range,
        // BEGINS_WITH, BETWEEN, etc.). Items that fail the key condition
        // are skipped silently. DynamoDB's index would never have
        // surfaced them, so they don't count toward ScannedCount either.
        if !evaluate_condition(&key_condition, &item, &expr_attr_names, &expr_attr_values)? {
            return Ok(true);
        }
        // Sparse index semantics: an item that does not carry the index's
        // key is not in the index, so the index could not have surfaced it.
        // A GSI was already filtered in SQL; an LSI has no dedicated column
        // and is checked here.
        if let Some(attr) = &index.membership_attr
            && !item.contains_key(attr)
        {
            return Ok(true);
        }
        // This item is "evaluated": the index surfaced it. AWS counts every
        // evaluated item toward ScannedCount and the Limit, and applies any
        // FilterExpression only AFTER that accounting.
        scanned_count += 1;

        // AWS applies the GSI Projection BEFORE anything the request asks
        // for: a KEYS_ONLY index can never surface a non-key attribute, so
        // neither the FilterExpression nor the ProjectionExpression can see
        // one. A filter on an unprojected attribute therefore matches
        // nothing rather than falling back to the base item.
        //
        // Built only when something reads it. Under `Select: COUNT` with no
        // filter nothing does, and under an ALL projection it borrows.
        let after_index = match view {
            Some(p) if filter_condition.is_some() || returns_items => p.filter(&item),
            _ => Cow::Borrowed(&item),
        };

        let passes_filter = match &filter_condition {
            Some(filter) => {
                evaluate_condition(filter, &after_index, &expr_attr_names, &expr_attr_values)?
            }
            None => true,
        };

        if passes_filter {
            matched_count += 1;
            if returns_items {
                let selected = match select_view {
                    Some(p) => p.filter(after_index.as_ref()),
                    None => Cow::Borrowed(after_index.as_ref()),
                };
                items.push(apply_projection_to_item(
                    &selected,
                    &projection_paths,
                    &expr_attr_names,
                )?);
            }
        }

        // The 1 MiB cap is charged against examined bytes seen through the
        // index view, on every read including COUNT. Measured without
        // building the projected item, so a COUNT read allocates nothing
        // per row.
        response_bytes += estimate_projected_item_bytes(&item, view);

        // The cursor advances for every evaluated item so LastEvaluatedKey
        // lands on the last item examined, not the last one matched. Which
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
        if response_bytes >= MAX_RESPONSE_BYTES {
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
                sk_pushdown.as_ref(),
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
                sk_pushdown.as_ref(),
                |_sk, attrs| {
                    let item = storage_value_to_item(attrs).ok_or_else(|| {
                        AwsError::internal("DynamoDB stored attrs is not an object")
                    })?;
                    handle(item)
                },
            )?;
        }
    } else {
        // No usable hash-key constraint extracted. Fall back to a full
        // table scan (matches the legacy in-memory behaviour). Resume on the
        // base primary key, which is what scan_table orders by.
        let scan_start = exclusive_start_key.as_ref().and_then(|esk| {
            let pk = esk
                .get(&base_hash_name)
                .and_then(crate::keys::storage_key)?;
            let sk = base_range_name
                .as_deref()
                .and_then(|rk| esk.get(rk))
                .and_then(crate::keys::storage_key)
                .unwrap_or_default();
            Some((pk.to_string(), sk.to_string()))
        });
        let scan_start_ref = scan_start.as_ref().map(|(p, s)| (p.as_str(), s.as_str()));
        sqlite.scan_table(
            &ctx.account_id,
            &ctx.region,
            table_name,
            scan_start_ref,
            None,
            |_pk, _sk, attrs| {
                let item = storage_value_to_item(attrs)
                    .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;
                handle(item)
            },
        )?;
    }

    // A COUNT read reports the count alone. Real DynamoDB returns no Items
    // for it, and accumulating one per match is what made a COUNT scan hold
    // the whole table.
    let mut result = json!({
        "Count": matched_count,
        "ScannedCount": scanned_count,
    });
    if returns_items {
        result["Items"] = Value::Array(items.into_iter().map(|i| item_to_json(&i)).collect());
    }

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
    state.charge_throughput(ctx, table_name, BucketKind::Read, read_units)?;
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
    let input = super::legacy::rewrite(input)?;
    let input = input.as_ref();
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
    let limit = parse_limit(input)?;
    let select = opt_str(input, "Select").unwrap_or("ALL_ATTRIBUTES");
    let exclusive_start_key = input
        .get("ExclusiveStartKey")
        .and_then(|v| v.as_object())
        .cloned();

    let filter_condition = filter_expr.map(parse_condition).transpose()?;
    let projection_paths: Vec<String> = projection_expr
        .map(parse_projection)
        .transpose()?
        .unwrap_or_default();

    let hash_key_name = table.hash_key().unwrap_or("").to_string();
    let range_key_name = table.range_key().map(|s| s.to_string());

    // Resolve the requested index up front so the per-index
    // ConsumedCapacity breakdown can be attributed correctly. Computed
    // while `table` is still borrowed since the Ref is dropped below.
    //
    // Scanning an index is not the same as scanning the table. Only items
    // that actually materialise into the index are visible (sparse
    // semantics), and a GSI's items are seen through its Projection. The
    // membership test is pushed into SQL below for a GSI; an LSI has no
    // dedicated column, so it is checked per item here.
    let scan_index_name = opt_str(input, "IndexName").map(|s| s.to_string());
    let index = resolve_index(&table, scan_index_name.as_deref())?;
    let scan_gsi_slot = index.gsi_slot;
    let scan_index_is_gsi = scan_gsi_slot.is_some();
    let select_view = resolve_select(
        select,
        &index,
        scan_index_name.is_some(),
        !projection_paths.is_empty(),
        "Scanning",
    )?;

    drop(table);

    // ConsistentRead is not supported when scanning a GSI: AWS rejects
    // ConsistentRead=true on an index scan with ValidationException because a
    // GSI only offers eventually consistent reads. See [`query`] for the same
    // guard on the Query path.
    if scan_index_is_gsi
        && input
            .get("ConsistentRead")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    {
        return Err(AwsError::validation(
            "Consistent reads are not supported on global secondary indexes",
        ));
    }

    // Parallel Scan: Segment/TotalSegments shard the table into N disjoint
    // slices. Both must be supplied together; we hash each row's (pk, sk)
    // and only emit those whose hash mod TotalSegments == Segment.
    let segmenting = parse_segments(input)?;

    // A supplied ExclusiveStartKey must carry the base primary key. Reject
    // `{}` and partial cursors the way real DynamoDB does.
    if let Some(esk) = exclusive_start_key.as_ref() {
        let on_index = scan_index_name.as_deref();
        validate_exclusive_start_key(
            esk,
            &[
                (&index.hash_key, true, on_index),
                (index.range_key.as_deref().unwrap_or(""), false, on_index),
                (&hash_key_name, true, None),
                (range_key_name.as_deref().unwrap_or(""), false, None),
            ],
        )?;
    }

    // Translate ExclusiveStartKey -> (pk, sk) tuple SQLite uses for
    // resume. Tables with no sort key encode sk as the empty string.
    let scan_start = exclusive_start_key.as_ref().and_then(|esk| {
        let pk = esk.get(&hash_key_name).and_then(crate::keys::storage_key)?;
        let sk = range_key_name
            .as_deref()
            .and_then(|rk| esk.get(rk))
            .and_then(crate::keys::storage_key)
            .unwrap_or_default();
        Some((pk.to_string(), sk.to_string()))
    });

    let mut scanned_count = 0usize;
    let mut matched_count = 0usize;
    let mut items: Vec<DynamoItem> = Vec::new();
    let mut response_bytes = 0usize;
    let mut last_item: Option<DynamoItem> = None;
    let mut hit_limit = false;

    let view = index.view();
    let returns_items = select != "COUNT";

    let scan_start_ref = scan_start.as_ref().map(|(p, s)| (p.as_str(), s.as_str()));
    sqlite.scan_table(
        &ctx.account_id,
        &ctx.region,
        table_name,
        scan_start_ref,
        scan_gsi_slot,
        |pk, sk, attrs| {
            // Skip rows that don't belong to this segment so the worker
            // only sees its slice. We don't count skipped rows toward
            // ScannedCount. They belong to another worker's count.
            if let Some((segment, total)) = segmenting
                && segment_index(pk, sk, total) != segment
            {
                return Ok(true);
            }
            let item = storage_value_to_item(attrs)
                .ok_or_else(|| AwsError::internal("DynamoDB stored attrs is not an object"))?;

            // An LSI has no dedicated key column, so its sparse-index
            // membership is checked here. A GSI was already filtered in
            // SQL, and re-checking it costs nothing.
            if let Some(attr) = &index.membership_attr
                && !item.contains_key(attr)
            {
                return Ok(true);
            }

            // Every row in this segment is "evaluated": it counts toward
            // ScannedCount and the Limit before the FilterExpression runs.
            scanned_count += 1;

            // A GSI scan sees the item through the index's Projection,
            // exactly as Query does, before the filter or any request-level
            // projection. Built only when something reads it.
            let after_index = match view {
                Some(p) if filter_condition.is_some() || returns_items => p.filter(&item),
                _ => Cow::Borrowed(&item),
            };

            let passes_filter = match &filter_condition {
                Some(filter) => {
                    evaluate_condition(filter, &after_index, &expr_attr_names, &expr_attr_values)?
                }
                None => true,
            };

            if passes_filter {
                matched_count += 1;
                if returns_items {
                    let selected = match select_view {
                        Some(p) => p.filter(after_index.as_ref()),
                        None => Cow::Borrowed(after_index.as_ref()),
                    };
                    items.push(apply_projection_to_item(
                        &selected,
                        &projection_paths,
                        &expr_attr_names,
                    )?);
                }
            }

            // 1 MiB cap is charged against examined bytes, not matches, on
            // every read including COUNT.
            response_bytes += estimate_projected_item_bytes(&item, view);

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
            if response_bytes >= MAX_RESPONSE_BYTES {
                hit_limit = true;
                return Ok(false);
            }
            Ok(true)
        },
    )?;

    let mut result = json!({
        "Count": matched_count,
        "ScannedCount": scanned_count,
    });
    if returns_items {
        result["Items"] = Value::Array(items.into_iter().map(|i| item_to_json(&i)).collect());
    }

    if hit_limit && let Some(item) = last_item {
        // We stream the base table even when scanning an index, so the base
        // primary key alone is enough for our own resume. AWS still returns
        // the index keys alongside it, and a client that round-trips the
        // cursor through its own key-shaped type would notice their absence.
        let lek = last_evaluated_key(
            &item,
            &index.hash_key,
            index.range_key.as_deref(),
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
    state.charge_throughput(ctx, table_name, BucketKind::Read, read_units)?;
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

/// Parse and validate `Limit` for Query and Scan. AWS constrains it to an
/// integer of at least 1, so `Limit: 0` and negatives are ValidationExceptions
/// rather than a request for an empty page. Silently ignoring them would let a
/// client bug pass here and fail against real DynamoDB.
///
/// A whole-valued float is accepted: a client whose serializer emits `1.0` for
/// an integer is asking for a limit of 1, and rejecting it produces a message
/// claiming 1.0 is less than 1.
fn parse_limit(input: &Value) -> Result<Option<usize>, AwsError> {
    let Some(raw) = input.get("Limit").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let whole = raw.as_u64().or_else(|| {
        raw.as_f64()
            .filter(|f| f.fract() == 0.0 && *f >= 1.0)
            .map(|f| f as u64)
    });
    match whole {
        Some(v) if v >= 1 => Ok(Some(v as usize)),
        _ => {
            let shown = raw.as_str().map_or_else(|| raw.to_string(), str::to_string);
            Err(AwsError::validation(format!(
                "1 validation error detected: Value '{shown}' at 'limit' failed to \
                 satisfy constraint: Member must have value greater than or equal to 1"
            )))
        }
    }
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

/// Hash `(pk, sk)` into `[0, total)`. Uses Rust's default hasher. The
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
        // their own. The partition-key Eq clause is missing.
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
        // `:v = :w` style. No key path at all.
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

/// Resolve a document path down to its top-level attribute name. Keys are
/// always top-level scalars, so we only care about the segment before the
/// first `.`/`[`; the placeholder on that segment is then resolved.
fn top_level_attr_name(path: &str, names: &HashMap<String, String>) -> String {
    let first = path.split(['.', '[']).next().unwrap_or(path);
    resolve_attribute_name(first, names)
}

/// Collect the resolved top-level attribute names a filter condition
/// references. Value placeholders (`:v`) are ignored; only attribute
/// paths matter for the key check below.
fn collect_filter_paths(
    expr: &ConditionExpr,
    names: &HashMap<String, String>,
    out: &mut Vec<String>,
) {
    let push_op = |op: &Operand, out: &mut Vec<String>| {
        if let Operand::Path(p) = op {
            out.push(top_level_attr_name(p, names));
        }
    };
    match expr {
        ConditionExpr::Comparison { left, right, .. } => {
            push_op(left, out);
            push_op(right, out);
        }
        ConditionExpr::Between { operand, low, high } => {
            push_op(operand, out);
            push_op(low, out);
            push_op(high, out);
        }
        ConditionExpr::In { operand, values } => {
            push_op(operand, out);
            for v in values {
                push_op(v, out);
            }
        }
        ConditionExpr::Logical { children, .. } => {
            for c in children {
                collect_filter_paths(c, names, out);
            }
        }
        ConditionExpr::Not(inner) => collect_filter_paths(inner, names, out),
        ConditionExpr::AttributeExists(p) | ConditionExpr::AttributeNotExists(p) => {
            out.push(top_level_attr_name(p, names));
        }
        ConditionExpr::AttributeType(p, v) => {
            out.push(top_level_attr_name(p, names));
            push_op(v, out);
        }
        ConditionExpr::BeginsWith(p, v) | ConditionExpr::Contains(p, v) => {
            push_op(p, out);
            push_op(v, out);
        }
        ConditionExpr::SizeComparison { path, right, .. } => {
            out.push(top_level_attr_name(path, names));
            push_op(right, out);
        }
    }
}

/// Reject a Query FilterExpression that references the partition or sort
/// key. Real DynamoDB raises ValidationException here because key
/// attributes belong in the KeyConditionExpression, not the filter; awsim
/// was silently accepting them, which let callers ship queries that only
/// fail against AWS. Scan has no such restriction, so this is Query-only.
fn validate_filter_not_on_keys(
    filter: &ConditionExpr,
    names: &HashMap<String, String>,
    hash_key: &str,
    range_key: Option<&str>,
) -> Result<(), AwsError> {
    let mut paths = Vec::new();
    collect_filter_paths(filter, names, &mut paths);
    // Name the partition key first, matching how AWS surfaces the error.
    if !hash_key.is_empty() && paths.iter().any(|p| p == hash_key) {
        return validation_err(&format!(
            "Filter Expression can only contain non-primary key attributes: Primary key attribute: {hash_key}"
        ));
    }
    if let Some(rk) = range_key
        && paths.iter().any(|p| p == rk)
    {
        return validation_err(&format!(
            "Filter Expression can only contain non-primary key attributes: Primary key attribute: {rk}"
        ));
    }
    Ok(())
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

    use crate::state::{GlobalSecondaryIndex, LocalSecondaryIndex, Projection};

    fn item_with(attrs: &[(&str, Value)]) -> DynamoItem {
        let mut m = DynamoItem::new();
        for (k, v) in attrs {
            m.insert(k.to_string(), v.clone());
        }
        m
    }

    fn ks(name: &str, kt: &str) -> KeySchemaElement {
        KeySchemaElement {
            attribute_name: name.to_string(),
            key_type: kt.to_string(),
        }
    }

    fn projection(kind: &str, non_key: &[&str]) -> Projection {
        Projection {
            projection_type: kind.to_string(),
            non_key_attributes: non_key.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Base table `pk`/`sk` with an ALL GSI and a KEYS_ONLY LSI.
    fn table_with_indexes() -> Table {
        Table {
            name: "t".into(),
            arn: "arn".into(),
            key_schema: vec![ks("pk", "HASH"), ks("sk", "RANGE")],
            attribute_definitions: vec![],
            billing_mode: "PAY_PER_REQUEST".into(),
            status: "ACTIVE".into(),
            created_at: 0.0,
            gsi: vec![GlobalSecondaryIndex {
                index_name: "G1".into(),
                key_schema: vec![ks("gpk", "HASH"), ks("gsk", "RANGE")],
                projection: projection("ALL", &[]),
                status: "ACTIVE".into(),
            }],
            lsi: vec![LocalSecondaryIndex {
                index_name: "L1".into(),
                key_schema: vec![ks("pk", "HASH"), ks("lsk", "RANGE")],
                projection: projection("KEYS_ONLY", &[]),
            }],
            stream_enabled: false,
            stream_arn: None,
            stream_view_type: None,
            stream_records: std::collections::VecDeque::new(),
            stream_sequence: 0,
            ttl: Default::default(),
            tags: Default::default(),
            deletion_protection_enabled: false,
            sse: Default::default(),
            read_capacity_units: 0,
            write_capacity_units: 0,
        }
    }

    #[test]
    fn resolve_index_without_a_name_uses_the_base_table() {
        let t = table_with_indexes();
        let r = resolve_index(&t, None).expect("base table");
        assert_eq!(r.hash_key, "pk");
        assert_eq!(r.range_key.as_deref(), Some("sk"));
        assert!(r.gsi_slot.is_none());
        assert!(r.view().is_none());
        assert!(r.membership_attr.is_none());
    }

    #[test]
    fn resolve_index_resolves_a_gsi_to_its_own_keys_and_view() {
        let t = table_with_indexes();
        let r = resolve_index(&t, Some("G1")).expect("G1");
        assert_eq!(r.hash_key, "gpk");
        assert_eq!(r.range_key.as_deref(), Some("gsk"));
        assert_eq!(r.gsi_slot, Some(0));
        // A GSI is confined to what it projects, so it has a view.
        assert!(r.view().is_some());
        assert_eq!(r.membership_attr.as_deref(), Some("gpk"));
    }

    #[test]
    fn resolve_index_gives_an_lsi_the_base_hash_key_and_no_view() {
        let t = table_with_indexes();
        let r = resolve_index(&t, Some("L1")).expect("L1");
        assert_eq!(r.hash_key, "pk", "an LSI shares the base partition key");
        assert_eq!(r.range_key.as_deref(), Some("lsk"));
        assert!(r.gsi_slot.is_none(), "an LSI has no GSI storage slot");
        assert!(
            r.view().is_none(),
            "an LSI fetches non-projected attributes from the base item"
        );
        assert!(
            r.projection.is_some(),
            "the projection is still needed for Select"
        );
        assert_eq!(r.membership_attr.as_deref(), Some("lsk"));
    }

    #[test]
    fn resolve_index_rejects_an_unknown_name() {
        let t = table_with_indexes();
        let Err(err) = resolve_index(&t, Some("nope")) else {
            panic!("an unknown index must be rejected");
        };
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("does not have the specified index"));
    }

    #[test]
    fn projected_size_matches_a_materialised_projection() {
        let item = item_with(&[
            ("pk", json!({ "S": "a" })),
            ("sk", json!({ "S": "b" })),
            ("lsk", json!({ "S": "c" })),
            ("payload", json!({ "S": "dropped by KEYS_ONLY" })),
        ]);
        let t = table_with_indexes();
        let lsi = resolve_index(&t, Some("L1")).expect("L1");
        let p = lsi.projection.as_ref().expect("projection");

        assert_eq!(
            estimate_projected_item_bytes(&item, Some(p)),
            estimate_item_bytes(&p.filter(&item)),
            "measuring through the projection must match measuring a copy of it"
        );
    }

    #[test]
    fn an_all_projection_borrows_rather_than_clones() {
        let item = item_with(&[("pk", json!({ "S": "a" })), ("v", json!({ "S": "b" }))]);
        let t = table_with_indexes();
        let gsi = resolve_index(&t, Some("G1")).expect("G1");
        let view = gsi.view().expect("a GSI has a view");

        assert!(
            matches!(view.filter(&item), Cow::Borrowed(_)),
            "ALL is the common projection; cloning here clones every examined item"
        );
        assert_eq!(
            estimate_projected_item_bytes(&item, Some(view)),
            estimate_item_bytes(&item),
            "an ALL projection drops nothing, so it measures the whole item"
        );
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
    fn query_honors_legacy_key_conditions() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for sk in ["a1", "a2", "b1"] {
            put_item(
                &state,
                &sqlite,
                &json!({"TableName": "t", "Item": {"pk": {"S": "p1"}, "sk": {"S": sk}}}),
                &c,
            )
            .unwrap();
        }
        // A different partition that the query must not return.
        put_item(
            &state,
            &sqlite,
            &json!({"TableName": "t", "Item": {"pk": {"S": "p2"}, "sk": {"S": "a1"}}}),
            &c,
        )
        .unwrap();

        let resp = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditions": {
                    "pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "p1"}]},
                    "sk": {"ComparisonOperator": "BEGINS_WITH", "AttributeValueList": [{"S": "a"}]},
                },
            }),
            &c,
        )
        .unwrap();

        assert_eq!(
            resp["Count"],
            json!(2),
            "only p1 items with sk begins_with a"
        );
    }

    #[test]
    fn scan_honors_legacy_scan_filter() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({"TableName": "t", "Item": {"pk": {"S": "p1"}, "sk": {"S": "s"}, "tier": {"S": "gold"}}}),
            &c,
        )
        .unwrap();
        put_item(
            &state,
            &sqlite,
            &json!({"TableName": "t", "Item": {"pk": {"S": "p2"}, "sk": {"S": "s"}, "tier": {"S": "silver"}}}),
            &c,
        )
        .unwrap();

        let resp = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "ScanFilter": {
                    "tier": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "gold"}]}
                },
            }),
            &c,
        )
        .unwrap();

        assert_eq!(resp["Count"], json!(1));
        assert_eq!(resp["Items"][0]["tier"], json!({"S": "gold"}));
    }

    #[test]
    fn scan_rejects_reserved_word_in_filter_even_on_empty_table() {
        // No items: AWS still validates the expression, so a bare reserved
        // word must be rejected up front rather than slipping through because
        // nothing was evaluated.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "FilterExpression": "Size = :v",
                "ExpressionAttributeValues": {":v": {"N": "1"}},
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(
            err.message.contains("reserved keyword"),
            "got {}",
            err.message
        );
    }

    #[test]
    fn query_rejects_reserved_word_in_projection() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": {":p": {"S": "x"}},
                "ProjectionExpression": "Status",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
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

    fn tenant_gsi() -> Vec<crate::state::GlobalSecondaryIndex> {
        use crate::state::{GlobalSecondaryIndex, Projection};
        vec![GlobalSecondaryIndex {
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
        }]
    }

    #[test]
    fn query_on_gsi_rejects_consistent_read() {
        let state = make_state_with_gsi(tenant_gsi());
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTenant",
                "ConsistentRead": true,
                "KeyConditionExpression": "tenant = :t",
                "ExpressionAttributeValues": { ":t": {"S": "a"} },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("global secondary index"));
    }

    #[test]
    fn scan_on_gsi_rejects_consistent_read() {
        let state = make_state_with_gsi(tenant_gsi());
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTenant",
                "ConsistentRead": true,
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("global secondary index"));
    }

    #[test]
    fn query_scanned_count_excludes_items_that_failed_key_condition() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        // Three sort-key slots; the key condition selects only sk = "y".
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

    /// Table "t" (pk/sk) with a KEYS_ONLY LSI "byRank" on (pk, rank).
    fn make_state_with_rank_lsi() -> DynamoState {
        use crate::state::Projection;
        let state = make_state();
        state.tables.get_mut("t").unwrap().lsi = vec![crate::state::LocalSecondaryIndex {
            index_name: "byRank".into(),
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "pk".into(),
                    key_type: "HASH".into(),
                },
                KeySchemaElement {
                    attribute_name: "rank".into(),
                    key_type: "RANGE".into(),
                },
            ],
            projection: Projection {
                projection_type: "KEYS_ONLY".into(),
                non_key_attributes: vec![],
            },
        }];
        state
    }

    #[test]
    fn gsi_filter_cannot_see_unprojected_attributes() {
        // A KEYS_ONLY GSI never stores 'secret', so DynamoDB has nothing to
        // compare and the filter matches nothing. It does not fall back to
        // the base item. The item is still evaluated, so ScannedCount is 1.
        let state = make_state_with_by_tag_gsi("KEYS_ONLY", vec![]);
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": {
                    "pk":     { "S": "p1" },
                    "sk":     { "S": "s1" },
                    "tag":    { "S": "shared" },
                    "secret": { "S": "hit" },
                },
            }),
            &c,
        )
        .unwrap();

        let req = json!({
            "TableName": "t",
            "IndexName": "byTag",
            "KeyConditionExpression": "tag = :t",
            "FilterExpression": "secret = :s",
            "ExpressionAttributeValues": { ":t": {"S": "shared"}, ":s": {"S": "hit"} },
        });
        let resp = query(&state, &sqlite, &req, &c).unwrap();
        assert_eq!(
            resp["Count"],
            json!(0),
            "filter on an unprojected GSI attribute must not match"
        );
        assert_eq!(resp["ScannedCount"], json!(1), "the item was still read");

        let resp = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTag",
                "FilterExpression": "secret = :s",
                "ExpressionAttributeValues": { ":s": {"S": "hit"} },
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["Count"], json!(0), "Scan agrees with Query");
    }

    #[test]
    fn lsi_filter_sees_unprojected_attributes() {
        // An LSI sits in the base item's own partition, so AWS fetches
        // non-projected attributes from the base table rather than hiding
        // them. A KEYS_ONLY LSI can still filter and project on 'secret'.
        let state = make_state_with_rank_lsi();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": {
                    "pk":     { "S": "p1" },
                    "sk":     { "S": "s1" },
                    "rank":   { "N": "1" },
                    "secret": { "S": "hit" },
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
                "IndexName": "byRank",
                "KeyConditionExpression": "pk = :p",
                "FilterExpression": "secret = :s",
                "ProjectionExpression": "secret",
                "ExpressionAttributeValues": { ":p": {"S": "p1"}, ":s": {"S": "hit"} },
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["Count"], json!(1), "LSI filter reads through to base");
        assert_eq!(
            resp["Items"][0]["secret"]["S"],
            json!("hit"),
            "LSI fetches an unprojected attribute the caller asked for"
        );
    }

    #[test]
    fn scan_index_last_evaluated_key_carries_index_keys() {
        // AWS keys an index-scan cursor on the index keys plus the base
        // primary key, because index keys are not unique on their own.
        let state = make_state_with_by_tag_gsi("ALL", vec![]);
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        for i in 0..3 {
            put_item(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "Item": {
                        "pk":  { "S": format!("p{i}") },
                        "sk":  { "S": "s1" },
                        "tag": { "S": "shared" },
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let resp = scan(
            &state,
            &sqlite,
            &json!({ "TableName": "t", "IndexName": "byTag", "Limit": 1 }),
            &c,
        )
        .unwrap();
        let lek = &resp["LastEvaluatedKey"];
        assert_eq!(lek["pk"]["S"], json!("p0"));
        assert_eq!(lek["sk"]["S"], json!("s1"));
        assert_eq!(
            lek["tag"]["S"],
            json!("shared"),
            "index key missing from LEK"
        );

        // The cursor still round-trips: feeding it back resumes the scan.
        let resp2 = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "IndexName": "byTag",
                "Limit": 1,
                "ExclusiveStartKey": lek,
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp2["Items"][0]["pk"]["S"], json!("p1"));
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
        // ScanIndexForward=false and Limit=50, then follow LastEvaluatedKey.
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
                        "slot": {"N": (i % 10).to_string()},
                    },
                }),
                &c,
            )
            .unwrap();
        }

        let req = json!({
            "TableName": "t",
            "KeyConditionExpression": "pk = :pk",
            "FilterExpression": "slot = :z",
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
    fn limit_below_one_is_rejected() {
        // AWS constrains Limit to >= 1. Zero is not "return nothing", and a
        // negative is not "no limit"; both are ValidationExceptions.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p"}, "sk": {"S": "a"} },
            }),
            &c,
        )
        .unwrap();

        for bad in [json!(0), json!(-1)] {
            let err = query(
                &state,
                &sqlite,
                &json!({
                    "TableName": "t",
                    "KeyConditionExpression": "pk = :pk",
                    "ExpressionAttributeValues": { ":pk": {"S": "p"} },
                    "Limit": bad,
                }),
                &c,
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("greater than or equal to 1"),
                "Query Limit {bad} should be rejected, got {err}"
            );

            let err = scan(
                &state,
                &sqlite,
                &json!({ "TableName": "t", "Limit": bad }),
                &c,
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("greater than or equal to 1"),
                "Scan Limit {bad} should be rejected, got {err}"
            );
        }

        // An absent or null Limit still means "no limit", not an error.
        let resp = scan(&state, &sqlite, &json!({ "TableName": "t" }), &c).unwrap();
        assert_eq!(resp["Count"], json!(1));
    }

    #[test]
    fn scan_limit_counts_evaluated_items_not_matches() {
        // Same semantics for Scan: 30 rows in 30 partitions, scanned in
        // (pk,sk) order p000..p029. Limit=10 evaluates p000..p009; only
        // p000 has slot 0 -> Count=1, ScannedCount=10, LEK at p009.
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
                        "slot": {"N": (i % 10).to_string()},
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
                "FilterExpression": "slot = :z",
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

    #[test]
    fn query_rejects_empty_exclusive_start_key() {
        // An empty {} cursor (a common client bug, since {} is truthy in JS)
        // must be rejected like real DynamoDB, not silently restarted.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "x"} },
                "ExclusiveStartKey": {},
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn scan_rejects_partial_exclusive_start_key() {
        // Missing the sort key on a pk+sk table is an invalid cursor.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "ExclusiveStartKey": { "pk": {"S": "x"} },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn exclusive_start_key_size_is_checked_before_its_contents() {
        // AWS separates the two failures: a cursor with the wrong number of
        // attributes is rejected on size, and only a correctly sized one is
        // checked attribute by attribute. Table "t" is pk + sk, so the
        // schema size is 2.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        let scan_with = |esk: Value| {
            scan(
                &state,
                &sqlite,
                &json!({ "TableName": "t", "ExclusiveStartKey": esk }),
                &c,
            )
            .unwrap_err()
        };

        for wrong_size in [
            json!({}),
            json!({ "pk": {"S": "x"} }),
            json!({ "pk": {"S": "x"}, "sk": {"S": "y"}, "extra": {"S": "z"} }),
        ] {
            let err = scan_with(wrong_size.clone());
            assert_eq!(err.code, "ValidationException");
            assert!(
                err.message.contains("same size as table's key schema"),
                "{wrong_size} should fail on size, got: {}",
                err.message
            );
        }

        // Right size, wrong names: a contents error, not a size error.
        let err = scan_with(json!({ "pk": {"S": "x"}, "nope": {"S": "y"} }));
        assert_eq!(err.code, "ValidationException");
        assert_eq!(err.message, "The provided starting key is invalid");
    }

    #[test]
    fn exclusive_start_key_values_are_validated_like_written_keys() {
        // A correctly shaped cursor still has to carry usable key values.
        // Before this check an empty-string key silently restarted the scan
        // from the beginning, and an oversized one was accepted as a cursor
        // even though the same key could never have been written.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        let scan_with = |esk: Value| {
            scan(
                &state,
                &sqlite,
                &json!({ "TableName": "t", "ExclusiveStartKey": esk }),
                &c,
            )
            .unwrap_err()
        };

        let err = scan_with(json!({ "pk": {"S": ""}, "sk": {"S": "y"} }));
        assert_eq!(err.code, "ValidationException");
        assert!(
            err.message.contains("cannot contain an empty string value"),
            "got: {}",
            err.message
        );

        let err = scan_with(json!({ "pk": {"S": "x"}, "sk": {"S": ""} }));
        assert!(
            err.message.contains("cannot contain an empty string value"),
            "an empty sort key is invalid too, got: {}",
            err.message
        );

        // 2048 bytes for a partition key, 1024 for a sort key.
        let err = scan_with(json!({
            "pk": {"S": "a".repeat(2049)},
            "sk": {"S": "y"},
        }));
        assert!(
            err.message.contains("Size of hashkey"),
            "got: {}",
            err.message
        );
        let err = scan_with(json!({
            "pk": {"S": "x"},
            "sk": {"S": "a".repeat(1025)},
        }));
        assert!(
            err.message.contains("Size of rangekey"),
            "got: {}",
            err.message
        );

        // The limits are exact, not approximate: at the cap the cursor is
        // valid and the scan simply resumes past it.
        let resp = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "ExclusiveStartKey": {
                    "pk": {"S": "a".repeat(2048)},
                    "sk": {"S": "b".repeat(1024)},
                },
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["Count"], json!(0));
    }

    #[test]
    fn oversized_keys_are_rejected_on_write_too() {
        // The read and write paths have to agree: a key that cannot be
        // written must not be presentable as a cursor, and vice versa.
        // Without this, a stored oversized key would produce a
        // LastEvaluatedKey our own cursor validation then rejects.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "a".repeat(2049)}, "sk": {"S": "y"} },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(
            err.message.contains("Size of hashkey"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn index_key_values_are_validated_on_write() {
        // A GSI cursor carries the index key's value, so index keys have to
        // clear the same rules as base keys. UpdateItem counts: setting a
        // GSI key attribute is another way to reach the index.
        let state = make_state_with_by_tag_gsi("ALL", vec![]);
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();

        let err = put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p1"}, "sk": {"S": "s1"}, "tag": {"S": ""} },
            }),
            &c,
        )
        .unwrap_err();
        assert!(
            err.message.contains("IndexName: byTag"),
            "an empty index key names the index, got: {}",
            err.message
        );

        let err = super::super::item::update_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Key": { "pk": {"S": "p1"}, "sk": {"S": "s1"} },
                "UpdateExpression": "SET tag = :t",
                "ExpressionAttributeValues": { ":t": {"S": "a".repeat(2049)} },
            }),
            &c,
        )
        .unwrap_err();
        assert!(
            err.message.contains("Size of hashkey"),
            "an update must not smuggle an oversized index key in, got: {}",
            err.message
        );
    }

    #[test]
    fn query_rejects_filter_on_sort_key() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :pk",
                "FilterExpression": "begins_with(sk, :prefix)",
                "ExpressionAttributeValues": {
                    ":pk": {"S": "p"},
                    ":prefix": {"S": "x"},
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert_eq!(
            err.message,
            "Filter Expression can only contain non-primary key attributes: Primary key attribute: sk"
        );
    }

    #[test]
    fn query_rejects_filter_on_partition_key() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :pk",
                "FilterExpression": "pk <> :other",
                "ExpressionAttributeValues": {
                    ":pk": {"S": "p"},
                    ":other": {"S": "q"},
                },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert_eq!(
            err.message,
            "Filter Expression can only contain non-primary key attributes: Primary key attribute: pk"
        );
    }

    #[test]
    fn query_rejects_filter_on_key_behind_name_placeholder() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let err = query(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "KeyConditionExpression": "pk = :pk",
                "FilterExpression": "attribute_not_exists(#s)",
                "ExpressionAttributeNames": { "#s": "sk" },
                "ExpressionAttributeValues": { ":pk": {"S": "p"} },
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert_eq!(
            err.message,
            "Filter Expression can only contain non-primary key attributes: Primary key attribute: sk"
        );
    }

    #[test]
    fn query_allows_filter_on_non_key_attribute() {
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p"}, "sk": {"S": "s"}, "status": {"S": "active"} },
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
                "FilterExpression": "#st = :status",
                "ExpressionAttributeNames": { "#st": "status" },
                "ExpressionAttributeValues": {
                    ":pk": {"S": "p"},
                    ":status": {"S": "active"},
                },
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["Items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn scan_allows_filter_on_key_attribute() {
        // Parity guard: unlike Query, real DynamoDB Scan permits key
        // attributes in a FilterExpression.
        let state = make_state();
        let sqlite = SqliteStore::in_memory().unwrap();
        let c = ctx();
        put_item(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "Item": { "pk": {"S": "p"}, "sk": {"S": "keep"} },
            }),
            &c,
        )
        .unwrap();
        let resp = scan(
            &state,
            &sqlite,
            &json!({
                "TableName": "t",
                "FilterExpression": "begins_with(sk, :prefix)",
                "ExpressionAttributeValues": { ":prefix": {"S": "ke"} },
            }),
            &c,
        )
        .unwrap();
        assert_eq!(resp["Items"].as_array().unwrap().len(), 1);
    }

    /// The bound only narrows what SQLite returns; the typed condition
    /// still decides. So these assertions are about it never being
    /// *narrower* than the real clause.
    mod sk_pushdown {
        use super::*;
        use crate::expressions::parse_condition;

        fn bound(expr: &str, values: serde_json::Value) -> Option<SkBound> {
            let cond = parse_condition(expr).unwrap();
            let names = HashMap::new();
            let vals = values.as_object().unwrap().clone();
            sk_bound_from_condition(&cond, "sk", &names, &vals)
        }

        #[test]
        fn between_maps_to_an_inclusive_range() {
            let b = bound(
                "pk = :p AND sk BETWEEN :a AND :b",
                json!({":p": {"S": "p"}, ":a": {"S": "b"}, ":b": {"S": "y"}}),
            )
            .expect("bound");
            assert_eq!(b.lower, Some(("b".to_string(), true)));
            assert_eq!(b.upper, Some(("y".to_string(), true)));
        }

        #[test]
        fn begins_with_maps_to_a_half_open_prefix_range() {
            let b = bound(
                "pk = :p AND begins_with(sk, :v)",
                json!({":p": {"S": "p"}, ":v": {"S": "item#"}}),
            )
            .expect("bound");
            assert_eq!(b.lower, Some(("item#".to_string(), true)));
            // '#' + 1 == '$', exclusive: covers every "item#..." string.
            assert_eq!(b.upper, Some(("item$".to_string(), false)));
        }

        #[test]
        fn comparisons_map_to_one_sided_bounds() {
            let gt = bound(
                "pk = :p AND sk > :v",
                json!({":p": {"S": "p"}, ":v": {"S": "m"}}),
            )
            .unwrap();
            assert_eq!(gt.lower, Some(("m".to_string(), false)));
            assert!(gt.upper.is_none());

            let le = bound(
                "pk = :p AND sk <= :v",
                json!({":p": {"S": "p"}, ":v": {"S": "m"}}),
            )
            .unwrap();
            assert_eq!(le.upper, Some(("m".to_string(), true)));
            assert!(le.lower.is_none());
        }

        /// `sk` is stored as raw text, so "10" < "9". Pushing a numeric
        /// range into SQL would drop matching items.
        #[test]
        fn numeric_sort_keys_are_never_pushed_down() {
            let b = bound(
                "pk = :p AND sk BETWEEN :a AND :b",
                json!({":p": {"S": "p"}, ":a": {"N": "2"}, ":b": {"N": "10"}}),
            );
            assert!(b.is_none(), "numeric sort key must not push down");
        }

        #[test]
        fn partition_key_only_yields_no_bound() {
            let b = bound("pk = :p", json!({":p": {"S": "p"}}));
            assert!(b.is_none());
        }

        #[test]
        fn next_prefix_steps_over_the_surrogate_gap() {
            // U+D7FF + 1 lands in the surrogate range, which holds no
            // scalar values, so it must jump to U+E000.
            let p = format!("a{}", char::from_u32(0xD7FF).unwrap());
            let up = next_prefix(&p).unwrap();
            assert!(up > p, "successor must sort after the prefix");
            assert_eq!(up.chars().last().unwrap() as u32, 0xE000);
        }

        #[test]
        fn next_prefix_carries_when_last_char_is_maximal() {
            let p = format!("a{}", char::MAX);
            let up = next_prefix(&p).expect("carry to the previous char");
            assert_eq!(up, "b");
            assert!(up > p);
        }
    }
}
