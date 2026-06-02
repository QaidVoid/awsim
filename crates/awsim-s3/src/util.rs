use awsim_core::AwsError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use md5::{Digest, Md5};

/// Compute an MD5 ETag for the given data (wrapped in quotes as S3 does).
pub fn compute_etag(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("\"{:x}\"", result)
}

/// Decode an `aws-chunked` framed body into its raw bytes.
///
/// AWS SDKs use this encoding when uploading with SigV4 streaming. Each
/// chunk is prefixed with `<hex-size>;chunk-signature=<sig>\r\n`, followed
/// by `<data>\r\n`, and the body terminates with a zero-sized chunk plus
/// optional trailers. We don't currently re-verify the per-chunk
/// signatures (SigV4 verification is skipped at the gateway anyway), so
/// the decoder just strips the framing and concatenates the data.
///
/// Returns Err if the framing is malformed (truncated mid-chunk, garbage
/// hex size, etc.) so callers surface InvalidRequest rather than storing
/// a corrupt object body.
pub fn decode_aws_chunked(framed: &[u8]) -> Result<Vec<u8>, AwsError> {
    let mut out = Vec::with_capacity(framed.len());
    let mut i = 0usize;
    while i < framed.len() {
        // Locate the first \r\n that closes the chunk header.
        let nl = match find_crlf(framed, i) {
            Some(n) => n,
            None => {
                return Err(AwsError::bad_request(
                    "InvalidRequest",
                    "aws-chunked: chunk header missing CRLF",
                ));
            }
        };
        let header = std::str::from_utf8(&framed[i..nl]).map_err(|_| {
            AwsError::bad_request("InvalidRequest", "aws-chunked: chunk header is not UTF-8")
        })?;
        // Header is `<hex-size>;chunk-signature=<sig>` (signature optional).
        let size_str = header.split(';').next().unwrap_or(header).trim();
        let size = usize::from_str_radix(size_str, 16).map_err(|_| {
            AwsError::bad_request(
                "InvalidRequest",
                format!("aws-chunked: bad hex chunk size '{size_str}'"),
            )
        })?;
        let data_start = nl + 2;
        if size == 0 {
            // Final chunk; trailers (if any) follow but we don't need them.
            return Ok(out);
        }
        let data_end = data_start.checked_add(size).ok_or_else(|| {
            AwsError::bad_request("InvalidRequest", "aws-chunked: chunk size overflow")
        })?;
        if data_end + 2 > framed.len() {
            return Err(AwsError::bad_request(
                "InvalidRequest",
                "aws-chunked: truncated chunk body",
            ));
        }
        if &framed[data_end..data_end + 2] != b"\r\n" {
            return Err(AwsError::bad_request(
                "InvalidRequest",
                "aws-chunked: chunk body missing trailing CRLF",
            ));
        }
        out.extend_from_slice(&framed[data_start..data_end]);
        i = data_end + 2;
    }
    // Body ended without a 0-sized terminator. Some clients still send
    // valid chunks back-to-back without one; accept what we decoded.
    Ok(out)
}

fn find_crlf(buf: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Validate the optional `Content-MD5` header against the actual body.
///
/// AWS S3 expects the value to be a base64-encoded 16-byte MD5 of the body.
/// Real S3 returns `BadDigest` (HTTP 400) on any mismatch and refuses to
/// store the object. awsim previously accepted (and stored!) any MD5 the
/// caller wrote in the header.
pub fn verify_content_md5(body: &[u8], provided_b64: &str) -> Result<(), AwsError> {
    let decoded = BASE64
        .decode(provided_b64)
        .map_err(|_| AwsError::bad_request("InvalidDigest", "Content-MD5 is not valid base64"))?;
    if decoded.len() != 16 {
        return Err(AwsError::bad_request(
            "InvalidDigest",
            "Content-MD5 must decode to 16 bytes",
        ));
    }
    let mut hasher = Md5::new();
    hasher.update(body);
    let actual = hasher.finalize();
    if actual.as_slice() == decoded.as_slice() {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "BadDigest",
            "Content-MD5 does not match the calculated MD5 of the request body",
        ))
    }
}

/// Validate an `x-amz-checksum-*` header against the body.
///
/// `algorithm` is the AWS algorithm name as parsed by
/// `parse_request_checksum` (`CRC32`, `CRC32C`, `CRC64NVME`, `SHA1`,
/// `SHA256`). `provided_b64` is the base64 of the asserted digest.
/// All five (CRC32, CRC32C, CRC64NVME, SHA1, SHA256) are recomputed from
/// the body and compared; a mismatch returns `BadDigest`.
pub fn verify_object_checksum(
    body: &[u8],
    algorithm: &str,
    provided_b64: &str,
) -> Result<(), AwsError> {
    let provided = BASE64.decode(provided_b64).map_err(|_| {
        AwsError::bad_request(
            "InvalidRequest",
            format!("{algorithm} checksum is not valid base64"),
        )
    })?;
    let actual: Vec<u8> = match algorithm {
        "SHA256" => {
            use sha2::Sha256;
            let mut h = Sha256::new();
            h.update(body);
            h.finalize().to_vec()
        }
        "SHA1" => {
            use sha1::Sha1;
            let mut h = Sha1::new();
            h.update(body);
            h.finalize().to_vec()
        }
        // S3 transmits CRC digests big-endian.
        "CRC32" => {
            const C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
            C.checksum(body).to_be_bytes().to_vec()
        }
        "CRC32C" => {
            const C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
            C.checksum(body).to_be_bytes().to_vec()
        }
        "CRC64NVME" => {
            const C: crc::Crc<u64> = crc::Crc::<u64>::new(&crc::CRC_64_NVME);
            C.checksum(body).to_be_bytes().to_vec()
        }
        // Unknown algorithm name: the parse layer should never produce one,
        // so don't reject.
        _ => return Ok(()),
    };
    if actual == provided {
        Ok(())
    } else {
        Err(AwsError::bad_request(
            "BadDigest",
            format!("{algorithm} checksum does not match the request body"),
        ))
    }
}

/// Compute the multipart ETag as AWS S3 does: MD5 of concatenated
/// per-part MD5s, followed by `-{part_count}`, all wrapped in quotes.
pub fn compute_multipart_etag(part_md5s: &[Vec<u8>], part_count: usize) -> String {
    let mut hasher = Md5::new();
    for md5_bytes in part_md5s {
        hasher.update(md5_bytes);
    }
    let result = hasher.finalize();
    format!("\"{:x}-{}\"", result, part_count)
}

/// Return the current time as an ISO 8601 UTC string.
/// Example: `2026-04-21T14:48:01.000Z`
///
/// The S3 SDK expects ISO 8601 for `CreationDate` in `ListAllMyBucketsResult`.
pub fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso8601(secs)
}

/// Format a Unix timestamp as ISO 8601 (e.g. `2026-04-21T14:48:01.000Z`).
pub fn format_iso8601(secs: u64) -> String {
    let s = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let mut days = hours / 24;
    let mut y = 1970u64;
    loop {
        let dy = if is_leap_year(y) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        y += 1;
    }
    let months = if is_leap_year(y) {
        [31u64, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 0usize;
    loop {
        if days < months[mo] {
            break;
        }
        days -= months[mo];
        mo += 1;
    }
    let d = days + 1;
    format!("{y:04}-{:02}-{d:02}T{h:02}:{min:02}:{s:02}.000Z", mo + 1)
}

/// Parse an RFC 7231 / RFC 1123 date string into Unix seconds.
///
/// Returns `None` if the input is malformed. Used to compare conditional
/// `If-Modified-Since` / `If-Unmodified-Since` headers against an object's
/// stored last-modified timestamp.
pub fn parse_rfc7231(s: &str) -> Option<i64> {
    // Parse: "<day-name>, <dd> <Mon> <yyyy> <HH>:<mm>:<ss> GMT"
    let parts: Vec<&str> = s.splitn(2, ", ").collect();
    let rest = if parts.len() == 2 { parts[1] } else { s };

    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() < 4 {
        return None;
    }
    let day: i64 = tokens[0].parse().ok()?;
    let month = MONTH_NAMES.iter().position(|&m| m == tokens[1])? as i64;
    let year: i64 = tokens[2].parse().ok()?;
    let time_parts: Vec<&str> = tokens[3].split(':').collect();
    if time_parts.len() < 3 {
        return None;
    }
    let h: i64 = time_parts[0].parse().ok()?;
    let min: i64 = time_parts[1].parse().ok()?;
    let s: i64 = time_parts[2].parse().ok()?;

    // Convert (year, month, day, h, min, s) to seconds since 1970-01-01.
    if year < 1970 {
        return None;
    }
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap_year(y as u64) { 366 } else { 365 };
    }
    let monthly = if is_leap_year(year as u64) {
        [31i64, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for &days_in_month in monthly.iter().take(month as usize) {
        days += days_in_month;
    }
    days += day - 1;

    Some(days * 86400 + h * 3600 + min * 60 + s)
}

/// Convert an RFC 7231 date string to ISO 8601 via Unix epoch (best-effort).
///
/// S3 stores `LastModified` as RFC 7231 (e.g. `Tue, 21 Apr 2026 14:52:46 GMT`)
/// internally but the S3 XML API (ListObjectsV2 etc.) requires ISO 8601
/// (e.g. `2026-04-21T14:52:46.000Z`).  This function converts between the
/// two formats.  If parsing fails the string is returned unchanged.
pub fn rfc7231_to_iso8601(rfc7231: &str) -> String {
    // Parse: "<day-name>, <dd> <Mon> <yyyy> <HH>:<mm>:<ss> GMT"
    // Example: "Tue, 21 Apr 2026 14:52:46 GMT"
    let parts: Vec<&str> = rfc7231.splitn(2, ", ").collect();
    let rest = if parts.len() == 2 {
        parts[1]
    } else {
        return rfc7231.to_string();
    };

    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() < 4 {
        return rfc7231.to_string();
    }

    let day: u64 = tokens[0].parse().unwrap_or(1);
    let month = MONTH_NAMES
        .iter()
        .position(|&m| m == tokens[1])
        .map(|i| i as u64 + 1)
        .unwrap_or(1);
    let year: u64 = tokens[2].parse().unwrap_or(1970);
    let time_parts: Vec<&str> = tokens[3].split(':').collect();
    if time_parts.len() < 3 {
        return rfc7231.to_string();
    }
    let h: u64 = time_parts[0].parse().unwrap_or(0);
    let min: u64 = time_parts[1].parse().unwrap_or(0);
    let s: u64 = time_parts[2].parse().unwrap_or(0);

    format!("{year:04}-{month:02}-{day:02}T{h:02}:{min:02}:{s:02}.000Z")
}

/// Return the current time as an RFC 7231 date string.
/// Example: `Mon, 21 Apr 2026 12:00:00 GMT`
pub fn now_rfc7231() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    format_rfc7231(secs)
}

/// Format a Unix timestamp as RFC 7231.
pub fn format_rfc7231(secs: u64) -> String {
    // Days per month (non-leap year, then leap-year Jan is fine since we compute the actual year).
    const DAYS_IN_MONTH: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    const DAY_NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let sec = secs % 60;
    let min = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let mut days = secs / 86400;

    // Day of week (Unix epoch = Thursday = index 0)
    let day_of_week = DAY_NAMES[(days % 7) as usize];

    // Compute year, month, day from epoch days.
    let mut year = 1970u64;
    loop {
        let is_leap = is_leap_year(year);
        let days_in_year = if is_leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let is_leap = is_leap_year(year);
    let mut month = 0usize;
    loop {
        let days_in_month = if month == 1 && is_leap {
            29
        } else {
            DAYS_IN_MONTH[month]
        };
        if days < days_in_month {
            break;
        }
        days -= days_in_month;
        month += 1;
    }

    let day = days + 1;

    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        day_of_week, day, MONTH_NAMES[month], year, hour, min, sec
    )
}

fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_etag() {
        // MD5 of empty string
        let etag = compute_etag(b"");
        assert_eq!(etag, "\"d41d8cd98f00b204e9800998ecf8427e\"");
    }

    #[test]
    fn test_format_rfc7231_known() {
        // Unix epoch 0 = Thu, 01 Jan 1970 00:00:00 GMT
        let s = format_rfc7231(0);
        assert_eq!(s, "Thu, 01 Jan 1970 00:00:00 GMT");
    }

    #[test]
    fn test_parse_rfc7231_round_trips_format() {
        for &secs in &[0u64, 1_000_000, 1_700_000_000, 1_900_000_000] {
            let formatted = format_rfc7231(secs);
            assert_eq!(parse_rfc7231(&formatted), Some(secs as i64), "for {secs}");
        }
    }

    #[test]
    fn test_parse_rfc7231_known_value() {
        // 1900-01-01 is rejected (before epoch).
        assert_eq!(parse_rfc7231("Mon, 01 Jan 1900 00:00:00 GMT"), None);
        // Garbage input is rejected.
        assert_eq!(parse_rfc7231("not a date"), None);
        // RFC 7231 without leading day-name is also accepted.
        assert!(parse_rfc7231("01 Jan 1970 00:00:01 GMT").is_some());
    }

    #[test]
    fn content_md5_accepts_correct_digest() {
        // MD5("hello") = 5d41402abc4b2a76b9719d911017c592
        let body = b"hello";
        let mut h = Md5::new();
        h.update(body);
        let b64 = BASE64.encode(h.finalize());
        assert!(verify_content_md5(body, &b64).is_ok());
    }

    #[test]
    fn content_md5_rejects_mismatch() {
        let err = verify_content_md5(b"hello", "AAAAAAAAAAAAAAAAAAAAAA==").unwrap_err();
        assert_eq!(err.code, "BadDigest");
    }

    #[test]
    fn content_md5_rejects_malformed_base64() {
        let err = verify_content_md5(b"hello", "not-base64!!").unwrap_err();
        assert_eq!(err.code, "InvalidDigest");
    }

    #[test]
    fn content_md5_rejects_wrong_length_digest() {
        let err = verify_content_md5(b"hello", BASE64.encode(b"too short").as_str()).unwrap_err();
        assert_eq!(err.code, "InvalidDigest");
    }

    #[test]
    fn checksum_sha256_round_trip() {
        let body = b"hello";
        let mut h = sha2::Sha256::new();
        h.update(body);
        let b64 = BASE64.encode(h.finalize());
        assert!(verify_object_checksum(body, "SHA256", &b64).is_ok());
        assert!(verify_object_checksum(b"goodbye", "SHA256", &b64).is_err());
    }

    #[test]
    fn checksum_sha1_round_trip() {
        let body = b"hello";
        let mut h = sha1::Sha1::new();
        h.update(body);
        let b64 = BASE64.encode(h.finalize());
        assert!(verify_object_checksum(body, "SHA1", &b64).is_ok());
        assert!(verify_object_checksum(b"goodbye", "SHA1", &b64).is_err());
    }

    #[test]
    fn checksum_crc_constants_match_catalog_check_values() {
        // The standard CRC "check" value is the digest of b"123456789".
        const CRC32: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        const CRC32C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
        const CRC64: crc::Crc<u64> = crc::Crc::<u64>::new(&crc::CRC_64_NVME);
        assert_eq!(CRC32.checksum(b"123456789"), 0xCBF4_3926);
        assert_eq!(CRC32C.checksum(b"123456789"), 0xE306_9283);
        assert_eq!(CRC64.checksum(b"123456789"), 0xAE8B_1486_0A79_9888);
    }

    #[test]
    fn checksum_crc32_round_trip() {
        let body = b"hello";
        const C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        let b64 = BASE64.encode(C.checksum(body).to_be_bytes());
        assert!(verify_object_checksum(body, "CRC32", &b64).is_ok());
        assert!(verify_object_checksum(b"goodbye", "CRC32", &b64).is_err());
    }

    #[test]
    fn checksum_crc32c_round_trip() {
        let body = b"hello";
        const C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
        let b64 = BASE64.encode(C.checksum(body).to_be_bytes());
        assert!(verify_object_checksum(body, "CRC32C", &b64).is_ok());
        assert!(verify_object_checksum(b"goodbye", "CRC32C", &b64).is_err());
    }

    #[test]
    fn checksum_crc64nvme_round_trip() {
        let body = b"hello";
        const C: crc::Crc<u64> = crc::Crc::<u64>::new(&crc::CRC_64_NVME);
        let b64 = BASE64.encode(C.checksum(body).to_be_bytes());
        assert!(verify_object_checksum(body, "CRC64NVME", &b64).is_ok());
        assert!(verify_object_checksum(b"goodbye", "CRC64NVME", &b64).is_err());
    }

    #[test]
    fn aws_chunked_single_chunk_decodes() {
        // 5-byte payload "hello" framed as a single SigV4-streaming chunk.
        let framed = b"5;chunk-signature=abc\r\nhello\r\n0;chunk-signature=def\r\n\r\n";
        let decoded = decode_aws_chunked(framed).unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn aws_chunked_multi_chunk_concatenates() {
        let framed =
            b"3;chunk-signature=a\r\nfoo\r\n3;chunk-signature=b\r\nbar\r\n0;chunk-signature=c\r\n";
        let decoded = decode_aws_chunked(framed).unwrap();
        assert_eq!(decoded, b"foobar");
    }

    #[test]
    fn aws_chunked_rejects_truncated_chunk_body() {
        // Header says 10 bytes but body is shorter.
        let framed = b"a;chunk-signature=x\r\nfoo";
        let err = decode_aws_chunked(framed).unwrap_err();
        assert_eq!(err.code, "InvalidRequest");
    }

    #[test]
    fn aws_chunked_rejects_bad_hex_size() {
        let framed = b"zz;chunk-signature=x\r\nhello\r\n";
        let err = decode_aws_chunked(framed).unwrap_err();
        assert_eq!(err.code, "InvalidRequest");
    }
}
