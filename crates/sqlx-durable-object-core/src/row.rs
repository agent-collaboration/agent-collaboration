use std::sync::Arc;

use sqlx_core::error::Error;

use crate::{TypeInfo, Value};

/// The rows of one result, from its column names and its rows of values.
///
/// Each column is typed by its first non-NULL value, since neither backend
/// reports declared types; `column` builds a driver's column from its name,
/// ordinal and that type, and every row shares the one column list. `row`
/// builds a driver's row from the columns and its values.
///
/// # Errors
///
/// When a row does not have one value per column.
pub fn rows<C, R>(
    names: Vec<String>,
    rows: Vec<Vec<Value>>,
    column: impl Fn(String, usize, TypeInfo) -> C,
    row: impl Fn(Arc<[C]>, Box<[Value]>) -> R,
) -> Result<Vec<R>, Error> {
    let columns: Arc<[C]> = names
        .into_iter()
        .enumerate()
        .map(|(ordinal, name)| {
            let type_info = rows
                .iter()
                .filter_map(|row| row.get(ordinal))
                .map(Value::type_info)
                .find(|type_info| *type_info != TypeInfo::Null)
                .unwrap_or(TypeInfo::Null);

            column(name, ordinal, type_info)
        })
        .collect();

    rows.into_iter()
        .map(|values| {
            if values.len() != columns.len() {
                return Err(Error::Protocol(format!(
                    "a row of {} values came back for {} columns",
                    values.len(),
                    columns.len()
                )));
            }

            Ok(row(Arc::clone(&columns), values.into_boxed_slice()))
        })
        .collect()
}

#[cfg(test)]
#[path = "row_test.rs"]
mod tests;
