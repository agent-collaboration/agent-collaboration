//! Immutable read authority accompanies mail; the SDK alone owns its status.
use crate::{InteractionStore, StorageError, push_record_rows};
use chrono::{DateTime, Utc};
use collaboration_protocol::{MailItemId, MailReadContext, PushOrigin, SessionRef};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use sqlx::Transaction;
use sqlx_turso::{Turso, TursoConnection};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredMailReadContext {
    pub item: MailItemId,
    pub context: MailReadContext,
    pub recorded_at: DateTime<Utc>,
}
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
    fn decode(self) -> Result<StoredMailReadContext, StorageError> {
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
        Ok(StoredMailReadContext {
            item,
            context,
            recorded_at: push_record_rows::decode_timestamp(self.recorded_at)
                .map_err(|_| StorageError::InvalidStoredRecord)?,
        })
    }
}
async fn read_context(
    connection: &mut TursoConnection,
    item: &MailItemId,
) -> Result<Option<StoredMailReadContext>, StorageError> {
    sqlx_turso::query_as!(ReadContextRow, "SELECT item_id AS \"item_id!: String\",context_json AS \"context_json!: String\",conversation_key,recorded_at AS \"recorded_at!: String\" FROM mail_read_contexts WHERE item_id=?", item.as_str()).fetch_optional(connection).await?.map(ReadContextRow::decode).transpose()
}

pub(crate) async fn prune_old_mail_read_contexts(
    transaction: &mut Transaction<'_, Turso>,
    cutoff: &str,
    batch_size: i64,
) -> Result<u64, StorageError> {
    let candidates = sqlx::query_as::<sqlx_turso::Turso, ReadContextRow>(
        "SELECT item_id,context_json,conversation_key,recorded_at FROM mail_read_contexts WHERE recorded_at<? AND NOT EXISTS (SELECT 1 FROM router_pushes p WHERE p.push_id=mail_read_contexts.item_id AND p.delivery_state IN ('pending','held','attempted')) AND NOT EXISTS (SELECT 1 FROM local_delivery_retries r WHERE r.item_id=mail_read_contexts.item_id AND r.scheduled_retry_at IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM pending_poll_mail poll WHERE poll.item_id=mail_read_contexts.item_id) AND NOT EXISTS (SELECT 1 FROM message_send_answers answer WHERE answer.item_id=mail_read_contexts.item_id AND answer.result IS NULL) AND NOT EXISTS (SELECT 1 FROM local_cancel_deliveries c WHERE (c.item_id=mail_read_contexts.item_id OR c.target_item=mail_read_contexts.item_id) AND c.outcome_json IS NULL) ORDER BY recorded_at,item_id LIMIT ?",
    )
        .bind(cutoff)
        .bind(batch_size)
        .fetch_all(&mut **transaction).await?;
    let mut pruned = 0;
    for row in candidates {
        let context = row.decode()?;
        pruned += sqlx::query::<sqlx_turso::Turso>(
            "DELETE FROM mail_read_contexts WHERE item_id=? AND recorded_at<?",
        )
        .bind(context.item.as_str())
        .bind(cutoff)
        .execute(&mut **transaction)
        .await?
        .rows_affected();
    }
    Ok(pruned)
}
impl InteractionStore {
    pub async fn mail_read_context(
        &self,
        item: &MailItemId,
    ) -> Result<Option<StoredMailReadContext>, StorageError> {
        read_context(&mut *self.writer.lock().await, item).await
    }
    pub async fn direct_mail_read_contexts(
        &self,
        caller: &SessionRef,
        other: &SessionRef,
        limit: u32,
    ) -> Result<Vec<StoredMailReadContext>, StorageError> {
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidRecord);
        }
        let key = conversation_key(&MailReadContext::Direct {
            origin: PushOrigin::Session(caller.clone()),
            recipient: other.clone(),
        })?
        .ok_or(StorageError::InvalidRecord)?;
        let limit = i64::from(limit);
        let rows = sqlx_turso::query_as!(ReadContextRow, "SELECT item_id AS \"item_id!: String\",context_json AS \"context_json!: String\",conversation_key,recorded_at AS \"recorded_at!: String\" FROM mail_read_contexts INDEXED BY mail_read_context_conversation WHERE conversation_key=? ORDER BY recorded_at DESC,item_id DESC LIMIT ?", key, limit).fetch_all(&mut *self.writer.lock().await).await?;
        rows.into_iter().map(ReadContextRow::decode).collect()
    }
}
pub async fn record_mail_read_context_in_scope<THost: Host>(
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
    if let Some(existing) = read_context(scope.executor(), item).await? {
        return if existing.context == *context {
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
