use collaboration_replication_protocol::{
    Epoch, InstallationId, ProjectId, ReplicationPosition, SchemaIdentity, SqlValue,
};

use crate::{BoundStatement, CentralError, CopyStatus, SqlRow, SqlTx, StoreError};

pub(crate) fn text(value: impl Into<String>) -> SqlValue {
    SqlValue::Text(value.into())
}
pub(crate) fn unsigned(value: u64) -> SqlValue {
    text(value.to_string())
}
pub(crate) fn statement(sql: &str, parameters: Vec<SqlValue>) -> BoundStatement {
    BoundStatement::new(sql, parameters)
}

pub(crate) fn query_statement(
    sql: &str,
    parameters: Vec<SqlValue>,
    columns: impl IntoIterator<Item = impl Into<String>>,
) -> BoundStatement {
    BoundStatement::query(
        sql,
        parameters,
        columns.into_iter().map(Into::into).collect(),
    )
}

pub(crate) fn epoch_field(row: &SqlRow, name: &str) -> Result<Epoch, StoreError> {
    Epoch::new(row.unsigned(name)?).map_err(|_| StoreError::Corrupt { field: name.into() })
}

pub(crate) fn position_fields(
    row: &SqlRow,
    prefix: &str,
) -> Result<ReplicationPosition, StoreError> {
    Ok(ReplicationPosition::new(
        epoch_field(row, &format!("{prefix}_epoch"))?,
        row.unsigned(&format!("{prefix}_commit"))?.into(),
    ))
}

pub(crate) fn load_copy(tx: &mut dyn SqlTx) -> Result<Option<CopyStatus>, CentralError> {
    let rows = tx.query(query_statement("SELECT project, holder, fence_epoch, start_epoch, start_commit, acked_epoch, acked_commit, schema_version, schema_fingerprint, archived FROM _copy_state WHERE singleton=1", Vec::new(), ["project", "holder", "fence_epoch", "start_epoch", "start_commit", "acked_epoch", "acked_commit", "schema_version", "schema_fingerprint", "archived"]))?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let archived = match row.integer("archived")? {
        0 => false,
        1 => true,
        _ => {
            return Err(StoreError::Corrupt {
                field: "archived".into(),
            }
            .into());
        }
    };
    let imported: Option<crate::import_rules::StoredImport> = crate::registry_storage::read_record(
        tx,
        "SELECT record FROM _copy_import_metadata WHERE singleton=?",
        "1",
    )?;
    let project = ProjectId::new(row.text("project")?).map_err(|_| StoreError::Corrupt {
        field: "project".into(),
    })?;
    let holder = match row.value("holder")? {
        SqlValue::Null => None,
        SqlValue::Text(value) => {
            Some(
                InstallationId::new(value.clone()).map_err(|_| StoreError::Corrupt {
                    field: "holder".into(),
                })?,
            )
        }
        _ => {
            return Err(StoreError::Corrupt {
                field: "holder".into(),
            }
            .into());
        }
    };
    let schema = SchemaIdentity::new(
        row.unsigned("schema_version")?,
        row.text("schema_fingerprint")?.into(),
    )
    .map_err(|_| StoreError::Corrupt {
        field: "schema_fingerprint".into(),
    })?;
    if imported.as_ref().is_some_and(|record| {
        record.import_epoch.value() != 1
            || record.header.project != project
            || record.header.head.epoch != record.import_epoch
            || record.header.schema.version > schema.version
    }) {
        return Err(StoreError::Corrupt {
            field: "import metadata".into(),
        }
        .into());
    }
    let state = CopyStatus {
        project,
        holder,
        schema,
        archived,
        imported: imported.as_ref().map(|record| crate::ImportedCopy {
            digest: record.digest.clone(),
            import_epoch: record.import_epoch,
        }),
        fence: epoch_field(row, "fence_epoch")?,
        start: position_fields(row, "start")?,
        acked: position_fields(row, "acked")?,
    };
    if state.holder.is_none()
        && imported.as_ref().is_none_or(|record| {
            state.fence != record.import_epoch
                || state.acked != record.header.head
                || state.start != ReplicationPosition::new(record.import_epoch, 0.into())
        })
    {
        return Err(StoreError::Corrupt {
            field: "unheld copy state".into(),
        }
        .into());
    }
    if state.fence < state.acked.epoch || state.start > state.acked || rows.len() != 1 {
        return Err(StoreError::Corrupt {
            field: "copy_state".into(),
        }
        .into());
    }
    Ok(Some(state))
}

pub(crate) fn save_copy(tx: &mut dyn SqlTx, state: &CopyStatus) -> Result<(), CentralError> {
    tx.execute(statement(
        "INSERT INTO _copy_state(singleton, project, holder, fence_epoch, start_epoch, start_commit, acked_epoch, acked_commit, schema_version, schema_fingerprint, archived) VALUES(1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(singleton) DO UPDATE SET project=excluded.project, holder=excluded.holder, fence_epoch=excluded.fence_epoch, start_epoch=excluded.start_epoch, start_commit=excluded.start_commit, acked_epoch=excluded.acked_epoch, acked_commit=excluded.acked_commit, schema_version=excluded.schema_version, schema_fingerprint=excluded.schema_fingerprint, archived=excluded.archived",
        vec![text(state.project.as_str()), state.holder.as_ref().map_or(SqlValue::Null,|holder|text(holder.as_str())), unsigned(state.fence.value()), unsigned(state.start.epoch.value()), unsigned(state.start.commit_id.value()), unsigned(state.acked.epoch.value()), unsigned(state.acked.commit_id.value()), unsigned(state.schema.version), text(state.schema.fingerprint.clone()), SqlValue::Integer(i64::from(state.archived))],
    ))?;
    Ok(())
}

pub(crate) fn encode_record<TRecord: serde::Serialize>(
    record: &TRecord,
) -> Result<String, CentralError> {
    serde_json::to_string(record).map_err(|error| CentralError::Encoding {
        message: error.to_string(),
    })
}

pub(crate) fn decode_record<TRecord: serde::de::DeserializeOwned>(
    value: &str,
    field: &str,
) -> Result<TRecord, CentralError> {
    serde_json::from_str(value).map_err(|_| {
        StoreError::Corrupt {
            field: field.into(),
        }
        .into()
    })
}
