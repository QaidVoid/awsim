//! Order-preserving text encoding for `N`-typed key values.
//!
//! Key columns are TEXT and SQLite compares TEXT byte-wise, so storing a
//! DynamoDB number verbatim sorts it lexicographically: `"10"` lands
//! before `"9"`, and `"-1"` before `"-2"`. That breaks `Query` ordering,
//! `ScanIndexForward`, and every `LastEvaluatedKey` cursor over a
//! numeric sort key.
//!
//! [`encode`] maps a number onto a fixed-width string whose byte order
//! matches its numeric order, so the existing `ORDER BY` and range
//! comparisons become correct without any special-casing downstream.
//!
//! Layout, always [`ENCODED_LEN`] bytes:
//!
//! ```text
//!   <sign><exponent, 3 digits><significand, 38 digits>
//! ```
//!
//! * sign is `'0'` negative, `'1'` zero, `'2'` positive, so the three
//!   classes separate before anything else is compared.
//! * the value is normalised to `0.<significand> x 10^exponent`, and the
//!   exponent is biased by [`EXP_BIAS`] to keep it non-negative.
//! * for negatives both the exponent and every significand digit are
//!   nine's-complemented, so a larger magnitude sorts earlier.
//!
//! Fixed width is what makes this safe: with every field the same
//! length, a plain byte comparison can never confuse a short encoding
//! with a prefix of a longer one.
//!
//! The encoding is one-way. Key columns are opaque tokens used for
//! ordering and identity; the values callers see always come from the
//! item's stored attributes.

/// Digits of significand retained. Matches DynamoDB's 38-digit limit,
/// so no representable key loses precision.
const SIGNIFICAND_DIGITS: usize = 38;

/// Added to the decimal exponent so it is never negative. DynamoDB
/// spans `1E-130` to `9.99..E+125`, which is an exponent of -129..=126
/// in `0.<significand>` form, so 200 keeps it inside three digits.
const EXP_BIAS: i32 = 200;

/// Width of the biased exponent field.
const EXP_DIGITS: usize = 3;

/// Total encoded width: sign + exponent + significand.
pub const ENCODED_LEN: usize = 1 + EXP_DIGITS + SIGNIFICAND_DIGITS;

/// Encode a DynamoDB `N` value into its order-preserving form.
///
/// Returns `None` if `raw` is not a well-formed number, which lets
/// callers fall back to storing it verbatim rather than fabricating a
/// key for input the validator should already have rejected.
pub fn encode(raw: &str) -> Option<String> {
    let s = raw.trim();
    let (negative, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };

    // Split off an exponent suffix.
    let (mantissa, exp_suffix) = match rest.find(['e', 'E']) {
        Some(i) => (&rest[..i], rest[i + 1..].parse::<i32>().ok()?),
        None => (rest, 0),
    };

    // Split the mantissa on its decimal point.
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }

    // All significant digits, decimal point notionally before the first.
    let digits: String = format!("{int_part}{frac_part}");
    let first_significant = digits.find(|c| c != '0');

    let Some(first) = first_significant else {
        // Every digit is zero, so the value is zero regardless of sign
        // or exponent. Zero has exactly one encoding.
        return Some(format!("1{}", "0".repeat(EXP_DIGITS + SIGNIFICAND_DIGITS)));
    };

    // Exponent such that value == 0.<significand> x 10^exponent.
    let exponent = int_part.len() as i32 - first as i32 + exp_suffix;

    // Trailing zeros carry no value and would make 1.0 and 1 encode
    // differently, so drop them.
    let significand = digits[first..].trim_end_matches('0');
    let significand = if significand.is_empty() {
        "0"
    } else {
        significand
    };
    if significand.len() > SIGNIFICAND_DIGITS {
        return None;
    }

    let biased = exponent + EXP_BIAS;
    if biased < 0 || biased >= 10i32.pow(EXP_DIGITS as u32) {
        return None;
    }

    let exp_field = format!("{biased:0EXP_DIGITS$}", EXP_DIGITS = EXP_DIGITS);
    let sig_field = format!("{significand:0<SIGNIFICAND_DIGITS$}");

    Some(if negative {
        // Invert both fields so a larger magnitude sorts earlier.
        format!(
            "0{}{}",
            nines_complement(&exp_field),
            nines_complement(&sig_field)
        )
    } else {
        format!("2{exp_field}{sig_field}")
    })
}

/// Replace each digit `d` with `9 - d`, so ascending input becomes
/// descending output.
fn nines_complement(digits: &str) -> String {
    digits
        .bytes()
        .map(|b| (b'9' - (b - b'0')) as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(s: &str) -> String {
        encode(s).unwrap_or_else(|| panic!("{s} should encode"))
    }

    /// The whole point: byte order must match numeric order.
    #[test]
    fn encoded_order_matches_numeric_order() {
        // Deliberately includes the pairs that break naive text sorting.
        let mut values = vec![
            "-100", "-10", "-9.5", "-9", "-2", "-1.5", "-1", "-0.5", "-0.001", "0", "0.001", "0.5",
            "1", "1.5", "2", "9", "9.5", "10", "100", "1000",
        ];
        let encoded_sorted = {
            let mut v: Vec<(String, &str)> = values.iter().map(|s| (enc(s), *s)).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v.into_iter().map(|(_, s)| s).collect::<Vec<_>>()
        };
        // `values` is already in numeric order.
        assert_eq!(encoded_sorted, values);

        // And the naive encoding really would have got it wrong, so this
        // test is testing something.
        values.sort();
        assert_ne!(values, encoded_sorted);
    }

    #[test]
    fn every_encoding_is_fixed_width() {
        for v in ["0", "1", "-1", "1e125", "1e-130", "-9.99e125", "123456789"] {
            assert_eq!(enc(v).len(), ENCODED_LEN, "{v}");
        }
    }

    #[test]
    fn equal_values_encode_identically_regardless_of_spelling() {
        assert_eq!(enc("1"), enc("1.0"));
        assert_eq!(enc("1"), enc("1.00"));
        assert_eq!(enc("1"), enc("+1"));
        assert_eq!(enc("100"), enc("1e2"));
        assert_eq!(enc("0.5"), enc("5e-1"));
        // Zero has one encoding no matter how it is written.
        assert_eq!(enc("0"), enc("-0"));
        assert_eq!(enc("0"), enc("0.000"));
        assert_eq!(enc("0"), enc("0e10"));
    }

    #[test]
    fn sign_classes_separate_cleanly() {
        assert!(enc("-0.0001") < enc("0"));
        assert!(enc("0") < enc("0.0001"));
    }

    #[test]
    fn spans_the_full_dynamodb_range() {
        assert!(enc("1e-130") < enc("1"));
        assert!(enc("1") < enc("9.99e125"));
        assert!(enc("-9.99e125") < enc("-1e-130"));
    }

    #[test]
    fn keeps_all_38_digits_of_precision() {
        let a = "1".repeat(38);
        let mut b = a.clone();
        b.replace_range(37..38, "2"); // differs only in the last digit
        assert!(enc(&a) < enc(&b), "38th digit must still be significant");
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "",
            "abc",
            "1.2.3",
            "--1",
            "1e",
            ".",
            "1e999999",
            "1e-999999",
        ] {
            assert!(encode(bad).is_none(), "{bad} should not encode");
        }
    }

    /// The encodable exponent window is deliberately wider than the
    /// range `validate_number` accepts. Values that far out never reach
    /// a key column, and encoding rather than rejecting them keeps this
    /// function total over anything the validator might later permit.
    #[test]
    fn encodable_window_is_wider_than_dynamodbs_own_range() {
        assert!(encode("1e400").is_some());
        assert!(enc("1e300") < enc("1e400"));
        assert!(enc("-1e400") < enc("-1e300"));
    }

    /// Exhaustive ordering check over a dense range, including the
    /// sign boundary, at a scale where any off-by-one shows up.
    #[test]
    fn dense_sweep_preserves_order() {
        let mut pairs: Vec<(i64, String)> =
            (-500..=500).map(|n| (n, enc(&n.to_string()))).collect();
        pairs.sort_by(|a, b| a.1.cmp(&b.1));
        let order: Vec<i64> = pairs.into_iter().map(|(n, _)| n).collect();
        let expected: Vec<i64> = (-500..=500).collect();
        assert_eq!(order, expected);
    }

    /// Fractions interleaved with integers, which is where a mishandled
    /// exponent tends to surface.
    #[test]
    fn fractional_sweep_preserves_order() {
        let mut vals: Vec<f64> = vec![];
        let mut x = -10.0f64;
        while x <= 10.0 {
            vals.push((x * 1000.0).round() / 1000.0);
            x += 0.037;
        }
        let mut encoded: Vec<(String, f64)> =
            vals.iter().map(|v| (enc(&v.to_string()), *v)).collect();
        encoded.sort_by(|a, b| a.0.cmp(&b.0));
        let got: Vec<f64> = encoded.into_iter().map(|(_, v)| v).collect();
        let mut want = vals.clone();
        want.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(got, want);
    }
}
