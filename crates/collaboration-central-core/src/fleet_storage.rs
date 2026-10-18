use crate::copy_storage::{encode_record, query_statement, statement, text};
use crate::{CentralError, SqlTx, StoreError};
use fleet_admission::FleetImportDigest;
use message_board::{FleetPosition, Record, RecordKind, RecordPosition, RecordRef};

pub(crate) fn corrupt(field: &str) -> CentralError {
    StoreError::Corrupt {
        field: field.into(),
    }
    .into()
}
pub(crate) fn bootstrap(tx: &mut dyn SqlTx) -> Result<(), CentralError> {
    for sql in [
        include_str!("../migrations/0004_fleet_storage.sql"),
        include_str!("../migrations/0005_archive_bookkeeping.sql"),
        include_str!("../migrations/0006_fleet_lease_order.sql"),
        include_str!("../migrations/0008_fleet_feed_retention.sql"),
        include_str!("../migrations/0010_fleet_notice_outbox.sql"),
        include_str!("../migrations/0011_tail_reports.sql"),
    ]
    .join(";")
    .split(';')
    .map(str::trim)
    .filter(|sql| !sql.is_empty())
    {
        tx.execute(statement(sql, Vec::new()))?;
    }
    Ok(())
}
pub(crate) fn kind_tag(kind: RecordKind) -> Result<&'static str, CentralError> {
    Ok(match kind {
        RecordKind::Workspace => "workspace",
        RecordKind::Board => "board",
        RecordKind::Project => "project",
        RecordKind::Adr => "adr",
        RecordKind::ProcessDefinition => "processDefinition",
        RecordKind::Request => "request",
        _ => return Err(corrupt("fleet record kind")),
    })
}
pub(crate) fn checkpoint(tx: &mut dyn SqlTx) -> Result<FleetPosition, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT last_position FROM fleet_checkpoint WHERE singleton=1",
        Vec::new(),
        ["last_position"],
    ))?;
    let row = rows.first().ok_or_else(|| corrupt("fleet checkpoint"))?;
    FleetPosition::try_from(
        u64::try_from(row.integer("last_position")?).map_err(|_| corrupt("fleet checkpoint"))?,
    )
    .map_err(|_| corrupt("fleet checkpoint"))
}
pub(crate) fn decode_record(payload: &str) -> Result<Record, CentralError> {
    let record: Record =
        serde_json::from_str(payload).map_err(|_| corrupt("fleet record payload"))?;
    // Reject constructor normalization of corrupt stored text. JSON remains
    // local to this boundary and never becomes the internal record model.
    let original: serde_json::Value =
        serde_json::from_str(payload).map_err(|_| corrupt("fleet record payload"))?;
    let canonical = serde_json::to_value(&record).map_err(|_| corrupt("fleet record payload"))?;
    if original != canonical {
        return Err(corrupt("fleet record canonical text"));
    }
    validate_record(&record)?;
    Ok(record)
}
fn validate_record(record: &Record) -> Result<(), CentralError> {
    let envelope = record.envelope();
    if envelope.id != record.reference().id
        || !matches!(envelope.position, RecordPosition::Fleet(_))
        || envelope.scope.project.is_some()
    {
        return Err(corrupt("fleet record envelope"));
    }
    let coherent = match record {
        Record::Workspace(value) => {
            value.workspace_id == envelope.scope.workspace && envelope.scope.board.is_none()
        }
        Record::Board(value) => {
            value.workspace_id == envelope.scope.workspace
                && envelope.scope.board.as_ref() == Some(&value.board_id)
        }
        Record::DirectoryEntry(value) => envelope.scope.board.as_ref() == Some(&value.board),
        Record::Adr(value) => {
            value.workspace == envelope.scope.workspace && envelope.scope.board.is_none()
        }
        Record::ProcessDefinition(_) => envelope.scope.board.is_none(),
        Record::Request(value) => value.observed_epoch.is_none() && value.accepted_in.is_none(),
        _ => false,
    };
    if !coherent {
        return Err(corrupt("fleet record scope"));
    }
    Ok(())
}
pub(crate) fn read_record(
    tx: &mut dyn SqlTx,
    reference: &RecordRef,
) -> Result<Option<Record>, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT payload,workspace FROM fleet_records WHERE kind=? AND id=?",
        vec![text(kind_tag(reference.kind)?), text(reference.id.as_str())],
        ["payload", "workspace"],
    ))?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let record = decode_record(row.text("payload")?)?;
    if record.reference() != *reference
        || record.envelope().scope.workspace.as_str() != row.text("workspace")?
    {
        return Err(corrupt("fleet record index"));
    }
    Ok(Some(record))
}
pub(crate) fn put_record(
    tx: &mut dyn SqlTx,
    record: &Record,
    digest: Option<&FleetImportDigest>,
) -> Result<(), CentralError> {
    validate_record(record)?;
    let reference = record.reference();
    let imported = digest.map_or(
        collaboration_replication_protocol::SqlValue::Null,
        |digest| text(digest.as_str()),
    );
    tx.execute(statement("INSERT INTO fleet_records(kind,id,workspace,payload,import_digest) VALUES(?,?,?,?,?) ON CONFLICT(kind,id) DO UPDATE SET workspace=excluded.workspace,payload=excluded.payload,import_digest=coalesce(fleet_records.import_digest,excluded.import_digest)", vec![text(kind_tag(reference.kind)?), text(reference.id.as_str()), text(record.envelope().scope.workspace.as_str()), text(encode_record(record)?), imported]))?;
    match record {
        Record::Workspace(value) => {
            tx.execute(statement("INSERT INTO fleet_workspaces(workspace_id,name) VALUES(?,?) ON CONFLICT(workspace_id) DO UPDATE SET name=excluded.name", vec![text(value.workspace_id.as_str()), text(value.name.as_str())]))?;
        }
        Record::Board(value) => {
            tx.execute(statement("INSERT INTO fleet_boards(board_id,workspace_id,name) VALUES(?,?,?) ON CONFLICT(board_id) DO UPDATE SET workspace_id=excluded.workspace_id,name=excluded.name", vec![text(value.board_id.as_str()), text(value.workspace_id.as_str()), text(value.name.as_str())]))?;
        }
        Record::DirectoryEntry(value) => {
            tx.execute(statement("INSERT INTO fleet_directory(project_id,board_id,title) VALUES(?,?,?) ON CONFLICT(project_id) DO UPDATE SET board_id=excluded.board_id,title=excluded.title", vec![text(value.id.as_str()), text(value.board.as_str()), text(value.title.as_str())]))?;
        }
        Record::ProcessDefinition(value) => {
            tx.execute(statement("INSERT INTO fleet_definitions(workspace_id,name,version,record_id) VALUES(?,?,?,?)", vec![text(record.envelope().scope.workspace.as_str()), text(value.reference.name.as_str()), text(value.reference.version.as_str()), text(reference.id.as_str())]))?;
        }
        Record::Adr(_) | Record::Request(_) => {}
        _ => return Err(corrupt("fleet record kind")),
    }
    if digest.is_some()
        && let Record::Workspace(value) = record
    {
        for repository in &value.repositories {
            tx.execute(statement(
                "INSERT OR IGNORE INTO fleet_repositories(workspace_id,reference) VALUES(?,?)",
                vec![
                    text(value.workspace_id.as_str()),
                    text(encode_record(repository)?),
                ],
            ))?;
        }
    }
    Ok(())
}
pub(crate) fn all_records(tx: &mut dyn SqlTx) -> Result<Vec<Record>, CentralError> {
    tx.query(query_statement(
        "SELECT payload FROM fleet_records ORDER BY kind,id",
        Vec::new(),
        ["payload"],
    ))?
    .iter()
    .map(|row| decode_record(row.text("payload")?))
    .collect()
}
