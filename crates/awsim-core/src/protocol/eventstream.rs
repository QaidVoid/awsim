//! AWS event-stream binary framing.
//!
//! Used by Bedrock `ConverseStream` and `InvokeModelWithResponseStream`
//! (and a handful of other AWS streaming APIs). Each message is a
//! self-delimiting binary frame with a CRC-protected prelude plus
//! typed headers and a JSON payload.
//!
//! Wire format:
//! ```text
//! +-----------------------------+
//! | Total length    (u32 BE)    |
//! +-----------------------------+
//! | Headers length  (u32 BE)    |
//! +-----------------------------+
//! | Prelude CRC32   (u32 BE)    |  CRC of the previous 8 bytes
//! +-----------------------------+
//! | Headers (variable)          |
//! +-----------------------------+
//! | Payload (variable)          |
//! +-----------------------------+
//! | Message CRC32   (u32 BE)    |  CRC of everything before, except itself
//! +-----------------------------+
//! ```
//!
//! Each header:
//! ```text
//! [name_len: u8][name: utf8][value_type: u8][value_len: u16 BE][value: utf8]
//! ```
//!
//! Only string-valued headers (type `0x07`) are needed for AWS
//! streaming responses, so that's all this encoder emits.

use serde_json::Value;

const HEADER_TYPE_STRING: u8 = 0x07;

/// CRC-32 (IEEE 802.3, reflected polynomial 0xedb88320). Streaming
/// responses are low-frequency enough that the bit-by-bit
/// implementation isn't worth optimising; a table-driven version
/// would shave microseconds we never feel.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &byte in data {
        let mut b = byte as u32;
        for _ in 0..8 {
            let bit = (crc ^ b) & 1;
            crc >>= 1;
            if bit == 1 {
                crc ^= 0xedb8_8320;
            }
            b >>= 1;
        }
    }
    !crc
}

/// One named string header. AWS uses `:`-prefixed names for
/// well-known headers (`:event-type`, `:content-type`, `:message-type`).
pub struct EventHeader {
    pub name: String,
    pub value: String,
}

/// Encode a single event-stream message and append it to `out`.
pub fn append_message(out: &mut Vec<u8>, headers: &[EventHeader], payload: &[u8]) {
    // Headers section.
    let mut hb: Vec<u8> = Vec::new();
    for h in headers {
        debug_assert!(h.name.len() <= u8::MAX as usize, "header name too long");
        debug_assert!(h.value.len() <= u16::MAX as usize, "header value too long");
        hb.push(h.name.len() as u8);
        hb.extend_from_slice(h.name.as_bytes());
        hb.push(HEADER_TYPE_STRING);
        hb.extend_from_slice(&(h.value.len() as u16).to_be_bytes());
        hb.extend_from_slice(h.value.as_bytes());
    }

    let headers_len = hb.len() as u32;
    let total_len = 4u32 + 4 + 4 + headers_len + payload.len() as u32 + 4;

    let start = out.len();
    out.extend_from_slice(&total_len.to_be_bytes());
    out.extend_from_slice(&headers_len.to_be_bytes());
    let prelude_crc = crc32(&out[start..start + 8]);
    out.extend_from_slice(&prelude_crc.to_be_bytes());
    out.extend_from_slice(&hb);
    out.extend_from_slice(payload);
    let msg_crc = crc32(&out[start..]);
    out.extend_from_slice(&msg_crc.to_be_bytes());
}

/// Marker key on a `Value` that tells the protocol layer "this is an
/// event-stream response, encode the payload as binary frames and
/// use the `application/vnd.amazon.eventstream` content type".
///
/// The marker value is an array of objects, each `{ headers: { ... },
/// payload: <Value> }`. Headers are always strings; payload is
/// JSON-encoded into the frame body.
pub const MARKER: &str = "__awsim_eventstream__";

/// Detect the marker and, if present, encode the events as concatenated
/// AWS event-stream binary frames. Returns `None` when the value is
/// just a regular JSON response.
pub fn try_encode(value: &Value) -> Option<Vec<u8>> {
    let frames = value.as_object()?.get(MARKER)?.as_array()?;
    let mut out: Vec<u8> = Vec::new();
    for frame in frames {
        let Some(obj) = frame.as_object() else {
            continue;
        };
        let mut headers: Vec<EventHeader> = Vec::new();
        if let Some(hmap) = obj.get("headers").and_then(Value::as_object) {
            for (k, v) in hmap {
                if let Some(s) = v.as_str() {
                    headers.push(EventHeader {
                        name: k.clone(),
                        value: s.to_string(),
                    });
                }
            }
        }
        let payload_bytes = obj
            .get("payload")
            .map(|p| serde_json::to_vec(p).unwrap_or_default())
            .unwrap_or_default();
        append_message(&mut out, &headers, &payload_bytes);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn crc32_known_vectors() {
        // AWS docs example: "abc" → 0x352441C2
        assert_eq!(crc32(b"abc"), 0x3524_41C2);
        // Empty input → 0
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn round_trip_one_message() {
        let value = json!({
            MARKER: [
                {
                    "headers": {
                        ":event-type": "messageStart",
                        ":message-type": "event",
                        ":content-type": "application/json",
                    },
                    "payload": {"role": "assistant"}
                }
            ]
        });
        let bytes = try_encode(&value).unwrap();
        // Total length is at least the prelude (12) + payload + final
        // CRC (4) + headers, all > 16.
        assert!(bytes.len() > 20);
        // First 4 bytes encode total length, which equals the buffer
        // size for a single-message stream.
        let total_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(total_len as usize, bytes.len());
    }

    #[test]
    fn no_marker_returns_none() {
        let value = json!({ "foo": "bar" });
        assert!(try_encode(&value).is_none());
    }
}
