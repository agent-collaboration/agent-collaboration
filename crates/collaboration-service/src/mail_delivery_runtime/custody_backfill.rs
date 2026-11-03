//! Upgrade admission runs once for this physical mail store, before its engine starts.
use super::{AtomicStore, MailDeliveryRuntime, RemoveMailRecord, StoreError};
use fleet_sync_delivery::{admit_existing_record_removals, delivery_rollback};
use interaction_storage::{
    complete_mail_removal_backfill_in_scope, mail_removal_backfill_completed_in_scope,
};

impl MailDeliveryRuntime {
    pub(crate) async fn initialize_record_removals(&self) -> Result<Option<usize>, StoreError> {
        let completed_at_ms = self.clock.now().timestamp_millis();
        #[cfg(test)]
        let probe = self.backfill_admission_probe.clone();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    if mail_removal_backfill_completed_in_scope(scope)
                        .await
                        .map_err(StoreError::backend)?
                    {
                        return Ok(None);
                    }
                    let admitted = admit_existing_record_removals::<_, RemoveMailRecord>(scope)
                        .await
                        .map_err(delivery_rollback)?;
                    complete_mail_removal_backfill_in_scope(scope, admitted, completed_at_ms)
                        .await
                        .map_err(StoreError::backend)?;
                    #[cfg(test)]
                    if let Some(socket) = probe {
                        super::project_admission::pause_project_admission(&socket).await?;
                    }
                    Ok(Some(admitted))
                })
            })
            .await
    }
}
