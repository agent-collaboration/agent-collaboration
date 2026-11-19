//! The content digest a write is known by: central's request log stores it to
//! tell a replay (same id, same content) from a conflict (same id, different
//! content), and the Router keeps it for an answered outbox write after the
//! text is gone. It is a hash, not the text.
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
#[error("the operation could not be canonicalized for its digest")]
pub struct DigestError;

/// SHA-256, lowercase hex, over the operation's canonical JSON (RFC 8785).
pub fn operation_content_digest(operation: &impl Serialize) -> Result<String, DigestError> {
    use sha2::{Digest, Sha256};
    let encoded = serde_json_canonicalizer::to_vec(operation).map_err(|_| DigestError)?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

#[cfg(test)]
#[path = "request_digest_test.rs"]
mod tests;
