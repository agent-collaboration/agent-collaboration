//! The project's chosen submission epoch commits with its outgoing SDK custody.
use super::*;
use fleet_sync_delivery::{
    DeliveryTransactionFailure, Outbox, OutboxAdmission, classify_delivery_store_error,
    delivery_rollback,
};
use interaction_storage::{
    read_mail_submission_epoch_in_scope, record_mail_submission_epoch_in_scope,
};
use message_board::EpochNumber;

impl MailDeliveryRuntime {
    pub(crate) fn attach_project_replication(
        &self,
        replication: Arc<dyn crate::ProjectReplicationPosition>,
    ) -> Result<(), String> {
        if let Err(replication) = self.project_replication.set(replication)
            && !self
                .project_replication
                .get()
                .is_some_and(|existing| Arc::ptr_eq(existing, &replication))
        {
            return Err(
                "mail publisher already has a different project replication owner".to_owned(),
            );
        }
        Ok(())
    }

    pub(crate) async fn acknowledged_mail_changes(
        &self,
        project: &message_board::ProjectId,
    ) -> Result<message_board_storage::AcknowledgedProjectChanges, message_board::BoardError> {
        self.project_replication
            .get()
            .ok_or_else(message_board::BoardError::board_unavailable)?
            .acknowledged_mail_changes(project)
            .await
    }
    /// Internal publisher entry: the caller supplies its admitted writer epoch.
    pub async fn enqueue_project_item(
        &self,
        item: DeliveryItem<RouterDeliveryTypes>,
        epoch: EpochNumber,
    ) -> Result<OutboxAdmission<CarrierAcceptance>, DeliveryTransactionFailure> {
        let MailSender::Project { project } = item.sender() else {
            return Err(DeliveryTransactionFailure::Corrupt {
                field: "project mail sender".to_owned(),
            });
        };
        item.body().validate_sender(item.sender()).map_err(|_| {
            DeliveryTransactionFailure::Corrupt {
                field: "project mail body".to_owned(),
            }
        })?;
        let session = self.session.as_ref().ok_or_else(|| {
            DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(
                "project mail requires its configured relay",
            ))
        })?;
        let relay = session.relay;
        let project = project.clone();
        let id = item
            .id()
            .as_uuid()
            .to_string()
            .try_into()
            .map_err(|error| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
            })?;
        let limits = self.config.limits;
        let now = self.clock.now();
        let read_context = collaboration_protocol::MailReadContext::Project {
            project: project.clone(),
            handoff: item
                .id()
                .as_uuid()
                .to_string()
                .try_into()
                .map_err(|error| {
                    DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
                })?,
        };
        #[cfg(test)]
        let probe = self.project_admission_probe.clone();
        let (admission, has_effects) = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let (admission, operation) =
                        Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                            .enqueue_and_admit::<SubmitMailTask>(item, relay)
                            .await
                            .map_err(delivery_rollback)?;
                    #[cfg(test)]
                    if let Some(socket) = probe {
                        pause_project_admission(&socket).await?;
                    }
                    if matches!(admission, OutboxAdmission::CancelAlreadyRequested { .. }) {
                        // Originals cannot acquire another control's custody.
                        return Err(delivery_rollback(DeliveryError::Corrupt {
                            field: "project original returned a cancel admission",
                        }));
                    }
                    record_project_custody_epoch(scope, &admission, &id, &project, epoch, now)
                        .await?;
                    interaction_storage::record_mail_read_context_in_scope(
                        scope,
                        &id,
                        &read_context,
                        now,
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    Ok((admission, operation.is_some()))
                })
            })
            .await
            .map_err(classify_delivery_store_error)?;
        if has_effects {
            self.host.wake_at(self.host.now()).await.map_err(|error| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
            })?;
        }
        Ok(admission)
    }
}

pub(super) async fn record_project_custody_epoch(
    scope: &mut TursoScope<'_, MailHost>,
    admission: &OutboxAdmission<CarrierAcceptance>,
    item: &collaboration_protocol::MailItemId,
    project: &message_board::ProjectId,
    epoch: EpochNumber,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), StoreError> {
    match admission {
        OutboxAdmission::Queued { .. } => {
            record_mail_submission_epoch_in_scope(scope, item, project, epoch, now)
                .await
                .map_err(epoch_storage_error)?;
        }
        OutboxAdmission::Replayed { .. } => {
            // Re-holding never supplies today's epoch to historical SDK custody.
            let recorded = read_mail_submission_epoch_in_scope(scope, item)
                .await
                .map_err(epoch_storage_error)?
                .ok_or_else(|| {
                    delivery_rollback(DeliveryError::Corrupt {
                        field: "project mail submission epoch missing on replay",
                    })
                })?;
            if &recorded.project != project {
                return Err(delivery_rollback(DeliveryError::Corrupt {
                    field: "project mail submission epoch project",
                }));
            }
        }
        // No custody of this proposed child exists under another canonical control.
        OutboxAdmission::CancelAlreadyRequested { .. } => {}
        OutboxAdmission::Refused { reason } => {
            return Err(delivery_rollback(DeliveryError::Refused {
                reason: reason.clone(),
            }));
        }
    }
    Ok(())
}

fn epoch_storage_error(error: interaction_storage::StorageError) -> StoreError {
    match error {
        interaction_storage::StorageError::InvalidStoredRecord => {
            delivery_rollback(DeliveryError::Corrupt {
                field: "project mail submission epoch",
            })
        }
        error => StoreError::backend(error),
    }
}

#[cfg(test)]
pub(super) async fn pause_project_admission(socket: &std::path::Path) -> Result<(), StoreError> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut connection = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(StoreError::backend)?;
    connection
        .write_all(b"R")
        .await
        .map_err(StoreError::backend)?;
    let mut release = [0_u8; 1];
    connection
        .read_exact(&mut release)
        .await
        .map_err(StoreError::backend)?;
    Err(StoreError::backend(
        "project admission crash probe unexpectedly released",
    ))
}
