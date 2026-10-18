//! Read-triggered marks update replicated reader state without allocating activity or a ledger.
use crate::{
    ProjectStorageError, ProjectWriteTransaction,
    project_record_io::{read_record, validate_project_record},
};
use message_board::{ReadMark, Record};

impl ProjectWriteTransaction {
    pub async fn write_reader_mark(&mut self, mark: ReadMark) -> Result<(), ProjectStorageError> {
        let record = Record::ReadMark(mark);
        validate_project_record(&record, &self.project_id)?;
        let reference = record.reference();
        if record.envelope().position != message_board::RecordPosition::Unsequenced
            || !matches!(&record,Record::ReadMark(mark) if mark.envelope.actor.declared_identity()==Some(&mark.reader))
        {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "readerActor",
            });
        }
        let id = reference.id.as_str();
        let payload = serde_json::to_string(&record)?;
        if read_record(&mut self.transaction, &reference)
            .await?
            .is_some()
        {
            sqlx_turso::query!(
                "UPDATE record_identities SET payload = ? WHERE kind = ? AND id = ?",
                payload,
                "readMark",
                id
            )
            .execute(&mut *self.transaction)
            .await?;
        } else {
            sqlx_turso::query!(
                "INSERT INTO record_identities(kind, id, payload) VALUES (?, ?, ?)",
                "readMark",
                id,
                payload
            )
            .execute(&mut *self.transaction)
            .await?;
        }
        self.write_record_relations(&record).await
    }
}
