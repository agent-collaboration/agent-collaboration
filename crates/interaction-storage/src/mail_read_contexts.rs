//! Immutable read authority accompanies mail; the SDK alone owns its status.
use crate::{StorageError, push_record_rows};
#[cfg(test)]
use chrono::{DateTime, Utc};
use collaboration_protocol::{MailItemId, MailReadContext, PushOrigin};
#[cfg(test)]
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
#[cfg(test)]
use sqlx_turso::TursoConnection;

#[derive(sqlx::FromRow)]
struct ReadContextRow {
    item_id: String,
    context_json: String,
    conversation_key: Option<String>,
    recorded_at: String,
}

fn conversation_key(context: &MailReadContext) -> Result<Option<String>, StorageError> {
    match context {
        MailReadContext::Direct {
            origin: PushOrigin::Session(sender),
            recipient,
        } => {
            let mut pair = [
                serde_json::to_string(sender).map_err(|_| StorageError::InvalidRecord)?,
                serde_json::to_string(recipient).map_err(|_| StorageError::InvalidRecord)?,
            ];
            pair.sort();
            serde_json::to_string(&pair)
                .map(Some)
                .map_err(|_| StorageError::InvalidRecord)
        }
        MailReadContext::Direct {
            origin: PushOrigin::OwnerUnverified,
            ..
        }
        | MailReadContext::Project { .. } => Ok(None),
        MailReadContext::Direct {
            origin: PushOrigin::Router(_),
            ..
        } => Err(StorageError::InvalidRecord),
    }
}
impl ReadContextRow {
    fn decode(self) -> Result<(MailItemId, MailReadContext), StorageError> {
        let item: MailItemId = self
            .item_id
            .try_into()
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        let context: MailReadContext = serde_json::from_str(&self.context_json)
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        if conversation_key(&context).map_err(|_| StorageError::InvalidStoredRecord)?
            != self.conversation_key
            || matches!(&context, MailReadContext::Project { handoff, .. } if handoff.as_str() != item.as_str())
        {
            return Err(StorageError::InvalidStoredRecord);
        }
        push_record_rows::decode_timestamp(self.recorded_at)
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        Ok((item, context))
    }
}
#[cfg(test)]
async fn read_context(
    connection: &mut TursoConnection,
    item: &MailItemId,
) -> Result<Option<(MailItemId, MailReadContext)>, StorageError> {
    sqlx_turso::query_as!(ReadContextRow, "SELECT item_id AS \"item_id!: String\",context_json AS \"context_json!: String\",conversation_key,recorded_at AS \"recorded_at!: String\" FROM mail_read_contexts WHERE item_id=?", item.as_str()).fetch_optional(connection).await?.map(ReadContextRow::decode).transpose()
}

// A read context goes once its item has no live push, scheduled retry, poll marker,
// unanswered send or unsettled cancel.
macro_rules! read_contexts_eligible {
    () => {
        "NOT EXISTS (SELECT 1 FROM router_pushes p WHERE p.push_id=mail_read_contexts.item_id AND p.delivery_state IN ('pending','held','attempted')) AND NOT EXISTS (SELECT 1 FROM local_delivery_retries r WHERE r.item_id=mail_read_contexts.item_id AND r.scheduled_retry_at IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM pending_poll_mail poll WHERE poll.item_id=mail_read_contexts.item_id) AND NOT EXISTS (SELECT 1 FROM message_send_answers answer WHERE answer.item_id=mail_read_contexts.item_id AND answer.result IS NULL) AND NOT EXISTS (SELECT 1 FROM local_cancel_deliveries c WHERE (c.item_id=mail_read_contexts.item_id OR c.target_item=mail_read_contexts.item_id) AND c.outcome_json IS NULL)"
    };
}

/// The record time of the oldest read context the rule may ever delete.
pub(crate) async fn oldest_mail_read_context(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<Option<String>, StorageError> {
    Ok(
        sqlx::query_scalar::<sqlx_turso::Turso, Option<String>>(concat!(
            "SELECT MIN(recorded_at) FROM mail_read_contexts WHERE ",
            read_contexts_eligible!()
        ))
        .fetch_one(connection)
        .await?,
    )
}

pub(crate) async fn prune_old_mail_read_contexts(
    transaction: &mut sqlite_storage_maintenance::SignaledTransaction<'_, sqlx_turso::Turso>,
    cutoff: &str,
    batch_size: i64,
) -> Result<u64, StorageError> {
    let candidates = sqlx::query_as::<sqlx_turso::Turso, ReadContextRow>(
        concat!(
            "SELECT item_id,context_json,conversation_key,recorded_at FROM mail_read_contexts WHERE recorded_at<? AND ",
            read_contexts_eligible!(),
            " ORDER BY recorded_at,item_id LIMIT ?"
        ),
    )
        .bind(cutoff)
        .bind(batch_size)
        .fetch_all(&mut **transaction).await?;
    let mut pruned = 0;
    for row in candidates {
        let (item, _) = row.decode()?;
        pruned += sqlx::query::<sqlx_turso::Turso>(
            "DELETE FROM mail_read_contexts WHERE item_id=? AND recorded_at<?",
        )
        .bind(item.as_str())
        .bind(cutoff)
        .execute(&mut **transaction)
        .await?
        .rows_affected();
    }
    Ok(pruned)
}
#[cfg(test)]
pub(crate) async fn record_mail_read_context_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
    context: &MailReadContext,
    now: DateTime<Utc>,
) -> Result<(), StorageError> {
    let key = conversation_key(context)?;
    if matches!(context, MailReadContext::Project { handoff, .. } if handoff.as_str()!=item.as_str())
    {
        return Err(StorageError::InvalidRecord);
    }
    if let Some((_, existing)) = read_context(scope.executor(), item).await? {
        return if existing == *context {
            Ok(())
        } else {
            Err(StorageError::InvalidStoredRecord)
        };
    }
    let content = serde_json::to_string(context).map_err(|_| StorageError::InvalidRecord)?;
    let now = push_record_rows::serialize_timestamp(now);
    sqlx_turso::query!("INSERT INTO mail_read_contexts(item_id,context_json,conversation_key,recorded_at) VALUES (?,?,?,?)", item.as_str(), content, key, now).execute(scope.executor()).await?;
    Ok(())
}
