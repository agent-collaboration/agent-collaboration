//! Conformance reads the owning Fleet schema without applying or repairing migrations.
use crate::fleet_record_codec::required;
use durable_steps::{StoreError, storage::TransactionScope};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode, types::Type,
};

pub const FLEET_SCHEMA_PATH: &str = "/admin/v1/conformance/fleet-schema";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetSchemaObject {
    Table {
        name: String,
        definition: String,
    },
    Index {
        name: String,
        table: String,
        definition: Option<String>,
    },
}

pub(crate) async fn read_schema<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Vec<FleetSchemaObject>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    crate::generated_fleet_schema::read_fleet_schema(scope)
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            let name = required(row.object_name, "schema.object_name")?;
            match required(row.object_kind, "schema.object_kind")?.as_str() {
                "table" => Ok(FleetSchemaObject::Table {
                    name,
                    definition: required(row.definition, "schema.definition")?,
                }),
                "index" => Ok(FleetSchemaObject::Index {
                    name,
                    table: required(row.table_name, "schema.table_name")?,
                    definition: row.definition,
                }),
                _ => Err(StoreError::backend("unexpected schema object kind")),
            }
        })
        .collect()
}
