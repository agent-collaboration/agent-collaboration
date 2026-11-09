//! The migration Worker's inventory answer, and the candidate it carries: every
//! object exactly as migrate would leave it (tools/central-migration
//! `migration_candidate.ts`), with each stored value's SQLite storage class.

use serde::Deserialize;
use std::collections::BTreeMap;

/// The parts of `POST /admin/v1/migration/inventory`'s answer the gate reads.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryAnswer {
    pub kind: String,
    pub stops: Vec<serde_json::Value>,
    pub run_time_millis: i64,
    pub candidate_digest: Option<String>,
    pub candidate_text: Option<String>,
}

/// One stored value with its storage class: integers as decimal text (exact
/// beyond 2^53), blobs as lower-case hex.
#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum TypedValue {
    Null,
    Integer(String),
    Real(f64),
    Text(String),
    Blob(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaEntry {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub sql: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableImage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<TypedValue>>,
}

/// One object's storage: schema in creation order, then every table's rows.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectImage {
    pub schema: Vec<SchemaEntry>,
    pub tables: BTreeMap<String, TableImage>,
}

impl ObjectImage {
    /// The text values of one column, in row order; empty when the table is absent.
    pub fn text_column(&self, table: &str, column: &str) -> Vec<String> {
        let Some(image) = self.tables.get(table) else {
            return Vec::new();
        };
        let Some(index) = image.columns.iter().position(|name| name == column) else {
            return Vec::new();
        };
        image
            .rows
            .iter()
            .filter_map(|row| match row.get(index) {
                Some(TypedValue::Text(value)) => Some(value.clone()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub fleet: ObjectImage,
    /// Each Copy without its project tables, keyed by project.
    pub copies: BTreeMap<String, ObjectImage>,
}
