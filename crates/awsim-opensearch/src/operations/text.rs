//! Text analysis for the `match` family of queries: their terms, typo tolerance and the
//! highlighting of what they matched.

use serde_json::{Map, Value};

/// A query's tolerance of typos, as its `fuzziness` and `prefix_length` ask.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fuzziness {
    auto: bool,
    edits: usize,
    prefix: usize,
}

impl Fuzziness {
    /// The tolerance a `match` or `multi_match` body asks, or none without `fuzziness`.
    /// `AUTO` (with or without its `:low,high` bounds) allows no edit to a term of up to two
    /// characters, one up to five, and two beyond, as OpenSearch's default bounds do.
    pub fn of(spec: &Map<String, Value>) -> Option<Self> {
        let prefix = spec
            .get("prefix_length")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let (auto, edits) = match spec.get("fuzziness")? {
            Value::String(s) if s.to_ascii_uppercase().starts_with("AUTO") => (true, 0),
            Value::String(s) => (false, s.parse().ok()?),
            Value::Number(n) => (false, n.as_u64()? as usize),
            _ => return None,
        };
        Some(Self {
            auto,
            edits: edits.min(2),
            prefix,
        })
    }

    fn edits_for(&self, term: &[char]) -> usize {
        if !self.auto {
            return self.edits;
        }
        match term.len() {
            0..=2 => 0,
            3..=5 => 1,
            _ => 2,
        }
    }
}

/// A text's terms as the standard analyzer finds them: lower-cased runs of letters and digits,
/// so punctuation and the backslashes of an escaped query separate terms.
pub fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// How well `field` matches `query`: the share of the query's terms among the field's, compared
/// whole as the standard analyzer compares them. A term only within `fuzziness` edits of one of
/// the field's counts half.
pub fn match_score(query: &str, field: &str, fuzziness: Option<Fuzziness>) -> f64 {
    let asked = terms(query);
    if asked.is_empty() {
        return 0.0;
    }
    let words = terms(field);
    let found: f64 = asked
        .iter()
        .map(|term| {
            if words.contains(term) {
                1.0
            } else if fuzziness.is_some_and(|f| words.iter().any(|w| near(term, w, f))) {
                0.5
            } else {
                0.0
            }
        })
        .sum();
    found / asked.len() as f64
}

/// Whether `word` is within the edits `fuzziness` allows of `term`, sharing its prefix.
fn near(term: &str, word: &str, fuzziness: Fuzziness) -> bool {
    let (t, w): (Vec<char>, Vec<char>) = (term.chars().collect(), word.chars().collect());
    let edits = fuzziness.edits_for(&t);
    if edits == 0 || t.len().abs_diff(w.len()) > edits {
        return false;
    }
    let prefix = fuzziness.prefix.min(t.len());
    w.len() >= prefix && t[..prefix] == w[..prefix] && distance(&t, &w) <= edits
}

/// The optimal string alignment distance: insertions, deletions, substitutions and swaps of
/// neighbours, each one edit, as OpenSearch's fuzzy matching counts transpositions.
fn distance(a: &[char], b: &[char]) -> usize {
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}

/// A hit's `highlight`: for each field `body.highlight` names whose text the query's words reach,
/// its fragments with each matching word between the pre and post tags. None when nothing matches.
pub fn highlight(body: &Value, query: &Value, source: &Value) -> Option<Value> {
    let spec = body.get("highlight")?.as_object()?;
    let mut asked = Vec::new();
    let mut fuzziness = None;
    query_texts(query, &mut asked, &mut fuzziness);
    if asked.is_empty() {
        return None;
    }
    let tag = |key: &str, default: &str| {
        spec.get(key)
            .and_then(Value::as_array)
            .and_then(|tags| tags.first())
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_string()
    };
    let tags = (tag("pre_tags", "<em>"), tag("post_tags", "</em>"));
    let mut found = Map::new();
    for (name, options) in highlighted_fields(spec.get("fields")) {
        let option = |key: &str, default: u64| {
            options
                .get(key)
                .or_else(|| spec.get(key))
                .and_then(Value::as_u64)
                .unwrap_or(default) as usize
        };
        let (number, size) = (
            option("number_of_fragments", 5),
            option("fragment_size", 100),
        );
        let texts: Vec<&str> = match field(source, &name) {
            Some(Value::String(s)) => vec![s.as_str()],
            Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
            _ => continue,
        };
        let limit = if number == 0 { usize::MAX } else { number };
        let fragments: Vec<Value> = texts
            .iter()
            .flat_map(|text| fragments(text, &asked, fuzziness, number, size, &tags))
            .take(limit)
            .map(Value::String)
            .collect();
        if !fragments.is_empty() {
            found.insert(name, Value::Array(fragments));
        }
    }
    (!found.is_empty()).then_some(Value::Object(found))
}

/// The terms of every text a query matches on, with the first typo tolerance it asks. Clauses
/// that only exclude (`must_not`) or filter are left out, as they highlight nothing.
fn query_texts(query: &Value, asked: &mut Vec<String>, fuzziness: &mut Option<Fuzziness>) {
    let Some(obj) = query.as_object() else {
        if let Some(clauses) = query.as_array() {
            for clause in clauses {
                query_texts(clause, asked, fuzziness);
            }
        }
        return;
    };
    for (key, value) in obj {
        match key.as_str() {
            "must_not" | "filter" => {}
            "multi_match" | "query_string" => {
                if let Some(spec) = value.as_object() {
                    if let Some(text) = spec.get("query").and_then(Value::as_str) {
                        asked.extend(terms(text));
                    }
                    *fuzziness = fuzziness.or_else(|| Fuzziness::of(spec));
                }
            }
            "match" | "match_phrase" => {
                for spec in value.as_object().into_iter().flat_map(Map::values) {
                    match spec {
                        Value::String(text) => asked.extend(terms(text)),
                        Value::Object(spec) => {
                            if let Some(text) = spec.get("query").and_then(Value::as_str) {
                                asked.extend(terms(text));
                            }
                            *fuzziness = fuzziness.or_else(|| Fuzziness::of(spec));
                        }
                        _ => {}
                    }
                }
            }
            _ => query_texts(value, asked, fuzziness),
        }
    }
}

/// The fields a highlight names, each with its own options, from either the object or the array
/// form of `fields`. Wildcard names are left out.
fn highlighted_fields(fields: Option<&Value>) -> Vec<(String, Map<String, Value>)> {
    let entries: Vec<(&String, &Value)> = match fields {
        Some(Value::Object(map)) => map.iter().collect(),
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(Value::as_object)
            .flat_map(|map| map.iter())
            .collect(),
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .filter(|(name, _)| !name.contains('*'))
        .map(|(name, options)| {
            (
                name.clone(),
                options.as_object().cloned().unwrap_or_default(),
            )
        })
        .collect()
}

fn field<'a>(source: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(source, |current, part| current.get(part))
}

/// The fragments of `text` holding a word the query matches, each of about `size` characters
/// starting a little before its first match, at most `number` of them, or the whole text marked
/// when `number` is 0 or the text fits in one fragment.
fn fragments(
    text: &str,
    asked: &[String],
    fuzziness: Option<Fuzziness>,
    number: usize,
    size: usize,
    (pre, post): &(String, String),
) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if !chars[at].is_alphanumeric() {
            at += 1;
            continue;
        }
        let start = at;
        while at < chars.len() && chars[at].is_alphanumeric() {
            at += 1;
        }
        let word: String = chars[start..at].iter().collect::<String>().to_lowercase();
        if asked
            .iter()
            .any(|term| *term == word || fuzziness.is_some_and(|f| near(term, &word, f)))
        {
            spans.push((start, at));
        }
    }
    if spans.is_empty() {
        return Vec::new();
    }
    let mark = |from: usize, to: usize| {
        let mut out = String::new();
        for (i, c) in chars.iter().enumerate().take(to).skip(from) {
            if spans.iter().any(|&(s, _)| s == i) {
                out.push_str(pre);
            }
            out.push(*c);
            if spans.iter().any(|&(_, e)| e == i + 1) {
                out.push_str(post);
            }
        }
        out
    };
    if number == 0 || chars.len() <= size {
        return vec![mark(0, chars.len())];
    }
    let mut out = Vec::new();
    let mut covered = 0;
    for &(first, _) in &spans {
        if out.len() == number {
            break;
        }
        if first < covered {
            continue;
        }
        // Back to the sentence the match is in, at most half a fragment, and never mid-word.
        let floor = first.saturating_sub(size / 2).max(covered);
        let mut from = first;
        while from > floor && !matches!(chars[from - 1], '.' | '!' | '?' | '\n') {
            from -= 1;
        }
        if from == floor && from > 0 && chars[from - 1].is_alphanumeric() {
            while from < first && chars[from].is_alphanumeric() {
                from += 1;
            }
        }
        while from < first && !chars[from].is_alphanumeric() {
            from += 1;
        }
        let mut to = (from + size).min(chars.len());
        while to < chars.len() && chars[to - 1].is_alphanumeric() && chars[to].is_alphanumeric() {
            to += 1;
        }
        out.push(mark(from, to).trim().to_string());
        covered = to;
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn auto(prefix: u64) -> Option<Fuzziness> {
        Fuzziness::of(
            json!({"fuzziness": "AUTO", "prefix_length": prefix})
                .as_object()
                .unwrap(),
        )
    }

    #[test]
    fn terms_drop_punctuation_and_escapes() {
        assert_eq!(terms(r"CAP\-4 Login"), ["cap", "4", "login"]);
    }

    #[test]
    fn fuzziness_allows_edits_by_length_and_keeps_the_prefix() {
        assert_eq!(match_score("lgoin", "Login form", None), 0.0);
        assert_eq!(match_score("lgoin", "Login form", auto(0)), 0.5);
        assert_eq!(match_score("lgoin", "Login form", auto(2)), 0.0);
        assert_eq!(match_score("loign", "Login form", auto(2)), 0.5);
        assert_eq!(match_score("fr", "for", auto(0)), 0.0);
        assert_eq!(
            match_score("valdation", "Login form validation", auto(2)),
            0.5
        );
        assert_eq!(match_score("form login", "Login form", auto(2)), 1.0);
        let one = Fuzziness::of(json!({"fuzziness": 1}).as_object().unwrap());
        assert_eq!(match_score("lgoinn", "login", one), 0.0);
    }

    #[test]
    fn highlights_matching_words_in_fragments() {
        let body = json!({"highlight": {
            "pre_tags": ["<mark>"], "post_tags": ["</mark>"],
            "fields": {"title": {"number_of_fragments": 1}, "content": {"fragment_size": 30, "number_of_fragments": 2}, "missing": {}}
        }});
        let query = json!({"bool": {
            "must": [{"multi_match": {"query": r"CAP\-4 logn", "fields": ["title", "content"], "fuzziness": "AUTO", "prefix_length": 2}}],
            "filter": [{"term": {"tenantId": "acme"}}]
        }});
        let source = json!({
            "title": "CAP-4 Login form validation",
            "content": "The login form must check every field. Nothing else here at all. Then login again to see it."
        });
        let found = highlight(&body, &query, &source).unwrap();
        assert_eq!(
            found["title"],
            json!(["<mark>CAP</mark>-<mark>4</mark> <mark>Login</mark> form validation"])
        );
        assert_eq!(
            found["content"],
            json!([
                "The <mark>login</mark> form must check every",
                "Then <mark>login</mark> again to see it."
            ])
        );
        assert!(found.get("missing").is_none());
        assert_eq!(highlight(&json!({}), &query, &source), None);
        assert_eq!(highlight(&body, &json!({"match_all": {}}), &source), None);
    }
}
