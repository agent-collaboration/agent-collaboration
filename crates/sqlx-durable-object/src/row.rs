use std::fmt::{self, Debug, Formatter};

use sqlx_core::column::ColumnIndex;
use sqlx_core::error::Error;
use sqlx_durable_object_core::Value;

use crate::database::{DoColumn, DoRow};

impl DoRow {
    /// The rows of one result, from the cursor's `columnNames` and the rows
    /// `raw()` yields.
    ///
    /// Each column is typed by its first non-NULL value, since the cursor
    /// reports no declared types. Every row shares the one column list.
    pub(crate) fn from_result(
        names: Vec<String>,
        rows: Vec<Vec<Value>>,
    ) -> Result<Vec<DoRow>, Error> {
        sqlx_durable_object_core::rows(
            names,
            rows,
            |name, ordinal, type_info| DoColumn {
                name,
                ordinal,
                type_info,
            },
            |columns, values| DoRow { columns, values },
        )
    }
}

impl ColumnIndex<DoRow> for &'_ str {
    /// The *last* column with this name, which is what sqlx-sqlite returns
    /// for `SELECT a.id, b.id`: it indexes names into a map, and the later
    /// insert wins. A linear scan is fine at the width of a real row.
    fn index(&self, row: &DoRow) -> Result<usize, Error> {
        row.columns
            .iter()
            .rposition(|column| column.name == *self)
            .ok_or_else(|| Error::ColumnNotFound((*self).into()))
    }
}

impl Debug for DoRow {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();

        for (column, value) in self.columns.iter().zip(self.values.iter()) {
            map.entry(&column.name, value);
        }

        map.finish()
    }
}

#[cfg(test)]
#[path = "row_test.rs"]
mod tests;
