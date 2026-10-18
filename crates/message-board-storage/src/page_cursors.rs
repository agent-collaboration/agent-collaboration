//! Preserve the board's signed cursor wire format with its Router-local key.
use crate::{CursorSigningKey, RouterLocalStorageError};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Serialize, de::DeserializeOwned};
use sha2::Sha256;
pub fn encode_board_cursor<TPayload: Serialize>(
    key: &CursorSigningKey,
    payload: &TPayload,
) -> Result<String, RouterLocalStorageError> {
    let payload = serde_json::to_vec(payload)?;
    let mut signer = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|_| RouterLocalStorageError::InvalidCursor)?;
    signer.update(&payload);
    Ok(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(signer.finalize().into_bytes())
    ))
}
pub fn decode_board_cursor<TPayload: DeserializeOwned>(
    key: &CursorSigningKey,
    cursor: &str,
) -> Result<TPayload, RouterLocalStorageError> {
    let (payload, signature) = cursor
        .split_once('.')
        .ok_or(RouterLocalStorageError::InvalidCursor)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| RouterLocalStorageError::InvalidCursor)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| RouterLocalStorageError::InvalidCursor)?;
    let mut verifier = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|_| RouterLocalStorageError::InvalidCursor)?;
    verifier.update(&payload);
    verifier
        .verify_slice(&signature)
        .map_err(|_| RouterLocalStorageError::InvalidCursor)?;
    serde_json::from_slice(&payload).map_err(|_| RouterLocalStorageError::InvalidCursor)
}
