//! The library owns schema canonicalization and hashing; storage excludes its local table.
use crate::ProjectStorageError;
use fleet_sync_protocol::{SchemaEntry, SchemaObjectKind};
use message_board::SchemaFingerprint;
use sqlx::Executor;
use sqlx_turso::Turso;

pub async fn schema_fingerprint<TScope>(
    transaction: &mut TScope,
) -> Result<SchemaFingerprint, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let rows=sqlx_turso::query!(r#"SELECT type AS "kind!: String",name AS "name!: String",tbl_name AS "owner!: String",sql AS "sql?: String" FROM sqlite_schema ORDER BY type,name"#).fetch_all(&mut *transaction).await?;
    let entries = rows
        .into_iter()
        .filter(|row| {
            !crate::project_bookkeeping::LOCAL_TABLES
                .iter()
                .any(|table| table.eq_ignore_ascii_case(&row.owner))
        })
        .map(|row| (row.kind, row.name, row.sql.unwrap_or_default()))
        .collect();
    fingerprint_entries(entries)
}

pub fn fingerprint_entries(
    entries: Vec<(String, String, String)>,
) -> Result<SchemaFingerprint, ProjectStorageError> {
    let entries = entries
        .into_iter()
        .filter(|(_, name, _)| !fleet_sync_protocol::reserved_namespace(name))
        .map(|(kind, name, sql)| {
            Ok(SchemaEntry {
                kind: SchemaObjectKind::try_from(kind.as_str())
                    .map_err(|_| ProjectStorageError::InvalidSchema)?,
                name,
                sql,
            })
        })
        .collect::<Result<Vec<_>, ProjectStorageError>>()?;
    let fingerprint = fleet_sync_protocol::schema_fingerprint(&entries)
        .map_err(|_| ProjectStorageError::InvalidSchema)?;
    SchemaFingerprint::try_from(fingerprint).map_err(|_| ProjectStorageError::InvalidSchema)
}
