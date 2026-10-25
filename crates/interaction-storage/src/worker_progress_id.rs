//! Immutable UUIDv7 identities of the moved worker progress rows.
use crate::StorageError;
use collaboration_protocol::UuidIdentity;

pub(crate) fn decode_worker_progress_id(value: String) -> Result<UuidIdentity, StorageError> {
    let identity =
        UuidIdentity::try_from(value.clone()).map_err(|_| StorageError::InvalidRecord)?;
    let bytes = value.as_bytes();
    if bytes.get(14) != Some(&b'7') || !matches!(bytes.get(19), Some(b'8' | b'9' | b'a' | b'b')) {
        return Err(StorageError::InvalidRecord);
    }
    Ok(identity)
}
