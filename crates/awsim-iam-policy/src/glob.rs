pub fn matches(pattern: &str, value: &str) -> bool {
    glob_match(pattern.as_bytes(), value.as_bytes(), None)
}

/// AWS-style ARN match, used by `ArnLike`/`ArnNotLike` and by Resource matching.
///
/// AWS documents this as: "Each of the six colon-delimited components of the
/// ARN is checked separately and each can include multi-character match
/// wildcards (*) or single-character match wildcards (?)." Splitting once
/// into the standard six components (`arn:partition:service:region:account:resource`)
/// and matching them independently captures that semantics: a wildcard in
/// the account segment never reaches into the resource segment, but a
/// wildcard inside the resource segment is still free to span any colons
/// or slashes that the resource itself contains (some services pack `:`
/// into their resource id).
///
/// If the pattern is not shaped like an ARN at all (for example bare `*`,
/// or a pattern with fewer than five colons) we fall back to unrestricted
/// matching so existing universal-wildcard policies keep working.
pub fn matches_arn(pattern: &str, arn: &str) -> bool {
    let p_parts: Vec<&str> = pattern.splitn(6, ':').collect();
    let v_parts: Vec<&str> = arn.splitn(6, ':').collect();
    if p_parts.len() != 6 || v_parts.len() != 6 {
        return glob_match(pattern.as_bytes(), arn.as_bytes(), None);
    }
    p_parts
        .iter()
        .zip(v_parts.iter())
        .all(|(p, v)| glob_match(p.as_bytes(), v.as_bytes(), None))
}

/// Standard backtracking glob: `*` matches any sequence, `?` matches any
/// single byte. The `_delim` parameter is reserved for future callers that
/// want stricter matching; ARN segment-awareness lives in [`matches_arn`]
/// instead, since the AWS rule is "split by colon, then glob each segment"
/// which is cleaner expressed at the split level than as a delimiter ban
/// on the inner matcher.
fn glob_match(pattern: &[u8], value: &[u8], _delim: Option<u8>) -> bool {
    let mut p = 0usize;
    let mut v = 0usize;
    let mut star_p: Option<usize> = None;
    let mut star_v: usize = 0;
    while v < value.len() {
        if p < pattern.len() {
            match pattern[p] {
                b'*' => {
                    star_p = Some(p);
                    star_v = v;
                    p += 1;
                    continue;
                }
                b'?' => {
                    p += 1;
                    v += 1;
                    continue;
                }
                c if c == value[v] => {
                    p += 1;
                    v += 1;
                    continue;
                }
                _ => {}
            }
        }
        if let Some(sp) = star_p {
            p = sp + 1;
            star_v += 1;
            v = star_v;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact() {
        assert!(matches("foo", "foo"));
        assert!(!matches("foo", "fooo"));
    }

    #[test]
    fn star() {
        assert!(matches("foo*", "foobar"));
        assert!(matches("*bar", "foobar"));
        assert!(matches("f*r", "foobar"));
        assert!(matches("*", "anything"));
        assert!(matches("**", "anything"));
    }

    #[test]
    fn question() {
        assert!(matches("f?o", "foo"));
        assert!(!matches("f?o", "fooo"));
    }

    #[test]
    fn arn_match() {
        assert!(matches_arn(
            "arn:aws:s3:::bucket/*",
            "arn:aws:s3:::bucket/key"
        ));
        assert!(!matches_arn(
            "arn:aws:s3:::other/*",
            "arn:aws:s3:::bucket/key"
        ));
    }

    #[test]
    fn star_in_account_does_not_cross_into_resource() {
        // `*` placed in the account-id slot must not consume the colon
        // before the resource segment, so a literal `user/alice` in the
        // pattern can only match a literal `user/alice` resource. A
        // value whose own resource happens to start with `role:user/alice`
        // must NOT satisfy a pattern of `:user/alice`.
        assert!(!matches_arn(
            "arn:aws:iam::*:user/alice",
            "arn:aws:iam::123:role:user/alice"
        ));
        assert!(matches_arn(
            "arn:aws:iam::*:user/alice",
            "arn:aws:iam::123:user/alice"
        ));
    }

    #[test]
    fn star_in_resource_segment_spans_internal_colons() {
        // CloudWatch Logs and similar services pack additional colons into
        // the resource segment. A trailing `*` in the resource segment must
        // still be free to consume them: the segment-awareness only kicks
        // in at the first five colons.
        assert!(matches_arn(
            "arn:aws:logs:us-east-1:123:log-group:my-group:*",
            "arn:aws:logs:us-east-1:123:log-group:my-group:log-stream:s1"
        ));
    }

    #[test]
    fn bare_star_matches_any_arn() {
        // `Resource: "*"` is the most common admin-policy shape and must
        // match every ARN regardless of how many colons it carries.
        assert!(matches_arn("*", "arn:aws:s3:::bucket/key"));
        assert!(matches_arn(
            "*",
            "arn:aws:logs:us-east-1:123:log-group:g:log-stream:s"
        ));
    }

    #[test]
    fn segment_count_mismatch_falls_back_to_unrestricted() {
        // Pattern is not ARN-shaped so we cannot split-by-segment; treat
        // it as an opaque glob the same as `matches`.
        assert!(matches_arn("anything*", "anything:goes:here"));
    }
}
