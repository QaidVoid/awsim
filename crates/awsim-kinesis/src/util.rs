use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use md5::{Digest, Md5};

use crate::state::ShardIteratorInfo;

/// Compute an MD5 hash of the partition key and return it as a 128-bit integer,
/// which maps into the hash key space [0, 2^128 - 1].
pub fn partition_key_to_hash(partition_key: &str) -> u128 {
    let mut hasher = Md5::new();
    hasher.update(partition_key.as_bytes());
    let result = hasher.finalize();
    let bytes: [u8; 16] = result.into();
    u128::from_be_bytes(bytes)
}

/// Find which shard index a hash value falls into, given the stream's shard list.
/// Each shard covers [start_hash, end_hash] inclusive.
pub fn hash_to_shard_index(
    hash: u128,
    shards: &[crate::state::Shard],
) -> usize {
    for (i, shard) in shards.iter().enumerate() {
        let start: u128 = shard.hash_key_range.0.parse().unwrap_or(0);
        let end: u128 = shard.hash_key_range.1.parse().unwrap_or(u128::MAX);
        if hash >= start && hash <= end {
            return i;
        }
    }
    // Fallback: last shard
    if shards.is_empty() { 0 } else { shards.len() - 1 }
}

/// Encode a ShardIteratorInfo into an opaque base64 token.
/// Format before encoding: "{stream_name}:{shard_index}:{position}"
pub fn encode_iterator(info: &ShardIteratorInfo) -> String {
    let raw = format!("{}:{}:{}", info.stream_name, info.shard_index, info.position);
    BASE64.encode(raw.as_bytes())
}

/// Decode an opaque shard iterator token back into ShardIteratorInfo.
pub fn decode_iterator(token: &str) -> Option<ShardIteratorInfo> {
    let bytes = BASE64.decode(token).ok()?;
    let raw = String::from_utf8(bytes).ok()?;
    let parts: Vec<&str> = raw.splitn(3, ':').collect();
    if parts.len() != 3 {
        return None;
    }
    let stream_name = parts[0].to_string();
    let shard_index: usize = parts[1].parse().ok()?;
    let position: usize = parts[2].parse().ok()?;
    Some(ShardIteratorInfo {
        stream_name,
        shard_index,
        position,
    })
}

/// Divide the full hash key space [0, 2^128 - 1] into `count` equal ranges.
/// Returns a Vec of (start, end) pairs, where end is inclusive.
pub fn divide_hash_space(count: usize) -> Vec<(u128, u128)> {
    if count == 0 {
        return vec![];
    }
    // Use u128::MAX as the top of the space
    let total = u128::MAX;
    let shard_size = total / count as u128;
    let mut ranges = Vec::with_capacity(count);
    for i in 0..count {
        let start = i as u128 * shard_size;
        let end = if i == count - 1 {
            total
        } else {
            start + shard_size - 1
        };
        ranges.push((start, end));
    }
    ranges
}
