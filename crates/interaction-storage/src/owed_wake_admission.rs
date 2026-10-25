//! Wake admission reuses the automation-owned push identity after an interrupted handoff.
use crate::{InteractionStore, PushRecord, PushRecordDraft, StorageError};
use collaboration_protocol::{PushKind, PushOrigin};
use sqlx::Connection;

impl InteractionStore {
    /// Admits an automation-owed wake push or returns its previously committed record.
    pub async fn admit_owed_wake_push(
        &self,
        draft: PushRecordDraft,
    ) -> Result<PushRecord, StorageError> {
        let pending = draft
            .clone()
            .into_pending()
            .map_err(|_| StorageError::InvalidRecord)?;
        if pending.kind != PushKind::Wake || pending.origin != PushOrigin::Router(PushKind::Wake) {
            return Err(StorageError::InvalidRecord);
        }
        crate::push_record_rows::validate_router_origin_reference(
            pending.kind,
            &pending.origin,
            pending.origin_router_ref.as_deref(),
        )?;
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let admission = async {
            if let Some(stored) =
                crate::push_records::read_push_with_connection(&mut transaction, &pending.push_id)
                    .await?
            {
                if crate::push_record_rows::draft_for_record(&stored) != draft {
                    return Err(StorageError::PushContentConflict);
                }
                return Ok(stored);
            }
            crate::push_records::insert_push_record_with_connection(&mut transaction, &pending)
                .await?;
            Ok(pending)
        }
        .await;
        match admission {
            Ok(record) => {
                transaction.commit().await?;
                Ok(record)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}
