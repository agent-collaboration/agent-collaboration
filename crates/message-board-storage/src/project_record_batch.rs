//! Page-dependent payloads stay opaque until their consumer actually returns them.
use crate::project_record_io::{
    record_kind_tag, validate_canonical_project_text, validate_project_record,
};
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{ProjectId, Record, RecordId, RecordKind, RecordRef};

pub struct StoredProjectRecord {
    project: ProjectId,
    reference: RecordRef,
    payload: String,
    redacted: Option<bool>,
}
impl StoredProjectRecord {
    pub fn reference(&self) -> &RecordRef {
        &self.reference
    }

    pub fn decode(self) -> Result<Record, ProjectStorageError> {
        tracing::trace!(target: "message_board_storage::decode", decode_kind = "record");
        let mut record: Record = serde_json::from_str(&self.payload).map_err(|_| {
            self.invalid(crate::project_record_io::invalid_record_payload_field(
                self.reference.kind,
                &self.payload,
            ))
        })?;
        if record.reference() != self.reference || record.envelope().id != self.reference.id {
            return Err(self.invalid("id"));
        }
        validate_canonical_project_text(&self.payload, &record, &self.reference)?;
        validate_project_record(&record, &self.project)?;
        if let Record::WorkTrailRow(row) = &mut record {
            row.redacted = self.redacted.ok_or_else(|| self.invalid("redacted"))?;
        }
        Ok(record)
    }
    fn invalid(&self, field: &'static str) -> ProjectStorageError {
        ProjectStorageError::InvalidRecord {
            record: self.reference.clone(),
            field,
        }
    }
}

async fn record_batch(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
    references: &[RecordRef],
) -> Result<Vec<StoredProjectRecord>, ProjectStorageError> {
    let keys = serde_json::to_string(
        &references
            .iter()
            .map(|reference| (record_kind_tag(reference.kind), reference.id.as_str()))
            .collect::<Vec<_>>(),
    )?;
    // CROSS JOIN keeps requested keys outside the composite primary-key lookup.
    // Keep the outer-only kind predicate out of LEFT JOIN ON: native reopen can
    // discard unmatched records there (docs/upstream-workarounds/uw-4-turso-left-join.md).
    let rows = sqlx_turso::query!(r#"SELECT r.kind AS "kind!: String", r.id AS "id!: String", r.payload AS "payload!: String", CASE WHEN r.kind = 'workTrailRow' THEN t.redacted END AS "redacted: bool" FROM json_each(?) k CROSS JOIN record_identities r ON r.kind = json_extract(k.value, '$[0]') AND r.id = json_extract(k.value, '$[1]') LEFT JOIN work_trail_rows t ON t.row_id = r.id ORDER BY r.kind, r.id"#, keys).fetch_all(connection).await?;
    let references: std::collections::BTreeMap<_, _> = references
        .iter()
        .map(|key| ((record_kind_tag(key.kind), key.id.as_str()), key))
        .collect();
    rows.into_iter()
        .map(|row| {
            let reference = references
                .get(&(row.kind.as_str(), row.id.as_str()))
                .copied()
                .cloned()
                .ok_or(ProjectStorageError::InvalidFeed)?;
            Ok(StoredProjectRecord {
                project: project.clone(),
                reference,
                payload: row.payload,
                redacted: row.redacted,
            })
        })
        .collect()
}

impl ProjectSnapshot {
    pub async fn record_batch(
        &mut self,
        references: &[RecordRef],
    ) -> Result<Vec<StoredProjectRecord>, ProjectStorageError> {
        record_batch(&mut self.transaction, &self.project_id, references).await
    }
    pub async fn record_keys(
        &mut self,
        kind: RecordKind,
        after: &str,
        limit: u64,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let tag = record_kind_tag(kind);
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let rows = sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities WHERE kind = ? AND id > ? ORDER BY id LIMIT ?"#, tag, after, limit).fetch_all(&mut *self.transaction).await?;
        rows.into_iter()
            .map(|id| {
                Ok(RecordRef {
                    kind,
                    id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
                })
            })
            .collect()
    }
}
impl ProjectWriteTransaction {
    pub async fn record_batch(
        &mut self,
        references: &[RecordRef],
    ) -> Result<Vec<StoredProjectRecord>, ProjectStorageError> {
        record_batch(&mut self.transaction, &self.project_id, references).await
    }
}
