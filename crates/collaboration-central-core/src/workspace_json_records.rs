//! Stored JSON stays opaque in SQL; this decoder owns syntax and field attribution.
use crate::workspace_decode_error::corrupt;
use central_storage::StoreFailure;
use serde::de::DeserializeOwned;

pub(crate) fn decode<T: DeserializeOwned>(
    payload: &str,
    record: &str,
    fields: &[(&str, &str)],
) -> Result<T, StoreFailure> {
    let mut envelope: serde_json::Value =
        serde_json::from_str(payload).map_err(|_| corrupt(record))?;
    let object = envelope.as_object_mut().ok_or_else(|| corrupt(record))?;
    for (key, column) in fields {
        let encoded = object.get_mut(*key).ok_or_else(|| corrupt(column))?;
        if let serde_json::Value::String(text) = encoded {
            *encoded = serde_json::from_str(text).map_err(|_| corrupt(column))?;
        } else if !encoded.is_null() {
            return Err(corrupt(column));
        }
    }
    serde_json::from_value(envelope).map_err(|_| corrupt(record))
}
