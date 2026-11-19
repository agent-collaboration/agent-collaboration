//! Record central's delivery fact and recover the original request acceptance.
use crate::{
    MachineLocalStore,
    outbox_item_state::OutboxItemState,
    outbox_receipt_profile::AnsweredWrite,
    outbox_store::{
        OutboxStoreError, StoredPayload, outbox_content, payload_content_digest,
        read_decoded_outbox,
    },
};
use central_link_client::{
    OutboxCancellation, OutboxWriteTarget, RequestStatus, SenderDeliveryOutcome,
};
use central_link_protocol::ReplyPayload;
use runtime_primitives::SyncTime;
use sqlite_storage_maintenance::TransactionStart;
use workspace_model::{
    Caller,
    central_mail::{CentralMailAnswer, CentralMailResult, MessageOutcome},
};

pub(crate) async fn record_sender_delivery(
    store: &mut MachineLocalStore,
    caller: &Caller,
    notice: &SenderDeliveryOutcome,
    now: SyncTime,
) -> Result<bool, OutboxStoreError> {
    let mut transaction = store.transaction(TransactionStart::Immediate).await?;
    let Some(row) = read_decoded_outbox(&mut transaction, &notice.request_id).await? else {
        return Ok(false);
    };
    if row.caller != *caller {
        return Ok(false);
    }
    let may_have_left = matches!(
        row.state,
        OutboxItemState::Sent
            | OutboxItemState::Resent
            | OutboxItemState::SentAnswerLost
            | OutboxItemState::Unconfirmed
    );
    let accepted = confirmed_request(&row.status, may_have_left, notice)?;
    let mut profile = match row.payload {
        StoredPayload::Answered(profile) => profile,
        StoredPayload::Unanswered(payload) => AnsweredWrite {
            request_id: notice.request_id.clone(),
            target: outbox_content(notice.request_id.as_str(), &payload)?.target,
            content_digest: payload_content_digest(&payload)?,
            delivery_outcome: None,
        },
    };
    if !matches!(profile.target, OutboxWriteTarget::Session { .. }) {
        return Err(mismatch(notice));
    }
    match profile.delivery_outcome.as_ref() {
        Some(previous) if previous == notice => return Ok(true),
        Some(previous) if previous.inbox_message_id == notice.inbox_message_id => {
            return Err(mismatch(notice));
        }
        Some(previous) if previous.outcome == MessageOutcome::DeliveredAfterAll => {
            return Ok(false);
        }
        Some(_) | None => {}
    }
    profile.delivery_outcome = Some(notice.clone());
    sqlx::query(
        "UPDATE outbox_items SET item_state = ?, payload_json = ?, outcome_json = ?, updated_at = ?
         WHERE request_id = ?",
    )
    .bind(OutboxItemState::Answered.as_str())
    .bind(serde_json::to_string(&profile)?)
    .bind(serde_json::to_string(&accepted)?)
    .bind(now.unix_millis())
    .bind(notice.request_id.as_str())
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(true)
}

pub(crate) fn confirmed_request(
    status: &RequestStatus,
    may_have_left: bool,
    notice: &SenderDeliveryOutcome,
) -> Result<ReplyPayload, OutboxStoreError> {
    match status {
        RequestStatus::Answered(
            reply @ ReplyPayload::Mail(CentralMailResult::Done(CentralMailAnswer::Queued {
                message_id,
            })),
        ) if *message_id == notice.message_id => Ok(reply.clone()),
        RequestStatus::Sent
        | RequestStatus::InOutbox { .. }
        | RequestStatus::Ended {
            reason: OutboxCancellation::NotConfirmedWithin15Minutes,
        } if may_have_left => Ok(ReplyPayload::Mail(CentralMailResult::Done(
            CentralMailAnswer::Queued {
                message_id: notice.message_id.clone(),
            },
        ))),
        _ => Err(mismatch(notice)),
    }
}

fn mismatch(notice: &SenderDeliveryOutcome) -> OutboxStoreError {
    OutboxStoreError::DeliveryOutcomeMismatch {
        request: notice.request_id.clone(),
    }
}
