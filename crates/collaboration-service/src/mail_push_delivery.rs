//! Recipient push policy and native I/O; custody and outcome writes remain in the library.
use crate::SubscriptionClock;
use crate::SubscriptionDeliveryService;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_runtime::MailPushPreparation;
use collaboration_runtime::mail_push_preparation::prepare_mail_push_with_load_policy;
use collaboration_runtime::received_mail_push;
use fleet_sync_delivery::{DeliveryError, LocalDelivery, LocalOutcome};
use fleet_sync_protocol::DeliveryItem;
use interaction_storage::InteractionStore;
use runtime_primitives::{CallId, SyncTime};
use std::sync::Arc;

pub struct MailPushDeliveryProps {
    pub interactions: Arc<InteractionStore>,
    pub actors: SubscriptionDeliveryService,
    pub presence: Arc<dyn TargetPresenceProbe>,
    pub machine: MachineIdentity,
    pub clock: Arc<dyn SubscriptionClock>,
}

pub struct MailPushDelivery {
    props: MailPushDeliveryProps,
}
impl MailPushDelivery {
    pub fn new(props: MailPushDeliveryProps) -> Self {
        Self { props }
    }

    /// Delivers the frozen push through its reader's native actor; the caller persists custody.
    /// Poll returns None for the caller's separate waiter handoff, with no native call.
    pub async fn deliver_push(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        call: &CallId,
    ) -> Result<Option<LocalDelivery<RouterDeliveryTypes>>, DeliveryError> {
        self.deliver_push_with_load_policy(item, call, LoadPolicy::LoadedOnly)
            .await
    }

    pub(crate) async fn deliver_push_with_load_policy(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        call: &CallId,
        direct_load_policy: LoadPolicy,
    ) -> Result<Option<LocalDelivery<RouterDeliveryTypes>>, DeliveryError> {
        let is_follow = matches!(item.body(), board_sync_protocol::BoardDeliveryBody::Notification(notification)
            if matches!(notification.source, collaboration_mail_protocol::NotificationSource::FollowBatch { .. }));
        let Some(draft) = received_mail_push(item, self.props.clock.now())? else {
            return Ok(None);
        };
        let working = self
            .props
            .interactions
            .get_push_record(&draft.push_id)
            .await
            .map_err(storage_error)?
            .ok_or(DeliveryError::CustodyMissing)?;
        let allow_hold = draft.mode != Some(collaboration_protocol::MessageDelivery::Steer)
            && draft.guard.is_none();
        let attempts = if is_follow {
            self.props
                .interactions
                .local_delivery_retry_attempts(&draft.push_id)
                .await
                .map_err(storage_error)?
        } else {
            0
        };
        let presence = self
            .props
            .presence
            .presence(&working.target)
            .await
            .unwrap_or_else(|error| TargetPresence::Unreachable {
                reason: error.to_string(),
            });
        let Some(preparation) = prepare_mail_push_with_load_policy(
            item,
            &working,
            &self.props.machine,
            &presence,
            call,
            direct_load_policy,
        )?
        else {
            return Ok(None);
        };
        let delivery = match preparation {
            MailPushPreparation::Hold { receipt } => LocalDelivery {
                outcome: LocalOutcome::Held { until: None },
                observation: receipt.into(),
            },
            MailPushPreparation::Drop { receipt } => LocalDelivery {
                outcome: LocalOutcome::Dropped,
                observation: receipt.into(),
            },
            MailPushPreparation::Refuse { delivery } => {
                crate::mail_push_outcome::local_message_outcome(delivery, allow_hold)?
            }
            MailPushPreparation::Attempt { kind, request } => {
                let delivery = if is_follow {
                    let MailSender::Project { project } = item.sender() else {
                        return Err(DeliveryError::Corrupt {
                            field: "follow sender",
                        });
                    };
                    self.props
                        .actors
                        .attempt_follow_mail_once(
                            project.clone(),
                            fleet_sync_protocol::DeliveryItemRef {
                                item: item.id(),
                                digest: item.digest().clone(),
                            },
                            *request,
                        )
                        .await
                } else {
                    self.props.actors.attempt_mail_once(kind, *request).await
                };
                if is_follow {
                    crate::mail_push_outcome::local_follow_outcome(
                        delivery.into_local_observation(),
                        attempts,
                        self.now(),
                    )
                } else {
                    crate::mail_push_outcome::local_message_outcome(delivery, allow_hold)?
                }
            }
        };
        Ok(Some(delivery))
    }

    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.props.clock.now().timestamp_millis())
    }
}
use collaboration_mail_protocol::MailSender;

fn storage_error(error: interaction_storage::StorageError) -> DeliveryError {
    DeliveryError::Storage(error.to_string())
}

#[cfg(test)]
#[path = "mail_push_delivery_tests.rs"]
mod tests;
