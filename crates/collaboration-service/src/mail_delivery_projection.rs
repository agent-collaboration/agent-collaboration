//! Receive and outcome hooks share the library's transaction and preserve full native receipts.
use crate::{RouterDeliveryTypes, received_mail_push};
use chrono::{DateTime, Utc};
use collaboration_mail_protocol::CarrierAcceptance;
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, MachineId, MessageSendResult, PushMessageSendResult,
    RouterLink,
};
use durable_steps::{Host, turso_store::TursoScope};
use fleet_sync_delivery::DeliveryError;
use fleet_sync_protocol::{DeliveryItem, ItemId, RecipientOutcome};
use interaction_storage::{
    MailPushUpdate, apply_mail_push_outcome_in_scope, clear_local_delivery_retry_in_scope,
    insert_mail_push_in_scope, read_mail_push_in_scope, read_message_send_answer_in_scope,
    record_local_delivery_retry_in_scope, record_message_send_outcome,
};

pub async fn project_received_mail<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &DeliveryItem<RouterDeliveryTypes>,
    received_at: DateTime<Utc>,
) -> Result<(), DeliveryError> {
    if crate::received_mail_push::is_poll_mail(item) {
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
            return Err(invalid_projection());
        };
        let collaboration_mail_protocol::NotificationSource::FollowBatch {
            membership, roots, ..
        } = &body.source
        else {
            return Err(invalid_projection());
        };
        if roots.len() != 1 {
            return Err(invalid_projection());
        }
        interaction_storage::insert_pending_poll_in_scope(
            scope,
            &interaction_storage::PendingPollMail {
                item: item.id(),
                recipient: item
                    .recipient()
                    .address
                    .clone()
                    .try_into()
                    .map_err(|_| invalid_projection())?,
                membership: membership
                    .as_str()
                    .to_owned()
                    .try_into()
                    .map_err(|_| invalid_projection())?,
                received_at,
            },
        )
        .await
        .map_err(storage_error)?;
    }
    if let Some(draft) = received_mail_push(item, received_at)? {
        insert_mail_push_in_scope(scope, draft)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

pub async fn project_mail_outcome<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: ItemId,
    outcome: &RecipientOutcome<CarrierAcceptance>,
    observation: Option<&crate::RouterLocalObservation>,
    recorded_at: DateTime<Utc>,
) -> Result<(), DeliveryError> {
    // A cancel's own outcome cannot rewrite its target's earlier receipt.
    if matches!(outcome, RecipientOutcome::TooLate { .. }) {
        return Ok(());
    }
    if !matches!(outcome, RecipientOutcome::Held) {
        interaction_storage::delete_pending_poll_in_scope(scope, item)
            .await
            .map_err(storage_error)?;
    }
    let push_id = item
        .as_uuid()
        .to_string()
        .try_into()
        .map_err(|_| invalid_projection())?;
    let Some(current) = read_mail_push_in_scope(scope, &push_id)
        .await
        .map_err(storage_error)?
    else {
        // Poll/control items have no push, and retention never recreates a pruned one.
        if !matches!(outcome, RecipientOutcome::Held) {
            clear_local_delivery_retry_in_scope(scope, &push_id)
                .await
                .map_err(storage_error)?;
        }
        return Ok(());
    };
    let is_follow = current.kind == collaboration_protocol::PushKind::SubscriptionActivity;
    let receipt_observation = observation.map(|observed| &observed.receipt);
    let receipt = outcome_receipt(
        outcome,
        receipt_observation,
        current.last_outcome.as_ref(),
        is_follow,
    )?;
    let update = if matches!(outcome, RecipientOutcome::Held) {
        MailPushUpdate::Held {
            receipt,
            held_at: recorded_at,
        }
    } else {
        MailPushUpdate::Finished {
            receipt,
            settled_at: recorded_at,
        }
    };
    let record = apply_mail_push_outcome_in_scope(scope, &push_id, update)
        .await
        .map_err(storage_error)?
        .ok_or_else(invalid_projection)?;
    interaction_storage::record_mail_route_evidence_in_scope(
        scope,
        &record,
        observation.and_then(|observed| observed.route_evidence.as_ref()),
    )
    .await
    .map_err(storage_error)?;
    if matches!(outcome, RecipientOutcome::Held) {
        let scheduled_retry_at = observation
            .and_then(|observed| observed.scheduled_retry_at)
            .map(|until| {
                DateTime::from_timestamp_millis(until.unix_millis()).ok_or_else(invalid_projection)
            })
            .transpose()?;
        record_local_delivery_retry_in_scope(
            scope,
            &push_id,
            is_follow && receipt_observation.is_some_and(is_known_follow_failure),
            scheduled_retry_at,
        )
        .await
        .map_err(storage_error)?;
    } else {
        clear_local_delivery_retry_in_scope(scope, &push_id)
            .await
            .map_err(storage_error)?;
    }
    let item_id = item
        .as_uuid()
        .to_string()
        .try_into()
        .map_err(|_| invalid_projection())?;
    if let Some(answer) = read_message_send_answer_in_scope(scope, &item_id)
        .await
        .map_err(storage_error)?
    {
        let result = PushMessageSendResult {
            push_id: record.push_id.clone(),
            link: RouterLink::new(
                MachineId::from(record.target.endpoint.service_id.clone()),
                record.push_id,
            )
            .to_string(),
            target: record.target,
            target_identity: answer.target_identity,
            delivery_state: record.delivery_state,
            receipt: record.last_outcome.ok_or_else(invalid_projection)?,
        };
        record_message_send_outcome(scope, &item_id, &MessageSendResult::Local(result))
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

/// A control records its own Local attribution outcome before marking an existing target.
pub async fn project_mail_cancel_outcome<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    cancel: ItemId,
    target: ItemId,
    outcome: &RecipientOutcome<CarrierAcceptance>,
    recorded_at: DateTime<Utc>,
) -> Result<(), DeliveryError> {
    let cancel_id = cancel
        .as_uuid()
        .to_string()
        .try_into()
        .map_err(|_| invalid_projection())?;
    let target_id = target
        .as_uuid()
        .to_string()
        .try_into()
        .map_err(|_| invalid_projection())?;
    interaction_storage::record_local_cancel_outcome_in_scope(
        scope, &cancel_id, &target_id, outcome,
    )
    .await
    .map_err(storage_error)?;
    if let RecipientOutcome::TooLate { at, .. } = outcome {
        project_cancelled_after_delivery(scope, cancel, target, *at, recorded_at).await?;
    }
    Ok(())
}

/// Called only by the SDK control-outcome hook's TooLate branch.
pub async fn project_cancelled_after_delivery<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    cancel: ItemId,
    target: ItemId,
    too_late_at: fleet_sync_protocol::TooLateAt,
    recorded_at: DateTime<Utc>,
) -> Result<(), DeliveryError> {
    let cancel = cancel
        .as_uuid()
        .to_string()
        .try_into()
        .map_err(|_| invalid_projection())?;
    let target = target
        .as_uuid()
        .to_string()
        .try_into()
        .map_err(|_| invalid_projection())?;
    let too_late_at = match too_late_at {
        fleet_sync_protocol::TooLateAt::Delivered => {
            collaboration_mail_protocol::TooLateAt::Delivered
        }
        fleet_sync_protocol::TooLateAt::Uncertain => {
            collaboration_mail_protocol::TooLateAt::Uncertain
        }
    };
    let mark = collaboration_protocol::CancelledAfterDelivery {
        cancel,
        too_late_at,
        recorded_at,
    };
    interaction_storage::record_cancelled_after_delivery_in_scope(scope, &target, &mark)
        .await
        .map_err(storage_error)
}

fn outcome_receipt(
    outcome: &RecipientOutcome<CarrierAcceptance>,
    observation: Option<&DeliveryReceipt>,
    previous: Option<&DeliveryReceipt>,
    is_follow: bool,
) -> Result<DeliveryReceipt, DeliveryError> {
    match outcome {
        RecipientOutcome::Delivered { acceptance } => {
            let receipt = observation.or(previous).ok_or_else(invalid_projection)?;
            if !acceptance_matches(acceptance, &receipt.outcome) {
                return Err(invalid_projection());
            }
            Ok(receipt.clone())
        }
        RecipientOutcome::Held => {
            let receipt = observation.or(previous).ok_or_else(invalid_projection)?;
            if !matches!(
                receipt.outcome,
                DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    ..
                }
            ) && !(is_follow && is_known_follow_failure(receipt))
            {
                return Err(invalid_projection());
            }
            Ok(receipt.clone())
        }
        RecipientOutcome::Uncertain => {
            if let Some(receipt) = observation {
                if receipt.outcome != DeliveryOutcome::Unknown {
                    return Err(invalid_projection());
                }
                return Ok(receipt.clone());
            }
            Ok(DeliveryReceipt {
                outcome: DeliveryOutcome::Unknown,
                reachability: None,
                client: None,
            })
        }
        RecipientOutcome::Expired
        | RecipientOutcome::Prevented { .. }
        | RecipientOutcome::Dropped
        | RecipientOutcome::Refused { .. } => {
            if let Some(receipt) = observation {
                if !matches!(
                    receipt.outcome,
                    DeliveryOutcome::NotSubmitted { .. } | DeliveryOutcome::Rejected(_)
                ) {
                    return Err(invalid_projection());
                }
                return Ok(receipt.clone());
            }
            let reason = match outcome {
                RecipientOutcome::Expired => "mail expired before local delivery".to_owned(),
                RecipientOutcome::Prevented { .. } => {
                    "mail cancelled before local delivery".to_owned()
                }
                RecipientOutcome::Dropped => "mail dropped by recipient policy".to_owned(),
                RecipientOutcome::Refused { reason } => format!(
                    "mail refused before local delivery: {}",
                    serde_json::to_string(reason).map_err(|_| invalid_projection())?
                ),
                _ => return Err(invalid_projection()),
            };
            Ok(DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    retryable: false,
                    reason,
                },
                reachability: None,
                client: None,
            })
        }
        RecipientOutcome::TooLate { .. } => Err(invalid_projection()),
    }
}

fn is_known_follow_failure(receipt: &DeliveryReceipt) -> bool {
    matches!(
        receipt.outcome,
        DeliveryOutcome::Rejected(_)
            | DeliveryOutcome::NotSubmitted {
                retryable: false,
                ..
            }
    )
}

fn acceptance_matches(acceptance: &CarrierAcceptance, outcome: &DeliveryOutcome) -> bool {
    matches!(
        (acceptance, outcome),
        (CarrierAcceptance::Started, DeliveryOutcome::Started)
            | (CarrierAcceptance::Steered, DeliveryOutcome::Steered)
            | (
                CarrierAcceptance::StartedOrSteered,
                DeliveryOutcome::StartedOrSteered
            )
            | (CarrierAcceptance::Queued, DeliveryOutcome::Queued)
            | (
                CarrierAcceptance::PeerMessageWritten,
                DeliveryOutcome::PeerMessageWritten
            )
    )
}
fn invalid_projection() -> DeliveryError {
    DeliveryError::Corrupt {
        field: "native mail outcome projection",
    }
}
fn storage_error(error: interaction_storage::StorageError) -> DeliveryError {
    match error {
        interaction_storage::StorageError::Database(cause) => {
            DeliveryError::Storage(cause.to_string())
        }
        error => DeliveryError::Storage(error.to_string()),
    }
}

#[cfg(test)]
#[path = "mail_delivery_projection_tests.rs"]
pub(crate) mod tests;
