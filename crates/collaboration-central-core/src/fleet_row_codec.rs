//! Validate scalar columns shared by Fleet storage codecs.
use durable_steps::StoreError;

pub(crate) fn required(value: Option<String>, field: &str) -> Result<String, StoreError> {
    value.ok_or_else(|| corrupt(field, "required Fleet column is NULL"))
}

pub(crate) fn check_millis(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<(), StoreError> {
    let value = required(value, field)?;
    let millis = value
        .parse::<i64>()
        .map_err(|error| corrupt(field, error))?;
    if required(class, field)? != "integer" || millis.to_string() != value {
        return Err(corrupt(
            field,
            "timestamp is not a canonical SQLite integer",
        ));
    }
    Ok(())
}

pub(crate) fn check_private_id(value: &str, field: &str) -> Result<(), StoreError> {
    let id = durable_steps::Uuid::parse_str(value).map_err(|error| corrupt(field, error))?;
    if !runtime_primitives::uuid_v7(id) || id.to_string() != value {
        return Err(corrupt(field, "identity must be canonical UUIDv7"));
    }
    Ok(())
}

pub(crate) fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("fleet.{field}"),
        message: message.to_string(),
    }
}
