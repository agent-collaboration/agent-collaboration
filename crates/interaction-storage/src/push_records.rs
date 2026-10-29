//! Durable push snapshots, delivery transitions, DM queries, and bounded pruning.
use crate::{InteractionStore, StorageError, push_record_rows};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushId, PushKind, PushOrigin, PushRecord, PushRecordDraft,
    RouterOriginRef, SessionRef,
};
use sqlx::Connection;
use sqlx_turso::TursoConnection;
use std::time::Duration;

const PUSH_INSERT_RETRY_WINDOW: Duration = Duration::from_secs(5);
const PUSH_CONNECTION_BUSY_TIMEOUT: Duration = Duration::from_secs(1);
const PUSH_INSERT_INITIAL_BACKOFF: Duration = Duration::from_millis(25);
const PUSH_INSERT_MAX_BACKOFF: Duration = Duration::from_millis(250);
const SQLITE_BUSY_PRIMARY_CODE: i32 = 5;
const SQLITE_PRIMARY_CODE_MASK: i32 = 0xff;
pub const MAX_PUSH_PRUNE_BATCH: u32 = 500;
pub const MAX_PUSH_LIST_LIMIT: u32 = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PushInboxQuery {
    pub target: SessionRef,
    pub limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectMessageHistoryQuery {
    pub caller: SessionRef,
    pub with: SessionRef,
    pub limit: u32,
}

impl InteractionStore {
    /// Persists a validated, pending push before its delivery is attempted.
    pub async fn insert_push_record(
        &self,
        draft: PushRecordDraft,
    ) -> Result<PushRecord, StorageError> {
        let record = draft
            .into_pending()
            .map_err(|_| StorageError::InvalidRecord)?;
        let deadline = tokio::time::Instant::now() + PUSH_INSERT_RETRY_WINDOW;
        let mut backoff = PUSH_INSERT_INITIAL_BACKOFF;

        loop {
            match self.insert_push_record_once(&record).await {
                Ok(()) => return Ok(record),
                Err(error) if is_database_busy(&error) => {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if remaining <= PUSH_CONNECTION_BUSY_TIMEOUT {
                        return Err(error);
                    }
                    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
                    tracing::debug!(
                        target: "interaction_storage::runtime_test_push_insert_busy",
                        push_id = record.push_id.as_str(),
                        "push-insert-native-busy-before-retry-backoff"
                    );
                    tokio::time::sleep(backoff.min(remaining)).await;
                    backoff = (backoff * 2).min(PUSH_INSERT_MAX_BACKOFF);
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn insert_push_record_once(&self, record: &PushRecord) -> Result<(), StorageError> {
        let mut connection = self.writer.lock().await;
        insert_push_record_with_connection(&mut connection, record).await
    }

    pub async fn get_push_record(
        &self,
        push_id: &PushId,
    ) -> Result<Option<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        read_push_with_connection(&mut connection, push_id).await
    }

    pub async fn get_push_record_by_router_ref(
        &self,
        origin_router_ref: &str,
    ) -> Result<Option<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        let rows = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE origin_kind='router' AND origin_router_ref=? ORDER BY created_at,push_id",
            origin_router_ref
        )
        .fetch_all(&mut *connection)
        .await?;
        if rows.len() > 1 {
            return Err(StorageError::InvalidRecord);
        }
        rows.into_iter()
            .next()
            .map(push_record_rows::PushRecordRow::into_record)
            .transpose()
    }

    pub async fn get_push_record_by_origin_reference(
        &self,
        origin: &RouterOriginRef,
    ) -> Result<Option<PushRecord>, StorageError> {
        let origin_router_ref = origin
            .canonical_string()
            .map_err(|_| StorageError::InvalidRecord)?;
        let record = self
            .get_push_record_by_router_ref(&origin_router_ref)
            .await?;
        if record.as_ref().is_some_and(|record| {
            record.origin_router_ref.as_deref() != Some(origin_router_ref.as_str())
                || !origin.supports_kind(record.kind)
        }) {
            return Err(StorageError::InvalidRecord);
        }
        Ok(record)
    }

    pub async fn list_pending_push_records(
        &self,
        target: &SessionRef,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        let (service_id, endpoint_id, session_id) = session_key(target);
        let rows = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND delivery_state='pending' ORDER BY created_at,push_id",
            service_id,
            endpoint_id,
            session_id
        )
        .fetch_all(&mut *connection)
        .await?;
        rows.into_iter()
            .map(push_record_rows::PushRecordRow::into_record)
            .collect()
    }

    pub async fn list_held_push_records(
        &self,
        target: &SessionRef,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        let (service_id, endpoint_id, session_id) = session_key(target);
        let rows = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND delivery_state='held' ORDER BY created_at,push_id",
            service_id,
            endpoint_id,
            session_id
        )
        .fetch_all(&mut *connection)
        .await?;
        rows.into_iter()
            .map(push_record_rows::PushRecordRow::into_record)
            .collect()
    }

    pub async fn list_held_carrier_push_records(
        &self,
        target: &SessionRef,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        let (service_id, endpoint_id, session_id) = session_key(target);
        let rows = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND retention='creationAge' AND delivery_state='held' ORDER BY created_at,push_id",
            service_id,
            endpoint_id,
            session_id
        )
        .fetch_all(&mut *connection)
        .await?;
        rows.into_iter()
            .map(push_record_rows::PushRecordRow::into_record)
            .collect()
    }

    /// Terminalize one selected held Reader batch, bounded by its replacement age.
    pub async fn settle_held_subscription_push(
        &self,
        target: &SessionRef,
        push_id: &PushId,
        created_before: Option<DateTime<Utc>>,
        reason: &str,
        settled_at: DateTime<Utc>,
    ) -> Result<u64, StorageError> {
        let mut connection = self.writer.lock().await;
        let kind = push_record_rows::serialize_kind(PushKind::SubscriptionActivity)?;
        let outcome = DeliveryReceipt {
            outcome: DeliveryOutcome::NotSubmitted {
                retryable: false,
                reason: reason.to_owned(),
            },
            reachability: None,
            client: None,
        };
        let outcome_json = push_record_rows::serialize_json(&outcome)?;
        let settled_at = push_record_rows::serialize_timestamp(settled_at);
        let (service_id, endpoint_id, session_id) = session_key(target);
        let created_before = created_before.map(push_record_rows::serialize_timestamp);
        let result = sqlx_turso::query!(
            "UPDATE router_pushes SET delivery_state='rejected',last_outcome_json=?,settled_at=? \
             WHERE kind=? AND target_service_id=? AND target_endpoint_id=? AND target_session_id=? \
               AND retention='creationAge' AND delivery_state='held' AND push_id=? AND (? IS NULL OR created_at<?)",
            outcome_json,
            settled_at,
            kind,
            service_id,
            endpoint_id,
            session_id,
            push_id.as_str(),
            created_before,
            created_before,
        )
        .execute(&mut *connection)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn list_direct_message_inbox(
        &self,
        request: &PushInboxQuery,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        validate_limit(request.limit)?;
        let kind = push_record_rows::serialize_kind(PushKind::DirectMessage)?;
        let (service_id, endpoint_id, session_id) = session_key(&request.target);
        let rows = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE kind=? AND target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND read_at IS NULL ORDER BY created_at DESC,push_id DESC LIMIT ?",
            kind,
            service_id,
            endpoint_id,
            session_id,
            i64::from(request.limit)
        )
        .fetch_all(&mut *connection)
        .await?;
        rows.into_iter()
            .map(push_record_rows::PushRecordRow::into_record)
            .collect()
    }

    pub async fn list_direct_message_history(
        &self,
        request: &DirectMessageHistoryQuery,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        validate_limit(request.limit)?;
        let kind = push_record_rows::serialize_kind(PushKind::DirectMessage)?;
        let (caller_service_id, caller_endpoint_id, caller_session_id) =
            session_key(&request.caller);
        let (with_service_id, with_endpoint_id, with_session_id) = session_key(&request.with);
        let rows = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE kind=? AND ((target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND origin_service_id=? AND origin_endpoint_id=? AND origin_session_id=?) OR (target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND origin_service_id=? AND origin_endpoint_id=? AND origin_session_id=?)) ORDER BY created_at DESC,push_id DESC LIMIT ?",
            kind,
            caller_service_id,
            caller_endpoint_id,
            caller_session_id,
            with_service_id,
            with_endpoint_id,
            with_session_id,
            with_service_id,
            with_endpoint_id,
            with_session_id,
            caller_service_id,
            caller_endpoint_id,
            caller_session_id,
            i64::from(request.limit)
        )
        .fetch_all(&mut *connection)
        .await?;
        rows.into_iter()
            .map(push_record_rows::PushRecordRow::into_record)
            .collect()
    }

    pub async fn mark_push_read(
        &self,
        push_id: &PushId,
        caller: &SessionRef,
        read_at: DateTime<Utc>,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let record = read_push_with_connection(&mut connection, push_id)
            .await?
            .ok_or(StorageError::PushNotFound)?;
        if record.kind != PushKind::DirectMessage {
            if &record.target != caller {
                return Err(StorageError::PushNotPermitted);
            }
            return Ok(record);
        }
        if &record.target != caller {
            if record.origin == PushOrigin::Session(caller.clone()) {
                return Ok(record);
            }
            return Err(StorageError::PushNotPermitted);
        }
        let read_at = push_record_rows::serialize_timestamp(read_at);
        sqlx_turso::query!(
            "UPDATE router_pushes SET read_at=COALESCE(read_at,?) WHERE push_id=?",
            read_at,
            push_id.as_str()
        )
        .execute(&mut *connection)
        .await?;
        read_push_with_connection(&mut connection, push_id)
            .await?
            .ok_or(StorageError::PushNotFound)
    }

    pub async fn prune_push_records(
        &self,
        now: DateTime<Utc>,
        batch_size: u32,
    ) -> Result<u64, StorageError> {
        let mut connection = self.writer.lock().await;
        if !(1..=MAX_PUSH_PRUNE_BATCH).contains(&batch_size) {
            return Err(StorageError::InvalidRecord);
        }
        let cutoff = now
            .checked_sub_signed(ChronoDuration::days(30))
            .ok_or(StorageError::InvalidRecord)?;
        let cutoff = push_record_rows::serialize_timestamp(cutoff);
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let pruned = async {
            let candidates = sqlx_turso::query!(
                "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",delivery_state AS \"delivery_state!: String\",created_at AS \"created_at!: String\",settled_at FROM router_pushes WHERE (retention='creationAge' AND created_at<?) OR (retention='mailTerminalAge' AND settled_at<?) OR retention NOT IN ('creationAge','mailTerminalAge') ORDER BY CASE WHEN retention='mailTerminalAge' THEN settled_at ELSE created_at END,push_id LIMIT ?",
                cutoff, cutoff, i64::from(batch_size)
            ).fetch_all(&mut *transaction).await?;
            let mut pruned = 0;
            for candidate in candidates {
                let retention: crate::push_retention::PushRetention =
                    serde_json::from_value(serde_json::Value::String(candidate.retention))
                        .map_err(|_| StorageError::InvalidRecord)?;
                let state = push_record_rows::decode_delivery_state(&candidate.delivery_state)?;
                retention.validate_clock(state, candidate.settled_at.as_deref())?;
                let age_from = match retention {
                    crate::push_retention::PushRetention::CreationAge => candidate.created_at,
                    crate::push_retention::PushRetention::MailTerminalAge => {
                        candidate.settled_at.ok_or(StorageError::InvalidRecord)?
                    }
                };
                // Validate the stored clock before the destructive operation.
                crate::push_record_rows::decode_timestamp(age_from)?;
                let id: PushId = candidate
                    .push_id
                    .try_into()
                    .map_err(|_| StorageError::InvalidRecord)?;
                pruned += sqlx_turso::query!("DELETE FROM router_pushes WHERE push_id=?", id.as_str())
                    .execute(&mut *transaction)
                    .await?
                    .rows_affected();
            }
            Ok::<_, StorageError>(pruned)
        }.await;
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
}

pub(crate) async fn insert_push_record_with_connection(
    connection: &mut TursoConnection,
    record: &PushRecord,
) -> Result<(), StorageError> {
    insert_push_record_with_retention(
        connection,
        record,
        crate::push_retention::PushRetention::CreationAge,
    )
    .await
}

pub(crate) async fn insert_push_record_with_retention(
    connection: &mut TursoConnection,
    record: &PushRecord,
    retention: crate::push_retention::PushRetention,
) -> Result<(), StorageError> {
    let retention = push_record_rows::serialize_json(&retention)?;
    let retention: String =
        serde_json::from_str(&retention).map_err(|_| StorageError::InvalidRecord)?;
    push_record_rows::validate_router_origin_reference(
        record.kind,
        &record.origin,
        record.origin_router_ref.as_deref(),
    )?;
    let kind = push_record_rows::serialize_kind(record.kind)?;
    let (origin_kind, origin_session) = push_record_rows::serialize_origin(&record.origin);
    let origin_service_id =
        origin_session.map(|session| String::from(session.endpoint.service_id.clone()));
    let origin_endpoint_id =
        origin_session.map(|session| String::from(session.endpoint.endpoint_id.clone()));
    let origin_session_id = origin_session.map(|session| String::from(session.session_id.clone()));
    let target_service_id = String::from(record.target.endpoint.service_id.clone());
    let target_endpoint_id = String::from(record.target.endpoint.endpoint_id.clone());
    let target_session_id = String::from(record.target.session_id.clone());
    let reply_to_push_id = record
        .reply_to_push_id
        .as_ref()
        .map(|push_id| push_id.as_str());
    let header_facts_json = push_record_rows::serialize_json(&record.header_facts)?;
    let ranges_json = record
        .activity
        .as_ref()
        .map(push_record_rows::serialize_json)
        .transpose()?;
    let created_at = push_record_rows::serialize_timestamp(record.created_at);
    let dm_delivery_mode = push_record_rows::serialize_direct_message_mode(record.mode);
    let dm_generation_guard_json = record
        .guard
        .as_ref()
        .map(push_record_rows::serialize_json)
        .transpose()?;

    let result = sqlx_turso::query!(
        "INSERT INTO router_pushes (push_id,retention,kind,origin_kind,origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id,target_endpoint_id,target_session_id,reply_to_push_id,header_facts_json,body,ranges_json,delivery_state,last_outcome_json,cancelled_after_delivery_json,created_at,settled_at,read_at,dm_delivery_mode,dm_generation_guard_json) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'pending',NULL,NULL,?,NULL,NULL,?,?) ON CONFLICT(push_id) DO NOTHING",
        record.push_id.as_str(),
        retention,
        kind,
        origin_kind,
        origin_service_id,
        origin_endpoint_id,
        origin_session_id,
        &record.origin_router_ref,
        target_service_id,
        target_endpoint_id,
        target_session_id,
        reply_to_push_id,
        header_facts_json,
        record.body.as_deref(),
        ranges_json,
        created_at,
        dm_delivery_mode,
        dm_generation_guard_json
    )
    .execute(connection)
    .await?;
    if result.rows_affected() == 0 {
        return Err(StorageError::PushAlreadyExists);
    }
    Ok(())
}

fn session_key(session: &SessionRef) -> (String, String, String) {
    (
        String::from(session.endpoint.service_id.clone()),
        String::from(session.endpoint.endpoint_id.clone()),
        String::from(session.session_id.clone()),
    )
}

fn validate_limit(limit: u32) -> Result<(), StorageError> {
    if !(1..=MAX_PUSH_LIST_LIMIT).contains(&limit) {
        Err(StorageError::InvalidRecord)
    } else {
        Ok(())
    }
}

fn is_database_busy(error: &StorageError) -> bool {
    let StorageError::Database(sqlx_error) = error else {
        return false;
    };
    sqlx_error
        .as_database_error()
        .and_then(|database_error| database_error.code())
        .is_some_and(|code| {
            code == "SQLITE_BUSY"
                || code.starts_with("SQLITE_BUSY_")
                || code
                    .parse::<i32>()
                    .is_ok_and(|code| code & SQLITE_PRIMARY_CODE_MASK == SQLITE_BUSY_PRIMARY_CODE)
        })
}

pub(crate) async fn read_push_with_connection(
    connection: &mut TursoConnection,
    push_id: &PushId,
) -> Result<Option<PushRecord>, StorageError> {
    let row = sqlx_turso::query_as!(
            push_record_rows::PushRecordRow,
            "SELECT push_id AS \"push_id!: String\",retention AS \"retention!: String\",kind AS \"kind!: String\",origin_kind AS \"origin_kind!: String\",origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id AS \"target_service_id!: String\",target_endpoint_id AS \"target_endpoint_id!: String\",target_session_id AS \"target_session_id!: String\",reply_to_push_id,header_facts_json AS \"header_facts_json!: String\",body,ranges_json,delivery_state AS \"delivery_state!: String\",last_outcome_json,cancelled_after_delivery_json,created_at AS \"created_at!: String\",settled_at,read_at,dm_delivery_mode,dm_generation_guard_json FROM router_pushes WHERE push_id=?",
            push_id.as_str()
        )
        .fetch_optional(connection)
        .await?;
    row.map(push_record_rows::PushRecordRow::into_record)
        .transpose()
}
