pub fn matches(pattern: &str, value: &str) -> bool {
    glob_match(pattern.as_bytes(), value.as_bytes(), None)
}

pub fn matches_arn(pattern: &str, arn: &str) -> bool {
    glob_match(pattern.as_bytes(), arn.as_bytes(), Some(b':'))
}

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
}
