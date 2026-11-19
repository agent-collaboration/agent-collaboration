//! Scalar storage checks shared by the machine and approval row codecs.
use central_storage::{ReadOutcome, SafeInteger, SqlValue, StoreFailure};
use chrono::{DateTime, SecondsFormat, Utc};
use fleet_identity_protocol::UnixSeconds;
use fleet_runtime_ports::stored_values::column;
use fleet_runtime_ports::stored_values::string;
use fleet_runtime_ports::workspace_decode_error::corrupt;
use uuid::Uuid;

pub(crate) fn identifier<T: TryFrom<String>>(value: &str, field: &str) -> Result<T, StoreFailure> {
    let parsed = Uuid::parse_str(value).map_err(|_| corrupt(field))?;
    if !fleet_runtime_ports::uuid_v7(parsed) || parsed.to_string() != value {
        return Err(corrupt(field));
    }
    T::try_from(value.to_owned()).map_err(|_| corrupt(field))
}

pub(crate) fn private_id(value: &str, field: &str) -> Result<Uuid, StoreFailure> {
    let parsed = Uuid::parse_str(value).map_err(|_| corrupt(field))?;
    if !fleet_runtime_ports::uuid_v7(parsed) || parsed.to_string() != value {
        return Err(corrupt(field));
    }
    Ok(parsed)
}

pub(crate) fn optional_text<'a>(
    rows: &ReadOutcome,
    row: &'a [SqlValue],
    field: &str,
) -> Result<Option<&'a str>, StoreFailure> {
    match column(rows, row, field)? {
        SqlValue::Null => Ok(None),
        SqlValue::Text(value) => Ok(Some(value)),
        _ => Err(corrupt(field)),
    }
}

pub(crate) fn nonnegative_integer(
    rows: &ReadOutcome,
    row: &[SqlValue],
    field: &str,
) -> Result<SafeInteger, StoreFailure> {
    match column(rows, row, field)? {
        SqlValue::Integer(value) if value.get() >= 0 => Ok(*value),
        _ => Err(corrupt(field)),
    }
}

pub(crate) fn millis(value: &str, field: &str) -> Result<i64, StoreFailure> {
    let parsed: DateTime<Utc> = value.parse().map_err(|_| corrupt(field))?;
    let milliseconds = parsed.timestamp_millis();
    if milliseconds < 0 || !parsed.timestamp_subsec_nanos().is_multiple_of(1_000_000) {
        return Err(corrupt(field));
    }
    SafeInteger::new(milliseconds).map_err(|_| corrupt(field))?;
    Ok(milliseconds)
}

pub(crate) fn seconds(value: &str, field: &str) -> Result<UnixSeconds, StoreFailure> {
    let milliseconds = millis(value, field)?;
    if milliseconds % 1000 != 0 {
        return Err(corrupt(field));
    }
    Ok(UnixSeconds(milliseconds / 1000))
}

pub(crate) fn row_millis(
    rows: &ReadOutcome,
    row: &[SqlValue],
    field: &str,
) -> Result<i64, StoreFailure> {
    millis(string(rows, row, field)?, field)
}

pub(crate) fn timestamp(milliseconds: i64, field: &str) -> Result<String, StoreFailure> {
    if milliseconds < 0 {
        return Err(corrupt(field));
    }
    SafeInteger::new(milliseconds).map_err(|_| corrupt(field))?;
    DateTime::from_timestamp_millis(milliseconds)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| corrupt(field))
}

pub(crate) fn seconds_timestamp(value: UnixSeconds, field: &str) -> Result<String, StoreFailure> {
    timestamp(
        value.0.checked_mul(1000).ok_or_else(|| corrupt(field))?,
        field,
    )
}

#[cfg(test)]
#[path = "machine_login_row_values_test.rs"]
mod tests;
