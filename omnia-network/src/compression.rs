//! Shared compression utilities for gossip messages.

use serde::{de::DeserializeOwned, Serialize};

/// Compression algorithm identifier: uncompressed.
pub const COMPRESSION_NONE: u8 = 0;
/// Compression algorithm identifier: snappy compressed.
pub const COMPRESSION_SNAPPY: u8 = 1;
/// Minimum payload size (bytes) to trigger compression.
pub const COMPRESSION_THRESHOLD: usize = 1024;
/// Maximum allowed decompressed size (2 MiB).
pub const MAX_DECOMPRESSED_SIZE: usize = 2 * 1024 * 1024;

/// Serialize and optionally compress a message.
///
/// If the serialized payload exceeds [`COMPRESSION_THRESHOLD`] bytes and
/// snappy compression reduces the size, the output is prefixed with
/// [`COMPRESSION_SNAPPY`]. Otherwise, the output is prefixed with
/// [`COMPRESSION_NONE`].
pub fn serialize_with_compression<T: Serialize>(msg: &T) -> Result<Vec<u8>, String> {
    let payload = postcard::to_allocvec(msg).map_err(|e| e.to_string())?;
    if payload.len() >= COMPRESSION_THRESHOLD {
        let mut encoder = snap::raw::Encoder::new();
        let compressed = encoder.compress_vec(&payload).map_err(|e| e.to_string())?;
        if compressed.len() < payload.len() {
            let mut out = Vec::with_capacity(1 + compressed.len());
            out.push(COMPRESSION_SNAPPY);
            out.extend_from_slice(&compressed);
            return Ok(out);
        }
    }
    let mut out = Vec::with_capacity(1 + payload.len());
    out.push(COMPRESSION_NONE);
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Deserialize a message, handling decompression.
///
/// Reads the first byte as a compression flag:
/// - [`COMPRESSION_NONE`]: payload follows verbatim
/// - [`COMPRESSION_SNAPPY`]: snappy-compressed payload follows
///
/// Enforces a decompressed size limit of [`MAX_DECOMPRESSED_SIZE`] to
/// prevent memory exhaustion from malicious payloads.
pub fn deserialize_with_compression<T: DeserializeOwned>(data: &[u8]) -> Result<T, String> {
    if data.is_empty() {
        return Err("empty payload".to_string());
    }
    let compression = data[0];
    let payload = &data[1..];
    let decompressed = match compression {
        COMPRESSION_NONE => payload.to_vec(),
        COMPRESSION_SNAPPY => {
            // Check the declared decompressed size before allocating.
            let decompressed_len =
                snap::raw::decompress_len(payload).map_err(|e| e.to_string())?;
            if decompressed_len > MAX_DECOMPRESSED_SIZE {
                return Err(format!(
                    "decompressed size {} exceeds limit {}",
                    decompressed_len, MAX_DECOMPRESSED_SIZE
                ));
            }
            let mut decoder = snap::raw::Decoder::new();
            decoder.decompress_vec(payload).map_err(|e| e.to_string())?
        }
        _ => return Err(format!("unknown compression algorithm: {}", compression)),
    };
    postcard::from_bytes(&decompressed).map_err(|e| e.to_string())
}
