//! Library-owned startup markers commit with their custody changes and admissions.
use super::{AtomicStore, MailDeliveryRuntime, RemoveMailRecord, RouterDeliveryTypes, StoreError};
use fleet_sync_delivery::{admit_existing_record_removals, backfill_submitters, delivery_rollback};
use runtime_primitives::Clock;

impl MailDeliveryRuntime {
    pub(crate) async fn initialize_record_removals(&self) -> Result<usize, StoreError> {
        let now = self.host.now();
        #[cfg(test)]
        let probe = self.backfill_admission_probe.clone();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let admitted = admit_existing_record_removals::<_, RemoveMailRecord>(
                        scope,
                        fleet_sync_delivery::DeliveryHostRole::Machine,
                        now,
                    )
                    .await
                    .map_err(delivery_rollback)?;
                    #[cfg(test)]
                    if let Some(socket) = probe {
                        super::local_cancellation::pause_cancel_admission(&socket).await?;
                    }
                    Ok(admitted)
                })
            })
            .await
    }

    pub(crate) async fn initialize_submitter_bindings(&self) -> Result<usize, StoreError> {
        let binding = self.binding.clone();
        let now = self.host.now();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    backfill_submitters::<RouterDeliveryTypes, _, _>(
                        scope,
                        fleet_sync_delivery::DeliveryHostRole::Machine,
                        binding.as_ref(),
                        now,
                    )
                    .await
                    .map_err(delivery_rollback)
                })
            })
            .await
    }
}
