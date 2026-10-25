//! One ordered handoff queue retains automation's push effects until interaction commits.
use crate::{AutomationStore, OwedPushOperation, PushOperation, PushOperationId, StorageError};
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{Connection, SqliteConnection};

pub const MAX_OWED_PUSH_OPERATION_BATCH: u32 = 100;
struct OwedPushOperationRow {
    operation_id: String,
    push_id: String,
    operation: String,
    owed_at: String,
}
impl OwedPushOperationRow {
    fn decode(self) -> Result<OwedPushOperation, StorageError> {
        let record = OwedPushOperation {
            operation_id: self
                .operation_id
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
            push_id: self
                .push_id
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
            operation: serde_json::from_str(&self.operation)
                .map_err(|_| StorageError::InvalidRecord)?,
            owed_at: DateTime::parse_from_rfc3339(&self.owed_at)
                .map_err(|_| StorageError::InvalidRecord)?
                .with_timezone(&Utc),
        };
        record.validate()?;
        Ok(record)
    }
}
impl AutomationStore {
    pub async fn has_owed_push_operations(
        &mut self,
        push_id: &collaboration_protocol::PushId,
    ) -> Result<bool, StorageError> {
        let row = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM owed_push_operations WHERE push_id=?) AS \"owed!: bool\"",
            push_id.as_str()
        )
        .fetch_one(&mut self.connection)
        .await?;
        Ok(row.owed)
    }
    pub async fn list_owed_push_operations(
        &mut self,
        limit: u32,
    ) -> Result<Vec<OwedPushOperation>, StorageError> {
        if !(1..=MAX_OWED_PUSH_OPERATION_BATCH).contains(&limit) {
            return Err(StorageError::InvalidRecord);
        }
        sqlx::query_as!(
            OwedPushOperationRow,
            "SELECT operation_id,push_id,operation,owed_at FROM owed_push_operations ORDER BY operation_id LIMIT ?",
            i64::from(limit)
        )
        .fetch_all(&mut self.connection)
        .await?
        .into_iter()
        .map(OwedPushOperationRow::decode)
        .collect()
    }

    /// Call only after interaction storage commits admission of this exact input.
    pub async fn acknowledge_owed_push_operation(
        &mut self,
        expected: &OwedPushOperation,
    ) -> Result<(), StorageError> {
        expected.validate()?;
        let mut transaction = self.connection.begin_with("BEGIN IMMEDIATE").await?;
        let acknowledged = async {
            let row = sqlx::query_as!(
                OwedPushOperationRow,
                "SELECT operation_id,push_id,operation,owed_at FROM owed_push_operations WHERE operation_id=?",
                expected.operation_id.as_str()
            )
            .fetch_optional(&mut *transaction)
            .await?;
            if let Some(row) = row {
                if !row.decode()?.same_input(expected)? {
                    return Err(StorageError::PushContentConflict);
                }
                sqlx::query!(
                    "DELETE FROM owed_push_operations WHERE operation_id=?",
                    expected.operation_id.as_str()
                )
                .execute(&mut *transaction)
                .await?;
            }
            Ok::<_, StorageError>(())
        }
        .await;
        match acknowledged {
            Ok(()) => {
                transaction.commit().await?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}

pub(crate) async fn insert_owed_push_operation(
    connection: &mut SqliteConnection,
    operation: PushOperation,
    owed_at: DateTime<Utc>,
) -> Result<(), StorageError> {
    operation.validate()?;
    let operation_id = PushOperationId::generate();
    let push_id = operation.push_id().as_str();
    let encoded = serde_json::to_string(&operation).map_err(|_| StorageError::InvalidRecord)?;
    let owed_at = owed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
    sqlx::query!(
        "INSERT INTO owed_push_operations(operation_id,push_id,operation,owed_at) VALUES (?,?,?,?)",
        operation_id.as_str(),
        push_id,
        encoded,
        owed_at
    )
    .execute(connection)
    .await?;
    Ok(())
}
