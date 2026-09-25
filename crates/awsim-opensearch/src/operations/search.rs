use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::index::index_not_found;
use crate::state::OpenSearchState;

/// Search documents in an index (or multiple indices).
///
/// Supports:
/// - `match` queries (single field)
/// - `multi_match` queries (multiple fields)
/// - `match_all` queries
/// - `bool` queries with `must`, `should`, `filter`
/// - `term` queries (exact match)
/// - `terms` queries (set membership)
/// - `range` queries (gt, gte, lt, lte on strings/numbers)
/// - `wildcard` queries (field-level pattern matching)
/// - `prefix` queries
/// - `exists` queries
/// - `ids` queries
/// - `query_string` queries
/// - `knn` queries, top-level or nested in `bool`, with an optional
///   `filter` (brute-force cosine similarity over a numeric vector
///   field. No ANN index, but correct enough for emulator workloads
///   up to a few thousand vectors)
///
/// Also honours `sort`, `from`/`size`, `collapse` (by `field`) and
/// `_source` filtering.
pub fn search(state: &OpenSearchState, index_pattern: &str, body: &Value) -> (u16, Value) {
    let size = body["size"].as_u64().unwrap_or(10) as usize;
    let from = body["from"].as_u64().unwrap_or(0) as usize;
    let query = body
        .get("query")
        .cloned()
        .unwrap_or(json!({"match_all": {}}));

    let matching_indices = resolve_indices(state, index_pattern);

    if matching_indices.is_empty() || !matching_indices.iter().any(|n| state.index_exists(n)) {
        let name = index_pattern.split(',').next().unwrap_or(index_pattern);
        return (404, index_not_found(name));
    }

    let mut hits: Vec<Value> = Vec::new();

    // Pre-extract ids filter for _id matching
    let ids_filter: Option<Vec<String>> = query
        .get("ids")
        .and_then(|i| i.get("values"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });

    for idx_name in &matching_indices {
        if !state.index_exists(idx_name) {
            continue;
        }
        // k-NN runs per shard, and each index here is a single shard.
        let mut query = query.clone();
        apply_knn_cutoffs(state, idx_name, &mut query);
        let _ = state.for_each_doc(idx_name, |doc_id, doc| {
            if let Some(ref allowed) = ids_filter
                && !allowed.contains(&doc_id.to_string())
            {
                return true;
            }
            let score = match_score(&query, doc);
            if score > 0.0 {
                hits.push(json!({
                    "_index": idx_name,
                    "_id": doc_id,
                    "_score": score,
                    "_source": doc,
                }));
            }
            true
        });
    }

    if let Some(sort_spec) = body.get("sort") {
        sort_hits(&mut hits, sort_spec);
    } else {
        hits.sort_by(|a, b| {
            let sa = a["_score"].as_f64().unwrap_or(0.0);
            let sb = b["_score"].as_f64().unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // With `collapse`, `hits.total` still counts every matching doc.
    let total = hits.len();
    if let Some(field) = body.pointer("/collapse/field").and_then(Value::as_str) {
        collapse_hits(&mut hits, field);
    }
    let paged: Vec<Value> = hits
        .into_iter()
        .skip(from)
        .take(size)
        .map(|mut hit| {
            if let Some(obj) = hit.as_object_mut()
                && let Some(source) = obj.remove("_source")
                && let Some(filtered) = filter_source(source, body.get("_source"))
            {
                obj.insert("_source".to_string(), filtered);
            }
            hit
        })
        .collect();

    (
        200,
        json!({
            "took": 1,
            "timed_out": false,
            "_shards": { "total": 1, "successful": 1, "skipped": 0, "failed": 0 },
            "hits": {
                "total": { "value": total, "relation": "eq" },
                "max_score": paged.first().and_then(|h| h["_score"].as_f64()).unwrap_or(0.0),
                "hits": paged,
            }
        }),
    )
}

/// Count documents matching a query.
pub fn count(state: &OpenSearchState, index_name: &str, body: &Value) -> (u16, Value) {
    let query = body
        .get("query")
        .cloned()
        .unwrap_or(json!({"match_all": {}}));

    let resolved = state.resolve_alias(index_name);

    if !resolved.iter().any(|n| state.index_exists(n)) {
        return (404, index_not_found(index_name));
    }

    let mut count: usize = 0;

    for name in &resolved {
        if !state.index_exists(name) {
            continue;
        }
        let _ = state.for_each_doc(name, |_, doc| {
            if match_score(&query, doc) > 0.0 {
                count += 1;
            }
            true
        });
    }

    (
        200,
        json!({
            "count": count,
            "_shards": { "total": 1, "successful": 1, "skipped": 0, "failed": 0 },
        }),
    )
}

/// Resolve an index pattern (wildcard, alias, or comma-separated list)
/// down to a concrete list of index names.
///
/// Wildcards: `prefix*`, `*suffix`, `pre*fix`, `*` (all).
fn resolve_indices(state: &OpenSearchState, pattern: &str) -> Vec<String> {
    if pattern.contains('*') {
        return state
            .list_indices()
            .into_iter()
            .filter_map(|(name, _)| {
                if wildcard_match(pattern, &name) {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
    }
    pattern
        .split(',')
        .flat_map(|s| state.resolve_alias(s.trim()))
        .collect()
}

/// Simple wildcard match supporting `*` (any chars) and `?` (single char).
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    wildcard_match_inner(&p, &t, 0, 0)
}

fn wildcard_match_inner(p: &[char], t: &[char], pi: usize, ti: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    if p[pi] == '*' {
        for i in ti..=t.len() {
            if wildcard_match_inner(p, t, pi + 1, i) {
                return true;
            }
        }
        return false;
    }
    if ti < t.len() && (p[pi] == '?' || p[pi] == t[ti]) {
        return wildcard_match_inner(p, t, pi + 1, ti + 1);
    }
    false
}

/// Score a document against a query. Returns 0.0 for no match.
pub(crate) fn match_score(query: &Value, doc: &Value) -> f64 {
    if let Some(obj) = query.as_object() {
        if obj.contains_key("match_all") {
            return 1.0;
        }

        // match: { "field": "value" } or { "field": { "query": "value" } }
        // Evaluate all fields and sum scores.
        if let Some(match_obj) = obj.get("match").and_then(|m| m.as_object()) {
            let mut total_score = 0.0;
            for (field, match_val) in match_obj {
                let query_text = match_val
                    .as_str()
                    .or_else(|| match_val.get("query").and_then(|q| q.as_str()))
                    .unwrap_or("");
                if let Some(field_val) = get_nested_field(doc, field) {
                    let field_str = value_to_string(field_val);
                    total_score += text_match_score(query_text, &field_str);
                }
            }
            return total_score;
        }

        // multi_match: { "query": "text", "fields": ["f1", "f2"] }
        if let Some(mm) = obj.get("multi_match").and_then(|m| m.as_object()) {
            let query_text = mm.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let fields = mm
                .get("fields")
                .and_then(|f| f.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();

            let mut best_score = 0.0;
            for field in &fields {
                // Handle boosted fields like "title^2"
                let (field_name, boost) = if let Some(pos) = field.find('^') {
                    let (name, b) = field.split_at(pos);
                    (name, b[1..].parse::<f64>().unwrap_or(1.0))
                } else {
                    (*field, 1.0)
                };

                if let Some(field_val) = get_nested_field(doc, field_name) {
                    let field_str = value_to_string(field_val);
                    let score = text_match_score(query_text, &field_str) * boost;
                    if score > best_score {
                        best_score = score;
                    }
                }
            }
            return best_score;
        }

        // term: { "field": "exact_value" }
        // Supports string, number, and boolean values.
        if let Some(term_obj) = obj.get("term").and_then(|t| t.as_object()) {
            for (field, expected) in term_obj {
                if let Some(field_val) = get_nested_field(doc, field)
                    && term_match(expected, field_val)
                {
                    return 1.0;
                }
            }
            return 0.0;
        }

        // terms: { "field": ["val1", "val2"] }
        if let Some(terms_obj) = obj.get("terms").and_then(|t| t.as_object()) {
            for (field, values) in terms_obj {
                if let Some(arr) = values.as_array()
                    && let Some(field_val) = get_nested_field(doc, field)
                {
                    for expected in arr {
                        if term_match(expected, field_val) {
                            return 1.0;
                        }
                    }
                }
            }
            return 0.0;
        }

        // range: { "field": { "gt": ..., "gte": ..., "lt": ..., "lte": ... } }
        if let Some(range_obj) = obj.get("range").and_then(|r| r.as_object()) {
            for (field, conditions) in range_obj {
                if let Some(field_val) = get_nested_field(doc, field)
                    && range_match(conditions, field_val)
                {
                    return 1.0;
                }
            }
            return 0.0;
        }

        // wildcard: { "field": { "value": "pattern*" } }
        if let Some(wc_obj) = obj.get("wildcard").and_then(|w| w.as_object()) {
            for (field, spec) in wc_obj {
                let pattern = spec
                    .as_str()
                    .or_else(|| spec.get("value").and_then(|v| v.as_str()))
                    .unwrap_or("");
                if let Some(field_val) = get_nested_field(doc, field) {
                    let field_str = value_to_string(field_val);
                    if wildcard_match(pattern, &field_str) {
                        return 1.0;
                    }
                }
            }
            return 0.0;
        }

        // prefix: { "field": { "value": "pre" } }
        if let Some(pre_obj) = obj.get("prefix").and_then(|p| p.as_object()) {
            for (field, spec) in pre_obj {
                let prefix_val = spec
                    .as_str()
                    .or_else(|| spec.get("value").and_then(|v| v.as_str()))
                    .unwrap_or("");
                if let Some(field_val) = get_nested_field(doc, field) {
                    let field_str = value_to_string(field_val);
                    if field_str
                        .to_lowercase()
                        .starts_with(&prefix_val.to_lowercase())
                    {
                        return 1.0;
                    }
                }
            }
            return 0.0;
        }

        // exists: { "field": "fieldName" }
        if let Some(exists_field) = obj
            .get("exists")
            .and_then(|e| e.get("field"))
            .and_then(|f| f.as_str())
        {
            return if get_nested_field(doc, exists_field).is_some() {
                1.0
            } else {
                0.0
            };
        }

        // ids: { "values": ["id1", "id2"] }
        // Filtering happens at the search loop level; all docs score 1.0.
        if obj.contains_key("ids") {
            return 1.0;
        }

        // knn: { "field": { "vector": [...], "k": 10, "filter": {...} } }
        if let Some(knn_obj) = obj.get("knn").and_then(|k| k.as_object()) {
            return knn_obj
                .iter()
                .map(|(field, spec)| {
                    let score = knn_score(field, spec, doc);
                    let cutoff = spec
                        .get(KNN_CUTOFF_KEY)
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    if score < cutoff { 0.0 } else { score }
                })
                .sum();
        }

        // bool: { "must": [...], "should": [...], "filter": [...], "must_not": [...] }
        //
        // Semantics (mirrors OpenSearch / Lucene):
        //   must     : every clause must match; contributes to score.
        //   filter   : every clause must match; no score contribution.
        //   must_not : no clause may match; no score contribution.
        //   should   : when must / filter present -> optional, additive
        //              score. Otherwise -> at least one must match.
        //   empty    : `{ "bool": {} }` (or only `must_not` clauses with
        //              no positive-match clauses) is a `match_all`
        //              baseline: every doc matches with score 1.0,
        //              gated only by the `must_not` exclusions.
        //
        // The third case is the permission-filter shape:
        //
        //   { "bool": { "should": [
        //       { "bool": { "must_not": [
        //           { "terms": { "type": ["chat","message",...] }}
        //       ] } }
        //   ] } }
        //
        // The inner bool has only `must_not`, so it must score 1.0 on
        // any doc whose `type` *isn't* in that list - otherwise the
        // outer `should` never has a match and the unified search
        // returns zero hits.
        if let Some(bool_obj) = obj.get("bool").and_then(|b| b.as_object()) {
            // must_not: any clause matches -> bool fails outright.
            if let Some(must_not) = bool_obj.get("must_not").and_then(|n| n.as_array()) {
                for clause in must_not {
                    if match_score(clause, doc) > 0.0 {
                        return 0.0;
                    }
                }
            }

            let has_must_or_filter =
                bool_obj.contains_key("must") || bool_obj.contains_key("filter");
            let has_should = bool_obj.contains_key("should");

            let mut total_score = 0.0;
            let mut must_pass = true;

            if let Some(must) = bool_obj.get("must").and_then(|m| m.as_array()) {
                for clause in must {
                    let s = match_score(clause, doc);
                    if s <= 0.0 {
                        must_pass = false;
                        break;
                    }
                    total_score += s;
                }
            }

            if let Some(filter) = bool_obj.get("filter").and_then(|f| f.as_array()) {
                for clause in filter {
                    if match_score(clause, doc) <= 0.0 {
                        must_pass = false;
                        break;
                    }
                }
            }

            if !must_pass {
                return 0.0;
            }

            let mut should_score = 0.0;
            let mut should_matched = false;
            if let Some(should) = bool_obj.get("should").and_then(|s| s.as_array()) {
                for clause in should {
                    let s = match_score(clause, doc);
                    if s > 0.0 {
                        should_matched = true;
                    }
                    should_score += s;
                }
            }

            // Bool with no positive-match clauses (`must`, `filter`,
            // `should`) acts as `match_all` minus the `must_not`
            // exclusions. We've already short-circuited above when a
            // must_not matched, so getting here means the doc passes.
            if !has_must_or_filter && !has_should {
                return 1.0;
            }

            // Only `should` (no must/filter): at least one should
            // clause must match for the bool to score.
            if !has_must_or_filter {
                return if should_matched { should_score } else { 0.0 };
            }

            // Has must/filter: should is purely additive.
            total_score += should_score;

            return if total_score > 0.0 { total_score } else { 1.0 };
        }

        // query_string: { "query": "text" }
        if let Some(qs) = obj.get("query_string").and_then(|q| q.as_object()) {
            let query_text = qs.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let doc_str = serde_json::to_string(doc)
                .unwrap_or_default()
                .to_lowercase();
            let query_lower = query_text.to_lowercase();
            return if query_lower
                .split_whitespace()
                .any(|term| doc_str.contains(term))
            {
                0.5
            } else {
                0.0
            };
        }
    }

    0.0
}

/// Check if an expected term value matches a field value.
/// Supports string, number, and boolean comparisons. Recurses into
/// JSON arrays so a `term` / `terms` query against a multi-value
/// field like `tags: ["rust", "systems"]` matches when *any*
/// element equals `expected` - which is how real OpenSearch
/// (and Lucene's term queries on `keyword`/`text` arrays) behave.
fn term_match(expected: &Value, field_val: &Value) -> bool {
    if let Some(arr) = field_val.as_array() {
        return arr.iter().any(|el| term_match(expected, el));
    }
    if let Some(s) = expected.as_str() {
        return value_to_string(field_val) == s;
    }
    if expected.is_string() {
        return value_to_string(field_val) == expected.as_str().unwrap_or("");
    }
    if let Some(n) = expected.as_f64() {
        if let Some(fn_val) = field_val.as_f64() {
            return (fn_val - n).abs() < f64::EPSILON;
        }
        let field_str = value_to_string(field_val);
        if let Ok(field_num) = field_str.parse::<f64>() {
            return (field_num - n).abs() < f64::EPSILON;
        }
    }
    if let Some(b) = expected.as_bool() {
        if let Some(fb) = field_val.as_bool() {
            return fb == b;
        }
        let field_str = value_to_string(field_val);
        return field_str == b.to_string();
    }
    // Fallback: compare string representations
    value_to_string(field_val) == value_to_string(expected)
}

/// Check range conditions against a field value.
fn range_match(conditions: &Value, field_val: &Value) -> bool {
    let cond = match conditions.as_object() {
        Some(o) => o,
        None => return false,
    };

    let field_f64 = field_val.as_f64();
    let field_str = value_to_string(field_val);

    for (op, threshold) in cond {
        let thresh_f64 = threshold.as_f64();
        let thresh_str = value_to_string(threshold);

        let passed = match op.as_str() {
            "gt" => {
                if let (Some(f), Some(t)) = (field_f64, thresh_f64) {
                    f > t
                } else {
                    field_str > thresh_str
                }
            }
            "gte" => {
                if let (Some(f), Some(t)) = (field_f64, thresh_f64) {
                    f >= t
                } else {
                    field_str >= thresh_str
                }
            }
            "lt" => {
                if let (Some(f), Some(t)) = (field_f64, thresh_f64) {
                    f < t
                } else {
                    field_str < thresh_str
                }
            }
            "lte" => {
                if let (Some(f), Some(t)) = (field_f64, thresh_f64) {
                    f <= t
                } else {
                    field_str <= thresh_str
                }
            }
            _ => true,
        };
        if !passed {
            return false;
        }
    }
    true
}

/// Simple text matching score.
fn text_match_score(query: &str, field: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let field_lower = field.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();

    if terms.is_empty() {
        return 0.0;
    }

    let matched = terms
        .iter()
        .filter(|term| field_lower.contains(*term))
        .count();

    if matched == 0 {
        return 0.0;
    }

    (matched as f64) / (terms.len() as f64)
}

/// Sort hits by the `sort` specification from the query body.
///
/// Supports array format: `["field1", {"field2": "asc"}]`
/// and the special `_score` / `_doc` sort keys.
fn sort_hits(hits: &mut [Value], sort_spec: &Value) {
    let sort_keys: Vec<(String, bool)> = if let Some(arr) = sort_spec.as_array() {
        arr.iter()
            .map(|entry| {
                if let Some(s) = entry.as_str() {
                    (s.to_string(), false) // default asc for string entries
                } else if let Some(obj) = entry.as_object() {
                    if let Some((field, order)) = obj.iter().next() {
                        let asc = order.as_str().map(|o| o == "asc").unwrap_or(false);
                        (field.clone(), asc)
                    } else {
                        ("_score".to_string(), false)
                    }
                } else {
                    ("_score".to_string(), false)
                }
            })
            .collect()
    } else if let Some(s) = sort_spec.as_str() {
        vec![(s.to_string(), false)]
    } else {
        return;
    };

    hits.sort_by(|a, b| {
        for (key, asc) in &sort_keys {
            let va = get_sort_value(a, key);
            let vb = get_sort_value(b, key);
            let cmp = compare_sort_values(&va, &vb);
            let ord = if *asc { cmp.reverse() } else { cmp };
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });

    // Set _score to null when sorting by non-score fields
    if !sort_keys.is_empty() && sort_keys[0].0 != "_score" {
        for hit in hits.iter_mut() {
            if let Some(obj) = hit.as_object_mut() {
                obj.insert("_score".to_string(), Value::Null);
            }
        }
    }
}

fn get_sort_value(hit: &Value, key: &str) -> Value {
    match key {
        "_score" => hit["_score"].clone(),
        "_doc" => json!(0),
        _ => hit
            .get("_source")
            .and_then(|s| get_nested_field(s, key))
            .cloned()
            .unwrap_or(Value::Null),
    }
}

fn compare_sort_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        (Value::Null, _) => std::cmp::Ordering::Greater,
        (_, Value::Null) => std::cmp::Ordering::Less,
        _ => {
            let sa = value_to_string(a);
            let sb = value_to_string(b);
            if let (Some(na), Some(nb)) = (a.as_f64(), b.as_f64()) {
                nb.partial_cmp(&na).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                sb.cmp(&sa)
            }
        }
    }
}

/// Get a nested field value from a JSON document using dot notation.
fn get_nested_field<'a>(doc: &'a Value, field: &str) -> Option<&'a Value> {
    let mut current = doc;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Field collapsing: keep only the best (first, since `hits` is already
/// sorted) hit per value of `field`, and expose that value under `fields`
/// like OpenSearch does. Docs missing the field form a single group.
fn collapse_hits(hits: &mut Vec<Value>, field: &str) {
    let mut seen = HashSet::new();
    hits.retain_mut(|hit| {
        let value = get_nested_field(&hit["_source"], field)
            .cloned()
            .unwrap_or(Value::Null);
        if !seen.insert(value.to_string()) {
            return false;
        }
        if let Some(obj) = hit.as_object_mut() {
            obj.insert("fields".to_string(), json!({ field: [value] }));
        }
        true
    });
}

/// Apply a request's `_source` option to a hit's source. Returns `None`
/// when the source should be omitted (`"_source": false`).
///
/// Accepts `true`/`false`, a field pattern or array of patterns
/// (includes), or `{ "includes": ..., "excludes": ... }` (also the
/// legacy singular `include` / `exclude`). Patterns are dotted paths and
/// may contain `*` wildcards.
fn filter_source(source: Value, spec: Option<&Value>) -> Option<Value> {
    fn patterns(v: Option<&Value>) -> Vec<&str> {
        match v {
            Some(Value::String(s)) => vec![s.as_str()],
            Some(Value::Array(arr)) => arr.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        }
    }

    let (includes, excludes) = match spec {
        None | Some(Value::Bool(true)) => return Some(source),
        Some(Value::Bool(false)) => return None,
        Some(Value::Object(o)) => (
            patterns(o.get("includes").or_else(|| o.get("include"))),
            patterns(o.get("excludes").or_else(|| o.get("exclude"))),
        ),
        other => (patterns(other), Vec::new()),
    };
    match source {
        Value::Object(obj) => Some(Value::Object(filter_fields(&obj, "", &includes, &excludes))),
        other => Some(other),
    }
}

/// Recursive worker for [`filter_source`]. An included object keeps its
/// whole subtree (minus excludes); an object that is not itself included
/// is kept only if some descendant is.
fn filter_fields(
    obj: &Map<String, Value>,
    prefix: &str,
    includes: &[&str],
    excludes: &[&str],
) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in obj {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if excludes.iter().any(|p| wildcard_match(p, &path)) {
            continue;
        }
        let included = includes.is_empty() || includes.iter().any(|p| wildcard_match(p, &path));
        if let Value::Object(child) = value {
            let child_includes = if included { &[][..] } else { includes };
            let filtered = filter_fields(child, &path, child_includes, excludes);
            if included || !filtered.is_empty() {
                out.insert(key.clone(), Value::Object(filtered));
            }
        } else if included {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

/// Internal key stamped onto each `knn` clause by [`apply_knn_cutoffs`]:
/// the score of the clause's k-th nearest neighbour in the index.
const KNN_CUTOFF_KEY: &str = "__awsim_knn_cutoff";

/// Stamp every `knn` clause in `query` with the score of its k-th
/// nearest neighbour in `index`, so `match_score` only keeps the top-k
/// docs. This mirrors the k-NN plugin, which picks the top-k (honouring
/// the clause's own `filter`) before any enclosing `bool` combines or
/// post-filters them.
fn apply_knn_cutoffs(state: &OpenSearchState, index: &str, query: &mut Value) {
    match query {
        Value::Object(obj) => {
            if let Some(knn_obj) = obj.get_mut("knn").and_then(|k| k.as_object_mut()) {
                for (field, spec) in knn_obj.iter_mut() {
                    let k = spec.get("k").and_then(Value::as_u64).unwrap_or(10) as usize;
                    let mut scores = Vec::new();
                    let _ = state.for_each_doc(index, |_, doc| {
                        let score = knn_score(field, spec, doc);
                        if score > 0.0 {
                            scores.push(score);
                        }
                        true
                    });
                    let nth = k.saturating_sub(1);
                    let cutoff = if scores.len() > nth {
                        *scores.select_nth_unstable_by(nth, |a, b| b.total_cmp(a)).1
                    } else {
                        0.0
                    };
                    if let Some(spec) = spec.as_object_mut() {
                        spec.insert(KNN_CUTOFF_KEY.to_string(), json!(cutoff));
                    }
                }
                return;
            }
            for value in obj.values_mut() {
                apply_knn_cutoffs(state, index, value);
            }
        }
        Value::Array(arr) => {
            for value in arr {
                apply_knn_cutoffs(state, index, value);
            }
        }
        _ => {}
    }
}

/// Similarity of `doc` to a `knn` clause spec, or 0.0 when the doc has no
/// comparable vector or fails the clause's `filter`.
///
/// Score is `(1 + cos) / 2` so the result lands in `[0, 1]` like the
/// real k-NN plugin's normalised score, and unrelated vectors don't
/// produce negative scores that would be filtered downstream.
fn knn_score(field: &str, spec: &Value, doc: &Value) -> f64 {
    let Some(vector) = spec.get("vector").and_then(extract_vector) else {
        return 0.0;
    };
    let Some(doc_vec) = get_nested_field(doc, field).and_then(extract_vector) else {
        return 0.0;
    };
    if vector.is_empty() || doc_vec.len() != vector.len() {
        return 0.0;
    }
    if let Some(filter) = spec.get("filter")
        && match_score(filter, doc) <= 0.0
    {
        return 0.0;
    }
    (1.0 + cosine_similarity(&vector, &doc_vec)) / 2.0
}

fn extract_vector(v: &Value) -> Option<Vec<f64>> {
    let arr = v.as_array()?;
    let out: Vec<f64> = arr.iter().filter_map(|n| n.as_f64()).collect();
    if out.len() == arr.len() {
        Some(out)
    } else {
        None
    }
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Convert a Value to a searchable string.
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(arr) => arr
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Object(obj) => obj
            .values()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Null => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::IndexMeta;

    fn test_state() -> OpenSearchState {
        let state = OpenSearchState::ephemeral().expect("ephemeral state");
        state
            .create_index_meta(
                "articles",
                IndexMeta {
                    mappings: json!({}),
                    settings: json!({}),
                    created_at: "2026-01-01".to_string(),
                    uuid: "test-uuid".to_string(),
                },
            )
            .unwrap();
        state
            .put_doc("articles", "1", &json!({"title": "Rust Programming", "body": "Learn Rust for systems programming", "tags": ["rust", "systems"]}))
            .unwrap();
        state
            .put_doc("articles", "2", &json!({"title": "Python Guide", "body": "Python is great for data science", "tags": ["python", "data"]}))
            .unwrap();
        state
            .put_doc("articles", "3", &json!({"title": "AWS Lambda", "body": "Serverless computing with AWS Lambda and Rust", "tags": ["aws", "lambda", "rust"]}))
            .unwrap();
        state
    }

    #[test]
    fn test_match_all() {
        let state = test_state();
        let (status, result) = search(&state, "articles", &json!({"query": {"match_all": {}}}));
        assert_eq!(status, 200);
        assert_eq!(result["hits"]["total"]["value"], 3);
    }

    #[test]
    fn test_match_query() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({"query": {"match": {"title": "Rust"}}}),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert!(
            hits.iter()
                .any(|h| h["_source"]["title"].as_str().unwrap().contains("Rust"))
        );
    }

    #[test]
    fn test_multi_match() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({
                "query": {"multi_match": {"query": "Rust", "fields": ["title^2", "body"]}}
            }),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 2); // "Rust Programming" and "AWS Lambda" (body mentions Rust)
    }

    #[test]
    fn test_bool_must() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({
                "query": {"bool": {"must": [{"match": {"body": "Rust"}}, {"match": {"body": "Lambda"}}]}}
            }),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["_source"]["title"], "AWS Lambda");
    }

    /// `bool` with only `must_not` is `match_all` minus the exclusions.
    /// Three docs in the corpus, two have `rust` in tags - the
    /// `must_not: [terms tags=[rust]]` should leave the Python doc
    /// only.
    #[test]
    fn test_bool_must_not_only_acts_as_match_all_minus_exclusions() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({
                "query": {
                    "bool": {
                        "must_not": [
                            { "terms": { "tags": ["rust"] } }
                        ]
                    }
                }
            }),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1, "only the Python doc should remain");
        assert_eq!(hits[0]["_source"]["title"], "Python Guide");
    }

    /// Permission-filter shape: `bool { should: [bool { must_not: [...] }] }`.
    /// Before the fix the inner bool returned 0 (no must / filter /
    /// should) and the outer should never matched, so the unified
    /// search returned zero hits regardless of doc contents.
    #[test]
    fn test_bool_should_with_nested_must_not_returns_excluded_docs() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({
                "query": {
                    "bool": {
                        "should": [
                            {
                                "bool": {
                                    "must_not": [
                                        { "terms": { "tags": ["python", "data"] } }
                                    ]
                                }
                            }
                        ]
                    }
                }
            }),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        let titles: Vec<&str> = hits
            .iter()
            .map(|h| h["_source"]["title"].as_str().unwrap())
            .collect();
        assert_eq!(hits.len(), 2, "non-Python docs should match");
        assert!(titles.contains(&"Rust Programming"));
        assert!(titles.contains(&"AWS Lambda"));
    }

    /// `must_not` short-circuits regardless of whether other clauses
    /// match: a doc that satisfies the `must` but is in the
    /// exclusion list must not be returned.
    #[test]
    fn test_bool_must_not_excludes_even_when_must_matches() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({
                "query": {
                    "bool": {
                        "must":     [{ "match": { "body": "Rust" } }],
                        "must_not": [{ "terms": { "tags": ["lambda"] } }],
                    }
                }
            }),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["_source"]["title"], "Rust Programming");
    }

    #[test]
    fn test_wildcard_index() {
        let state = test_state();
        let (_, result) = search(&state, "art*", &json!({"query": {"match_all": {}}}));
        assert_eq!(result["hits"]["total"]["value"], 3);
    }

    #[test]
    fn test_pagination() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({"query": {"match_all": {}}, "size": 2, "from": 0}),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(result["hits"]["total"]["value"], 3);
    }

    /// Brute-force k-NN: query `[1,0,0]` should rank the identical
    /// vector first, the orthogonal one last.
    #[test]
    fn test_knn_search() {
        let state = OpenSearchState::ephemeral().expect("ephemeral");
        state
            .create_index_meta(
                "vecs",
                IndexMeta {
                    mappings: json!({}),
                    settings: json!({}),
                    created_at: "2026-01-01".to_string(),
                    uuid: "test-uuid-vecs".to_string(),
                },
            )
            .unwrap();
        state
            .put_doc("vecs", "a", &json!({"embedding": [1.0, 0.0, 0.0]}))
            .unwrap();
        state
            .put_doc("vecs", "b", &json!({"embedding": [0.9, 0.1, 0.0]}))
            .unwrap();
        state
            .put_doc("vecs", "c", &json!({"embedding": [0.0, 1.0, 0.0]}))
            .unwrap();

        let (_, result) = search(
            &state,
            "vecs",
            &json!({
                "query": {"knn": {"embedding": {"vector": [1.0, 0.0, 0.0], "k": 3}}}
            }),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0]["_id"], "a");
        assert_eq!(hits[1]["_id"], "b");
        assert_eq!(hits[2]["_id"], "c");
        // Identical vector -> cosine = 1.0 -> score = 1.0
        let top = hits[0]["_score"].as_f64().unwrap();
        assert!((top - 1.0).abs() < 1e-9, "top score {} != 1.0", top);
    }

    fn tenant_vec_state() -> OpenSearchState {
        let state = OpenSearchState::ephemeral().expect("ephemeral");
        state
            .create_index_meta(
                "chunks",
                IndexMeta {
                    mappings: json!({}),
                    settings: json!({}),
                    created_at: "2026-01-01".to_string(),
                    uuid: "test-uuid-chunks".to_string(),
                },
            )
            .unwrap();
        for (id, tenant, embedding, text) in [
            ("a", "t1", [1.0, 0.0, 0.0], "alpha"),
            ("b", "t1", [0.9, 0.1, 0.0], "bravo"),
            ("c", "t2", [0.95, 0.05, 0.0], "charlie"),
            ("d", "t1", [0.0, 1.0, 0.0], "delta"),
        ] {
            state
                .put_doc(
                    "chunks",
                    id,
                    &json!({"tenantId": tenant, "embedding": embedding, "text": text}),
                )
                .unwrap();
        }
        state
    }

    fn hit_ids(result: &Value) -> Vec<&str> {
        result["hits"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["_id"].as_str().unwrap())
            .collect()
    }

    /// `knn` inside `bool.must` with a `bool.filter` is the usual
    /// filtered vector search; before the fix it matched nothing.
    #[test]
    fn test_knn_in_bool_must_with_filter() {
        let state = tenant_vec_state();
        let (_, result) = search(
            &state,
            "chunks",
            &json!({"query": {"bool": {
                "must": [{"knn": {"embedding": {"vector": [1.0, 0.0, 0.0], "k": 4}}}],
                "filter": [{"term": {"tenantId": "t1"}}],
            }}}),
        );
        assert_eq!(hit_ids(&result), vec!["a", "b", "d"]);
    }

    /// A `bool.filter` is applied after the top-k is picked, so it can
    /// leave fewer than `k` hits.
    #[test]
    fn test_knn_in_bool_is_post_filtered() {
        let state = tenant_vec_state();
        let (_, result) = search(
            &state,
            "chunks",
            &json!({"query": {"bool": {
                "must": [{"knn": {"embedding": {"vector": [1.0, 0.0, 0.0], "k": 2}}}],
                "filter": [{"term": {"tenantId": "t1"}}],
            }}}),
        );
        assert_eq!(hit_ids(&result), vec!["a"]);
    }

    /// The clause's own `filter` is applied before the top-k is picked.
    #[test]
    fn test_knn_clause_filter_is_pre_filtered() {
        let state = tenant_vec_state();
        let (_, result) = search(
            &state,
            "chunks",
            &json!({"query": {"knn": {"embedding": {
                "vector": [1.0, 0.0, 0.0],
                "k": 2,
                "filter": {"bool": {"filter": [{"term": {"tenantId": "t1"}}]}},
            }}}}),
        );
        assert_eq!(hit_ids(&result), vec!["a", "b"]);
    }

    /// Hybrid shape: a doc matches through either the text clause or
    /// the kNN top-k, and docs outside both are excluded.
    #[test]
    fn test_knn_in_bool_should_hybrid() {
        let state = tenant_vec_state();
        let (_, result) = search(
            &state,
            "chunks",
            &json!({"query": {"bool": {
                "should": [
                    {"match": {"text": "delta"}},
                    {"knn": {"embedding": {"vector": [1.0, 0.0, 0.0], "k": 1}}},
                ],
                "minimum_should_match": 1,
            }}}),
        );
        let mut ids = hit_ids(&result);
        ids.sort();
        assert_eq!(ids, vec!["a", "d"]);
    }

    /// Chunked-document shape: several chunks share a `collapseId`, and
    /// only the best-scoring chunk per document comes back.
    #[test]
    fn test_collapse_keeps_best_hit_per_value() {
        let state = OpenSearchState::ephemeral().expect("ephemeral");
        state
            .create_index_meta(
                "docs",
                IndexMeta {
                    mappings: json!({}),
                    settings: json!({}),
                    created_at: "2026-01-01".to_string(),
                    uuid: "test-uuid-docs".to_string(),
                },
            )
            .unwrap();
        for (id, doc, rank) in [
            ("d1_0", "d1", 3),
            ("d1_1", "d1", 5),
            ("d2_0", "d2", 4),
            ("x", "", 1),
        ] {
            let mut source = json!({"rank": rank});
            if !doc.is_empty() {
                source["collapseId"] = json!(doc);
            }
            state.put_doc("docs", id, &source).unwrap();
        }

        let (_, result) = search(
            &state,
            "docs",
            &json!({
                "query": {"match_all": {}},
                "sort": [{"rank": "desc"}],
                "collapse": {"field": "collapseId"},
            }),
        );
        assert_eq!(hit_ids(&result), vec!["d1_1", "d2_0", "x"]);
        assert_eq!(result["hits"]["total"]["value"], 4);
        assert_eq!(
            result["hits"]["hits"][0]["fields"]["collapseId"],
            json!(["d1"])
        );
    }

    #[test]
    fn test_source_false_omits_source() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({"query": {"match_all": {}}, "_source": false}),
        );
        let hits = result["hits"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 3);
        assert!(hits.iter().all(|h| h.get("_source").is_none()));
    }

    #[test]
    fn test_source_includes_array() {
        let state = test_state();
        let (_, result) = search(
            &state,
            "articles",
            &json!({"query": {"ids": {"values": ["1"]}}, "_source": ["title", "tag*"]}),
        );
        assert_eq!(
            result["hits"]["hits"][0]["_source"],
            json!({"title": "Rust Programming", "tags": ["rust", "systems"]})
        );
    }

    #[test]
    fn test_source_includes_and_excludes_nested_paths() {
        let source = json!({
            "id": "1",
            "embedding": [0.1, 0.2],
            "meta": {"owner": "u1", "secret": "s", "tags": {"a": 1}},
            "other": {"x": 1},
        });

        assert_eq!(
            filter_source(source.clone(), Some(&json!({"excludes": ["embedding"]}))),
            Some(
                json!({"id": "1", "meta": {"owner": "u1", "secret": "s", "tags": {"a": 1}}, "other": {"x": 1}})
            )
        );
        assert_eq!(
            filter_source(
                source.clone(),
                Some(&json!({"includes": ["meta"], "excludes": ["meta.secret"]}))
            ),
            Some(json!({"meta": {"owner": "u1", "tags": {"a": 1}}}))
        );
        assert_eq!(
            filter_source(source, Some(&json!({"include": "meta.tags.*"}))),
            Some(json!({"meta": {"tags": {"a": 1}}}))
        );
    }
}
