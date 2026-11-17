//! The Mac outbox in provider-operations.sqlite (owner 2026-11-15). Every
//! time comes from the caller's injected clock; stored values are decoded
//! through Rust types and an invalid row is refused, never coerced.
use crate::ProviderOperationStore;
use crate::provider_operation_store::OneStatementWrite;
use central_link_client::{MemberOutcome, RequestStatus};
use central_link_protocol::{ReplyPayload, RequestPayload};
use runtime_primitives::SyncTime;
use sqlite_storage_maintenance::TransactionStart;
use workspace_model::{Caller, RefusalReason, RequestId};

#[derive(Debug, thiserror::Error)]
pub enum OutboxStoreError {
    #[error("outbox storage unavailable")]
    Database(#[from] sqlx::Error),
    #[error("outbox write could not be encoded")]
    Encode(#[from] serde_json::Error),
    #[error("outbox item {request_id} has an invalid stored {field}")]
    CorruptItem {
        request_id: String,
        field: &'static str,
    },
    #[error("the payload does not carry request id {request}")]
    RequestIdMismatch { request: RequestId },
    #[error("outbox item {request} is not {expected}")]
    UnexpectedState {
        request: RequestId,
        expected: &'static str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutboxItemState {
    InOutbox,
    Sent,
    Accepted,
    Refused,
}

impl OutboxItemState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::InOutbox => "inOutbox",
            Self::Sent => "sent",
            Self::Accepted => "accepted",
            Self::Refused => "refused",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "inOutbox" => Some(Self::InOutbox),
            "sent" => Some(Self::Sent),
            "accepted" => Some(Self::Accepted),
            "refused" => Some(Self::Refused),
            _ => None,
        }
    }
}

/// A write entering the outbox, with the mark its receipt promises.
#[derive(Clone, Debug)]
pub struct NewOutboxItem {
    pub request: RequestId,
    pub caller: Caller,
    pub payload: RequestPayload,
    pub flush_at: SyncTime,
}

#[derive(Debug, Eq, PartialEq)]
pub enum OutboxInsert {
    Stored,
    /// The same caller already stored this exact write.
    AlreadyStored(RequestStatus),
    /// The id is taken by a different write or another caller.
    ConflictingContent,
}

/// A write loaded for sending in a batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredOutboxWrite {
    pub request: RequestId,
    pub caller: Caller,
    pub payload: RequestPayload,
}

struct OutboxRow {
    request_id: String,
    caller_json: String,
    payload_json: String,
    item_state: String,
    flush_at_ms: i64,
    outcome_json: Option<String>,
}

/// The outbox table, borrowed from the provider-operations store whose
/// connection and signalled transactions it uses.
pub struct MacOutbox<'store> {
    store: &'store mut ProviderOperationStore,
}

impl<'store> MacOutbox<'store> {
    pub fn new(store: &'store mut ProviderOperationStore) -> Self {
        Self { store }
    }

    /// Stores a write in the outbox, or reports the earlier identical write.
    pub async fn insert(
        &mut self,
        item: &NewOutboxItem,
        now: SyncTime,
    ) -> Result<OutboxInsert, OutboxStoreError> {
        if item.payload.write_request_id() != Some(&item.request) {
            return Err(OutboxStoreError::RequestIdMismatch {
                request: item.request.clone(),
            });
        }
        let request_id = item.request.as_str();
        let caller_json = serde_json::to_string(&item.caller)?;
        let payload_json = serde_json::to_string(&item.payload)?;
        let state = OutboxItemState::InOutbox.as_str();
        let flush_at_ms = item.flush_at.unix_millis();
        let now_ms = now.unix_millis();
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let existing = sqlx::query_as!(
            OutboxRow,
            r#"SELECT request_id AS "request_id!", caller_json, payload_json, item_state,
                      flush_at_ms, outcome_json
               FROM outbox_items WHERE request_id = ?"#,
            request_id
        )
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = existing {
            let stored = decode_row(&row)?;
            return Ok(
                if stored.caller == item.caller && stored.payload == item.payload {
                    OutboxInsert::AlreadyStored(stored.status)
                } else {
                    OutboxInsert::ConflictingContent
                },
            );
        }
        sqlx::query!(
            "INSERT INTO outbox_items
               (request_id, caller_json, payload_json, item_state, flush_at_ms, outcome_json,
                created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, NULL, ?, ?)",
            request_id,
            caller_json,
            payload_json,
            state,
            flush_at_ms,
            now_ms,
            now_ms
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(OutboxInsert::Stored)
    }

    /// The caller's own write; another caller's id answers NotFound.
    pub async fn request_status(
        &mut self,
        caller: &Caller,
        request: &RequestId,
    ) -> Result<RequestStatus, OutboxStoreError> {
        let request_id = request.as_str();
        let row = sqlx::query_as!(
            OutboxRow,
            r#"SELECT request_id AS "request_id!", caller_json, payload_json, item_state,
                      flush_at_ms, outcome_json
               FROM outbox_items WHERE request_id = ?"#,
            request_id
        )
        .fetch_optional(&mut self.store.connection)
        .await?;
        let Some(row) = row else {
            return Ok(RequestStatus::NotFound);
        };
        let stored = decode_row(&row)?;
        Ok(if stored.caller == *caller {
            stored.status
        } else {
            RequestStatus::NotFound
        })
    }

    /// Writes not yet sent, with their receipt marks, to load the flush reducer.
    pub async fn waiting(&mut self) -> Result<Vec<(SyncTime, RequestId)>, OutboxStoreError> {
        let state = OutboxItemState::InOutbox.as_str();
        let rows = sqlx::query!(
            r#"SELECT request_id AS "request_id!", flush_at_ms FROM outbox_items
               WHERE item_state = ? ORDER BY flush_at_ms, request_id"#,
            state
        )
        .fetch_all(&mut self.store.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    SyncTime::from_unix_millis(row.flush_at_ms),
                    parse_request_id(&row.request_id)?,
                ))
            })
            .collect()
    }

    /// Writes sent whose answer never came, e.g. across a restart; the Host
    /// returns them to the outbox to resend under the same RequestId.
    pub async fn sent_without_answer(&mut self) -> Result<Vec<RequestId>, OutboxStoreError> {
        let state = OutboxItemState::Sent.as_str();
        let rows = sqlx::query_scalar!(
            r#"SELECT request_id AS "request_id!" FROM outbox_items
               WHERE item_state = ? ORDER BY request_id"#,
            state
        )
        .fetch_all(&mut self.store.connection)
        .await?;
        rows.iter().map(|id| parse_request_id(id)).collect()
    }

    /// Moves outbox writes to sent and returns what goes in the batch frame.
    pub async fn mark_sent(
        &mut self,
        requests: &[RequestId],
        now: SyncTime,
    ) -> Result<Vec<StoredOutboxWrite>, OutboxStoreError> {
        let from = OutboxItemState::InOutbox.as_str();
        let to = OutboxItemState::Sent.as_str();
        let now_ms = now.unix_millis();
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let mut writes = Vec::with_capacity(requests.len());
        for request in requests {
            let request_id = request.as_str();
            let row = sqlx::query_as!(
                OutboxRow,
                r#"UPDATE outbox_items SET item_state = ?, updated_at = ?
                   WHERE request_id = ? AND item_state = ?
                   RETURNING request_id AS "request_id!", caller_json, payload_json,
                             item_state, flush_at_ms, outcome_json"#,
                to,
                now_ms,
                request_id,
                from
            )
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| OutboxStoreError::UnexpectedState {
                request: request.clone(),
                expected: "in the outbox",
            })?;
            let stored = decode_row(&row)?;
            writes.push(StoredOutboxWrite {
                request: request.clone(),
                caller: stored.caller,
                payload: stored.payload,
            });
        }
        transaction.commit().await?;
        Ok(writes)
    }

    /// Keeps central's answer for one sent write.
    pub async fn record_outcome(
        &mut self,
        request: &RequestId,
        outcome: &MemberOutcome,
        now: SyncTime,
    ) -> Result<(), OutboxStoreError> {
        let (state, outcome_json) = match outcome {
            MemberOutcome::Accepted(reply) => {
                (OutboxItemState::Accepted, serde_json::to_string(reply)?)
            }
            MemberOutcome::Refused(reason) => {
                (OutboxItemState::Refused, serde_json::to_string(reason)?)
            }
        };
        let to = state.as_str();
        let from = OutboxItemState::Sent.as_str();
        let now_ms = now.unix_millis();
        let request_id = request.as_str();
        let changed = sqlx::query!(
            "UPDATE outbox_items SET item_state = ?, outcome_json = ?, updated_at = ?
             WHERE request_id = ? AND item_state = ?",
            to,
            outcome_json,
            now_ms,
            request_id,
            from
        )
        .execute_as_transaction(self.store)
        .await?
        .rows_affected();
        expect_one(changed, request, "sent")
    }

    /// Sent writes left unanswered go back to the outbox with their receipt mark.
    pub async fn return_unanswered(
        &mut self,
        requests: &[RequestId],
        now: SyncTime,
    ) -> Result<(), OutboxStoreError> {
        let from = OutboxItemState::Sent.as_str();
        let to = OutboxItemState::InOutbox.as_str();
        let now_ms = now.unix_millis();
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        for request in requests {
            let request_id = request.as_str();
            let changed = sqlx::query!(
                "UPDATE outbox_items SET item_state = ?, updated_at = ?
                 WHERE request_id = ? AND item_state = ?",
                to,
                now_ms,
                request_id,
                from
            )
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            expect_one(changed, request, "sent")?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

struct DecodedRow {
    caller: Caller,
    payload: RequestPayload,
    status: RequestStatus,
}

fn decode_row(row: &OutboxRow) -> Result<DecodedRow, OutboxStoreError> {
    let corrupt = |field| OutboxStoreError::CorruptItem {
        request_id: row.request_id.clone(),
        field,
    };
    parse_request_id(&row.request_id)?;
    let caller = serde_json::from_str(&row.caller_json).map_err(|_| corrupt("caller_json"))?;
    let payload: RequestPayload =
        serde_json::from_str(&row.payload_json).map_err(|_| corrupt("payload_json"))?;
    if payload.write_request_id().map(RequestId::as_str) != Some(row.request_id.as_str()) {
        return Err(corrupt("payload_json"));
    }
    let state = OutboxItemState::parse(&row.item_state).ok_or_else(|| corrupt("item_state"))?;
    let status = match (state, row.outcome_json.as_deref()) {
        (OutboxItemState::InOutbox, None) => RequestStatus::InOutbox {
            flush_at: SyncTime::from_unix_millis(row.flush_at_ms),
        },
        (OutboxItemState::Sent, None) => RequestStatus::Sent,
        (OutboxItemState::Accepted, Some(outcome)) => RequestStatus::Accepted(
            serde_json::from_str::<ReplyPayload>(outcome).map_err(|_| corrupt("outcome_json"))?,
        ),
        (OutboxItemState::Refused, Some(outcome)) => RequestStatus::Refused(
            serde_json::from_str::<RefusalReason>(outcome).map_err(|_| corrupt("outcome_json"))?,
        ),
        (OutboxItemState::InOutbox | OutboxItemState::Sent, Some(_))
        | (OutboxItemState::Accepted | OutboxItemState::Refused, None) => {
            return Err(corrupt("outcome_json"));
        }
    };
    Ok(DecodedRow {
        caller,
        payload,
        status,
    })
}

fn parse_request_id(value: &str) -> Result<RequestId, OutboxStoreError> {
    value.parse().map_err(|_| OutboxStoreError::CorruptItem {
        request_id: value.to_owned(),
        field: "request_id",
    })
}

fn expect_one(
    changed: u64,
    request: &RequestId,
    expected: &'static str,
) -> Result<(), OutboxStoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(OutboxStoreError::UnexpectedState {
            request: request.clone(),
            expected,
        })
    }
}

#[cfg(test)]
#[path = "outbox_store_test.rs"]
mod tests;
