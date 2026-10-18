use crate::{FleetStorageError, project_record_io::record_kind_tag};
use message_board::{Record, RecordId, RecordKind, RecordPosition, RecordRef};

pub(crate) fn decode_fleet_reference(
    tag: &str,
    id: String,
) -> Result<RecordRef, FleetStorageError> {
    let kind = fleet_record_kind(tag)?;
    let field = match kind {
        RecordKind::Workspace => "workspaceId",
        RecordKind::Board => "boardId",
        RecordKind::Project => "projectId",
        RecordKind::Adr => "adrId",
        RecordKind::ProcessDefinition => "definitionId",
        RecordKind::Request => "requestId",
        _ => "id",
    };
    Ok(RecordRef {
        kind,
        id: RecordId::try_from(id).map_err(|_| FleetStorageError::InvalidRecordKey { field })?,
    })
}

pub(crate) fn decode_fleet_record(
    reference: &RecordRef,
    payload: &str,
) -> Result<Record, FleetStorageError> {
    tracing::trace!(target: "message_board_storage::decode", decode_kind = "record");
    let invalid = || FleetStorageError::InvalidRecordAt {
        record: reference.clone(),
    };
    let record: Record = serde_json::from_str(payload).map_err(|_| invalid())?;
    crate::project_record_io::validate_canonical_project_text(payload, &record, reference)
        .map_err(|_| invalid())?;
    validate_fleet_record(&record).map_err(|_| invalid())?;
    if record.reference() != *reference {
        return Err(invalid());
    }
    Ok(record)
}

pub(crate) fn validate_fleet_record(record: &Record) -> Result<(), FleetStorageError> {
    let envelope = record.envelope();
    if envelope.id != record.reference().id
        || !matches!(envelope.position, RecordPosition::Fleet(_))
        || envelope.scope.project.is_some()
    {
        return Err(FleetStorageError::InvalidRecord);
    }
    let valid = match record {
        Record::Workspace(row) => {
            row.workspace_id == envelope.scope.workspace && envelope.scope.board.is_none()
        }
        Record::Board(row) => {
            row.workspace_id == envelope.scope.workspace
                && envelope.scope.board.as_ref() == Some(&row.board_id)
        }
        Record::DirectoryEntry(row) => envelope.scope.board.as_ref() == Some(&row.board),
        Record::Adr(row) => {
            row.workspace == envelope.scope.workspace && envelope.scope.board.is_none()
        }
        Record::ProcessDefinition(_) => envelope.scope.board.is_none(),
        Record::Request(row) => row.observed_epoch.is_none() && row.accepted_in.is_none(),
        _ => false,
    };
    if !valid {
        return Err(FleetStorageError::InvalidRecord);
    }
    Ok(())
}

pub(crate) async fn read_fleet_record(
    connection: &mut sqlx_turso::TursoConnection,
    reference: &RecordRef,
) -> Result<Option<Record>, FleetStorageError> {
    let kind = record_kind_tag(reference.kind);
    let id = reference.id.as_str();
    let row = sqlx_turso::query!(r#"SELECT payload AS "payload!: String", workspace AS "workspace!: String" FROM fleet_records WHERE kind = ? AND id = ?"#, kind, id).fetch_optional(connection).await?;
    row.map(|row| {
        let record = decode_fleet_record(reference, &row.payload)?;
        if record.envelope().scope.workspace.as_str() != row.workspace {
            return Err(FleetStorageError::InvalidRecord);
        }
        Ok(record)
    })
    .transpose()
}

pub(crate) fn fleet_record_kind(tag: &str) -> Result<RecordKind, FleetStorageError> {
    match tag {
        "workspace" => Ok(RecordKind::Workspace),
        "board" => Ok(RecordKind::Board),
        "project" => Ok(RecordKind::Project),
        "adr" => Ok(RecordKind::Adr),
        "processDefinition" => Ok(RecordKind::ProcessDefinition),
        "request" => Ok(RecordKind::Request),
        _ => Err(FleetStorageError::InvalidRecord),
    }
}
