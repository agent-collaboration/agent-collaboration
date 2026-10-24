//! Recover only creation-age carrier work; SDK mail recovery belongs to its runtime.
use crate::{InteractionStore, StorageError, push_record_rows};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, EndpointId, EndpointRef, PushId, PushKind, PushRecord,
    SessionId, SessionRef, UuidIdentity,
};
use sqlx::{Connection, FromRow};

#[derive(FromRow)]
struct DirectMessageTargetRow {
    target_service_id: String,
    target_endpoint_id: String,
    target_session_id: String,
}

impl InteractionStore {
    /// Finds reader owners that must resume known-unsent direct messages.
    pub async fn direct_message_recovery_targets(&self) -> Result<Vec<SessionRef>, StorageError> {
        let mut connection = self.writer.lock().await;
        let kind = push_record_rows::serialize_kind(PushKind::DirectMessage)?;
        let board = push_record_rows::serialize_kind(PushKind::BoardNotification)?;
        let rows = sqlx_turso::query_as!(
            DirectMessageTargetRow,
            "SELECT DISTINCT target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\" FROM router_pushes WHERE retention='creationAge' AND kind IN (?,?) AND delivery_state IN ('pending','held') ORDER BY target_service_id,target_endpoint_id,target_session_id",
            kind, board
        )
        .fetch_all(&mut *connection)
        .await?;
        let mut targets = Vec::with_capacity(rows.len());
        for row in rows {
            match direct_message_recovery_target(row) {
                Ok(target) => targets.push(target),
                Err(field) => {
                    tracing::warn!(
                        record_field = field,
                        "direct-message recovery target has invalid stored field"
                    );
                }
            }
        }
        Ok(targets)
    }

    pub async fn list_unsent_direct_messages(
        &self,
        target: &SessionRef,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        let kind = push_record_rows::serialize_kind(PushKind::DirectMessage)?;
        let board = push_record_rows::serialize_kind(PushKind::BoardNotification)?;
        let (service_id, endpoint_id, session_id) = (
            String::from(target.endpoint.service_id.clone()),
            String::from(target.endpoint.endpoint_id.clone()),
            String::from(target.session_id.clone()),
        );
        let rows = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE retention='creationAge' AND kind IN (?,?) AND target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND delivery_state IN ('pending','held') ORDER BY created_at,push_id",
            kind, board, service_id, endpoint_id, session_id
        ).fetch_all(&mut *connection).await?;
        rows.into_iter()
            .map(push_record_rows::PushRecordRow::into_record)
            .collect()
    }

    /// The process died after an attempt began. Its effect is unknowable; never resend it.
    pub async fn settle_interrupted_pushes(&self, now: DateTime<Utc>) -> Result<u64, StorageError> {
        let receipt = DeliveryReceipt {
            outcome: DeliveryOutcome::Unknown,
            reachability: None,
            client: None,
        };
        let outcome_json = push_record_rows::serialize_json(&receipt)?;
        let settled_at = push_record_rows::serialize_timestamp(now);
        let direct_message = push_record_rows::serialize_kind(PushKind::DirectMessage)?;
        let subscription_activity =
            push_record_rows::serialize_kind(PushKind::SubscriptionActivity)?;
        let subscription_expiry = push_record_rows::serialize_kind(PushKind::SubscriptionExpiry)?;
        let approval = push_record_rows::serialize_kind(PushKind::Approval)?;
        let question = push_record_rows::serialize_kind(PushKind::Question)?;
        let board_notification = push_record_rows::serialize_kind(PushKind::BoardNotification)?;
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let mut settled = 0;
        loop {
            let ids = sqlx_turso::query!("SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE retention='creationAge' AND delivery_state='attempted' AND last_outcome_json IS NULL AND kind IN (?, ?, ?, ?, ?, ?) ORDER BY push_id LIMIT 128", direct_message,subscription_activity,subscription_expiry,approval,question,board_notification).fetch_all(&mut *transaction).await?;
            if ids.is_empty() {
                break;
            }
            for row in ids {
                let id: PushId = row
                    .push_id
                    .try_into()
                    .map_err(|_| StorageError::InvalidRecord)?;
                let _ =
                    crate::push_records::read_push_with_connection(&mut transaction, &id).await?;
                sqlx_turso::query!("UPDATE router_pushes SET delivery_state='outcome_unknown',last_outcome_json=?,settled_at=? WHERE retention='creationAge' AND push_id=? AND delivery_state='attempted' AND last_outcome_json IS NULL",outcome_json,settled_at,id.as_str()).execute(&mut *transaction).await?;
                settled += 1;
            }
        }
        transaction.commit().await?;
        Ok(settled)
    }
}

fn direct_message_recovery_target(row: DirectMessageTargetRow) -> Result<SessionRef, &'static str> {
    let service_id =
        UuidIdentity::try_from(row.target_service_id).map_err(|_| "target_service_id")?;
    let endpoint_id =
        EndpointId::try_from(row.target_endpoint_id).map_err(|_| "target_endpoint_id")?;
    let session_id = SessionId::try_from(row.target_session_id).map_err(|_| "target_session_id")?;
    Ok(SessionRef {
        endpoint: EndpointRef {
            service_id,
            endpoint_id,
        },
        session_id,
    })
}
