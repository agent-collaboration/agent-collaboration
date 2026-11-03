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
                    let admitted =
                        admit_existing_record_removals::<_, RemoveMailRecord>(scope, now)
                            .await
                            .map_err(delivery_rollback)?;
                    #[cfg(test)]
                    if let Some(socket) = probe {
                        super::project_admission::pause_project_admission(&socket).await?;
                    }
                    Ok(admitted)
                })
            })
            .await
    }

    pub(crate) async fn initialize_submitter_bindings(&self) -> Result<usize, StoreError> {
        loop {
            let binding = self.binding.clone();
            let now = self.host.now();
            let result = self
                .store
                .transaction(move |scope| {
                    Box::pin(async move {
                        backfill_submitters::<RouterDeliveryTypes, _, _>(
                            scope,
                            binding.as_ref(),
                            now,
                        )
                        .await
                        .map_err(delivery_rollback)
                    })
                })
                .await;
            match result {
                Err(error) => {
                    if matches!(
                        fleet_sync_delivery::classify_delivery_store_error(error.clone()),
                        fleet_sync_delivery::DeliveryTransactionFailure::BindingUnavailable {
                            reason: fleet_sync_protocol::SenderBindingFailure::Unknown,
                            ..
                        }
                    ) && super::installation_directory::refresh_sender_bindings(
                        self.binding.as_ref(),
                        self.owner.as_ref(),
                        self.session.as_ref(),
                    )
                    .await?
                    {
                        continue;
                    }
                    return Err(error);
                }
                Ok(changed) => return Ok(changed),
            }
        }
    }
}
