//! One bounded transaction prunes settled interaction projections and eligible pushes.
use crate::{InteractionStore, StorageError, push_record_rows, push_records::MAX_PUSH_PRUNE_BATCH};
use chrono::{DateTime, Utc};
use collaboration_protocol::{PushDeliveryState, PushId};
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct PushRetentionCandidate {
    push_id: String,
    retention: String,
    delivery_state: String,
    created_at: String,
    settled_at: Option<String>,
}

// A terminal push goes once no unanswered send, unsettled cancel, scheduled retry or poll
// hand-off still names it.
macro_rules! settled_push_eligible {
    () => {
        "router_pushes.delivery_state NOT IN ('pending','held','attempted') AND NOT EXISTS (SELECT 1 FROM message_send_answers a WHERE a.item_id=router_pushes.push_id AND a.result IS NULL) AND NOT EXISTS (SELECT 1 FROM local_cancel_deliveries c WHERE (c.item_id=router_pushes.push_id OR c.target_item=router_pushes.push_id) AND c.outcome_json IS NULL) AND NOT EXISTS (SELECT 1 FROM local_delivery_retries r WHERE r.item_id=router_pushes.push_id AND r.scheduled_retry_at IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM pending_poll_mail poll WHERE poll.item_id=router_pushes.push_id)"
    };
}

/// The first moment the rule allows deleting any interaction record it holds now: settled
/// answers, approval routes, settled cancels, read contexts and terminal pushes. A record
/// still protected is not counted; removing its protection commits and wakes the worker.
pub(super) async fn prune_due(
    store: &InteractionStore,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, StorageError> {
    let mut connection = store.writer.lock().await;
    let stamp_ms = |millis: Option<i64>| {
        millis
            .map(|millis| {
                DateTime::<Utc>::from_timestamp_millis(millis).ok_or(StorageError::InvalidRecord)
            })
            .transpose()
    };
    let stamp_text =
        |text: Option<String>| text.map(push_record_rows::decode_timestamp).transpose();
    let oldest = [
        stamp_ms(
            crate::message_send_answers::oldest_settled_message_send_answer_ms(&mut connection)
                .await?,
        )?,
        stamp_ms(crate::approval_routes::oldest_approval_route_use_ms(&mut connection).await?)?,
        stamp_ms(
            crate::local_cancel_deliveries::oldest_settled_local_cancel_ms(&mut connection)
                .await?,
        )?,
        stamp_text(crate::mail_read_contexts::oldest_mail_read_context(&mut connection).await?)?,
        stamp_text(
            sqlx::query_scalar::<sqlx_turso::Turso, Option<String>>(concat!(
                "SELECT MIN(CASE WHEN retention='mailTerminalAge' THEN settled_at ELSE created_at END) FROM router_pushes WHERE ((retention='creationAge') OR (retention='mailTerminalAge' AND settled_at IS NOT NULL)) AND ",
                settled_push_eligible!()
            ))
            .fetch_one(&mut *connection)
            .await?,
        )?,
    ]
    .into_iter()
    .flatten()
    .min();
    Ok(oldest.and_then(|stamped| {
        sqlite_storage_maintenance::first_deletable_at(
            stamped,
            crate::INTERACTION_RETENTION_MONTHS,
            now,
        )
    }))
}

fn is_terminal(state: PushDeliveryState) -> bool {
    matches!(
        state,
        PushDeliveryState::Delivered
            | PushDeliveryState::OutcomeUnknown
            | PushDeliveryState::Rejected
    )
}

pub(super) async fn prune(
    store: &InteractionStore,
    now: DateTime<Utc>,
    batch_size: u32,
) -> Result<u64, StorageError> {
    if !(1..=MAX_PUSH_PRUNE_BATCH).contains(&batch_size) {
        return Err(StorageError::InvalidRecord);
    }
    let cutoff =
        sqlite_storage_maintenance::months_before(now, crate::INTERACTION_RETENTION_MONTHS)
            .ok_or(StorageError::InvalidRecord)?;
    let cutoff_text = push_record_rows::serialize_timestamp(cutoff);
    let cutoff_ms = cutoff.timestamp_millis();
    let batch_size = i64::from(batch_size);
    let mut connection = store.writer.lock().await;
    let mut transaction = store
        .transaction(
            &mut connection,
            sqlite_storage_maintenance::TransactionStart::Immediate,
        )
        .await?;
    let pruned = async {
        let mut pruned = crate::message_send_answers::prune_settled_message_send_answers(
            &mut transaction,
            cutoff_ms,
            batch_size,
        )
        .await?;
        pruned += crate::approval_routes::prune_expired(
            &mut transaction,
            cutoff_ms,
            batch_size,
        )
        .await?;
        pruned += crate::local_cancel_deliveries::prune_settled_local_cancels(
            &mut transaction,
            cutoff_ms,
            batch_size,
        )
        .await?;
        pruned += crate::mail_read_contexts::prune_old_mail_read_contexts(
            &mut transaction,
            &cutoff_text,
            batch_size,
        )
        .await?;

        let candidates = sqlx::query_as::<sqlx_turso::Turso, PushRetentionCandidate>(
            concat!(
                "SELECT push_id,retention,delivery_state,created_at,settled_at FROM router_pushes WHERE (((retention='creationAge' AND created_at<?) OR (retention='mailTerminalAge' AND settled_at<?)) AND ",
                settled_push_eligible!(),
                ") OR retention NOT IN ('creationAge','mailTerminalAge') OR (retention='mailTerminalAge' AND ((delivery_state IN ('pending','held','attempted') AND settled_at IS NOT NULL) OR (delivery_state IN ('delivered','outcome_unknown','rejected') AND settled_at IS NULL))) ORDER BY CASE WHEN retention='mailTerminalAge' THEN settled_at ELSE created_at END,push_id LIMIT ?"
            ),
        )
            .bind(&cutoff_text)
            .bind(&cutoff_text)
            .bind(batch_size)
            .fetch_all(&mut *transaction).await?;
        for candidate in candidates {
            let retention: crate::push_retention::PushRetention =
                serde_json::from_value(Value::String(candidate.retention))
                    .map_err(|_| StorageError::InvalidRecord)?;
            let state = push_record_rows::decode_delivery_state(&candidate.delivery_state)?;
            retention.validate_clock(state, candidate.settled_at.as_deref())?;
            if !is_terminal(state) {
                continue;
            }
            let age_from = match retention {
                crate::push_retention::PushRetention::CreationAge => candidate.created_at,
                crate::push_retention::PushRetention::MailTerminalAge => {
                    candidate.settled_at.ok_or(StorageError::InvalidRecord)?
                }
            };
            push_record_rows::decode_timestamp(age_from)?;
            let id: PushId = candidate
                .push_id
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?;
            let deleted = sqlx::query::<sqlx_turso::Turso>(concat!(
                "DELETE FROM router_pushes WHERE push_id=? AND ",
                settled_push_eligible!()
            ))
            .bind(id.as_str())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if deleted > 0 {
                pruned += deleted;
                pruned += crate::mail_submission_epochs::delete_submission_epoch_for_pruned_push(
                    &mut transaction,
                    id.as_str(),
                )
                .await?;
            }
        }
        Ok::<_, StorageError>(pruned)
    }
    .await;
    let pruned = match pruned {
        Ok(pruned) => pruned,
        Err(error) => {
            transaction.rollback().await?;
            return Err(error);
        }
    };
    transaction.commit().await?;
    Ok(pruned)
}
