//! DynamoDB reserved-word validation for expression attribute names.
//!
//! AWS rejects a reserved keyword used as a bare attribute name in any
//! expression (Condition, Filter, KeyCondition, Projection, Update); the caller
//! must alias it through `ExpressionAttributeNames` (`#alias`). This module
//! enforces that at parse time.
//!
//! The keyword list is vendored in `reserved_words.txt` (one word per line;
//! blank lines and lines beginning with `#` are ignored) and embedded at
//! compile time, so the canonical AWS list can be updated as data without
//! touching code.

use awsim_core::AwsError;

const RESERVED_WORDS: &str = include_str!("reserved_words.txt");

/// Iterate the vendored keywords, skipping blanks and `#` comment lines.
fn words() -> impl Iterator<Item = &'static str> {
    RESERVED_WORDS
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

/// True when `word` is a DynamoDB reserved keyword (case-insensitive).
pub fn is_reserved(word: &str) -> bool {
    words().any(|w| w.eq_ignore_ascii_case(word))
}

/// Reject any bare attribute-name segment in `path` that is a reserved keyword.
///
/// Segments beginning with `#` are `ExpressionAttributeNames` aliases and are
/// exempt. A trailing list index (`items[0]`) is ignored for the name check.
/// Runs at parse time, so the request is rejected regardless of whether any
/// stored item would have matched.
pub fn check_path(path: &str) -> Result<(), AwsError> {
    for segment in path.split('.') {
        if segment.starts_with('#') {
            continue;
        }
        let name = segment.split('[').next().unwrap_or(segment).trim();
        if !name.is_empty() && is_reserved(name) {
            return Err(AwsError::validation(format!(
                "Invalid expression: Attribute name is a reserved keyword; \
                 reserved keyword: {name}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_word_is_detected_case_insensitively() {
        // These are in the seed list; the check must be case-insensitive.
        assert!(is_reserved("SIZE"));
        assert!(is_reserved("size"));
        assert!(is_reserved("Status"));
        assert!(!is_reserved("my_attribute"));
    }

    #[test]
    fn check_path_rejects_bare_reserved_word() {
        let err = check_path("Size").unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn check_path_allows_aliased_and_nested_non_reserved() {
        // Aliased segments are exempt even when they map to a reserved word.
        assert!(check_path("#s").is_ok());
        assert!(check_path("profile.address").is_ok());
        // A reserved word in a nested position is still rejected.
        assert!(check_path("profile.Size").is_err());
        // List index is stripped before the name check.
        assert!(check_path("tags[0]").is_ok());
    }
}
