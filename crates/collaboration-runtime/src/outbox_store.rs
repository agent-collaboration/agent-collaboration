//! The Mac outbox in machine_local.sqlite (owner 2026-11-15, record:210). Every
//! time comes from the caller's injected clock; stored values are decoded
//! through Rust types and an invalid row is refused, never coerced.
use crate::MachineLocalStore;
use crate::machine_local_store::MachineLocalWrite;
use central_link_client::{
    LinkAdmissionRefusal, MemberOutcome, OutboxCancellation, OutboxWriteContent, OutboxWriteTarget,
    OutgoingOutcome, OutgoingWrite, PendingWrite, PendingWriteState, RequestStatus,
};
use central_link_protocol::{ReplyPayload, RequestPayload};
use runtime_primitives::SyncTime;
use sqlite_storage_maintenance::TransactionStart;
use workspace_model::{Caller, RequestId, operation_content_digest};

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
    #[error("outbox write content could not be digested")]
    Digest,
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
    /// Went out in a batch whose answer was lost; back in the outbox to be
    /// resent with the same id, and may already be in central.
    SentAnswerLost,
    Sent,
    Answered,
    Refused,
    /// A direct message not sent within 15 minutes (owner, record:358); its
    /// text is gone.
    Cancelled,
    /// Any other write still unsent after 24 hours; its text is gone.
    GaveUp,
}

impl OutboxItemState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::InOutbox => "inOutbox",
            Self::SentAnswerLost => "sentAnswerLost",
            Self::Sent => "sent",
            Self::Answered => "answered",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
            Self::GaveUp => "gaveUp",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "inOutbox" => Some(Self::InOutbox),
            "sentAnswerLost" => Some(Self::SentAnswerLost),
            "sent" => Some(Self::Sent),
            "answered" => Some(Self::Answered),
            "refused" => Some(Self::Refused),
            "cancelled" => Some(Self::Cancelled),
            "gaveUp" => Some(Self::GaveUp),
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

struct OutgoingRow {
    request_id: String,
    caller_json: String,
    payload_json: String,
    item_state: String,
    flush_at_ms: i64,
    outcome_json: Option<String>,
    updated_at: i64,
}

/// Answered writes are kept this long after their answer, then deleted.
pub(crate) const ANSWERED_RETENTION_MILLIS: i64 = 60 * 24 * 60 * 60 * 1000;

/// The outbox table, borrowed from the machine-local store whose
/// connection and signalled transactions it uses.
pub struct MacOutbox<'store> {
    store: &'store mut MachineLocalStore,
}

impl<'store> MacOutbox<'store> {
    pub fn new(store: &'store mut MachineLocalStore) -> Self {
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
            let same_write = match &stored.payload {
                StoredPayload::Unanswered(payload) => payload == &item.payload,
                // The text is gone once answered. The same write (replay its answer)
                // has the same target, workspace included, and the same content
                // digest; any difference is a conflict. The outbox spans workspaces.
                StoredPayload::Answered(answered) => {
                    OutboxWriteContent::of(&item.payload)
                        .is_some_and(|content| content.target == answered.target)
                        && payload_content_digest(&item.payload)? == answered.content_digest
                }
            };
            return Ok(if stored.caller == item.caller && same_write {
                OutboxInsert::AlreadyStored(stored.status)
            } else {
                OutboxInsert::ConflictingContent
            });
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

    /// The caller's own writes not yet resolved, oldest first: unsent, sent
    /// with a lost answer (or still in flight), and answered ones the read
    /// layer drops once it sees their created ids. A refused write never
    /// reached central and is not pending.
    pub async fn pending_for(
        &mut self,
        caller: &Caller,
    ) -> Result<Vec<PendingWrite>, OutboxStoreError> {
        let caller_json = serde_json::to_string(caller)?;
        let refused = OutboxItemState::Refused.as_str();
        let rows = sqlx::query_as!(
            OutboxRow,
            r#"SELECT request_id AS "request_id!", caller_json, payload_json, item_state,
                      flush_at_ms, outcome_json
               FROM outbox_items WHERE caller_json = ? AND item_state <> ?
               ORDER BY created_at, request_id"#,
            caller_json,
            refused
        )
        .fetch_all(&mut self.store.connection)
        .await?;
        let mut pending = Vec::with_capacity(rows.len());
        for row in &rows {
            let stored = decode_owned_row(row, caller)?;
            let (target, state) = match (stored.state, stored.status, stored.payload) {
                (OutboxItemState::InOutbox, _, StoredPayload::Unanswered(payload)) => {
                    let content = outbox_content(&row.request_id, &payload)?;
                    (
                        content.target,
                        PendingWriteState::Unsent { body: content.body },
                    )
                }
                (
                    OutboxItemState::SentAnswerLost | OutboxItemState::Sent,
                    _,
                    StoredPayload::Unanswered(payload),
                ) => {
                    let content = outbox_content(&row.request_id, &payload)?;
                    let body = content.body;
                    (content.target, PendingWriteState::SentAnswerLost { body })
                }
                (
                    OutboxItemState::Answered,
                    RequestStatus::Answered(reply),
                    StoredPayload::Answered(answered),
                ) => (answered.target, PendingWriteState::Answered(reply)),
                (
                    OutboxItemState::Cancelled | OutboxItemState::GaveUp,
                    RequestStatus::Cancelled { reason },
                    StoredPayload::Answered(cancelled),
                ) => (cancelled.target, PendingWriteState::Cancelled { reason }),
                _ => return Err(corrupt_row(&row.request_id, "item_state")),
            };
            pending.push(PendingWrite {
                request_id: parse_request_id(&row.request_id)?,
                target,
                flush_at: SyncTime::from_unix_millis(row.flush_at_ms),
                state,
            });
        }
        Ok(pending)
    }

    /// The caller's recently answered writes, newest answer first.
    pub async fn outgoing_for(
        &mut self,
        caller: &Caller,
        limit: u16,
    ) -> Result<Vec<OutgoingWrite>, OutboxStoreError> {
        let caller_json = serde_json::to_string(caller)?;
        let answered = OutboxItemState::Answered.as_str();
        let refused = OutboxItemState::Refused.as_str();
        let limit = i64::from(limit);
        let rows = sqlx::query_as!(
            OutgoingRow,
            r#"SELECT request_id AS "request_id!", caller_json, payload_json, item_state,
                      flush_at_ms, outcome_json, updated_at
               FROM outbox_items WHERE caller_json = ? AND item_state IN (?, ?)
               ORDER BY updated_at DESC, request_id DESC LIMIT ?"#,
            caller_json,
            answered,
            refused,
            limit
        )
        .fetch_all(&mut self.store.connection)
        .await?;
        let mut outgoing = Vec::with_capacity(rows.len());
        for row in rows {
            let request_id = row.request_id.clone();
            let answered_at = SyncTime::from_unix_millis(row.updated_at);
            let row = OutboxRow {
                request_id: row.request_id,
                caller_json: row.caller_json,
                payload_json: row.payload_json,
                item_state: row.item_state,
                flush_at_ms: row.flush_at_ms,
                outcome_json: row.outcome_json,
            };
            let stored = decode_owned_row(&row, caller)?;
            let StoredPayload::Answered(answered) = stored.payload else {
                return Err(corrupt_row(&request_id, "payload_json"));
            };
            let outcome = match stored.status {
                RequestStatus::Answered(reply) => OutgoingOutcome::Answered(reply),
                RequestStatus::Refused(reason) => OutgoingOutcome::Refused(reason),
                RequestStatus::NotFound
                | RequestStatus::InOutbox { .. }
                | RequestStatus::Sent
                | RequestStatus::Cancelled { .. } => {
                    return Err(corrupt_row(&request_id, "item_state"));
                }
            };
            outgoing.push(OutgoingWrite {
                request_id: parse_request_id(&request_id)?,
                target: answered.target,
                outcome,
                answered_at,
            });
        }
        Ok(outgoing)
    }

    /// Deletes ended writes (answered, refused, cancelled or given up) 60 days
    /// after they ended, on the injected clock.
    pub async fn delete_ended_before_retention(
        &mut self,
        now: SyncTime,
    ) -> Result<u64, OutboxStoreError> {
        let cutoff = now.unix_millis().saturating_sub(ANSWERED_RETENTION_MILLIS);
        let answered = OutboxItemState::Answered.as_str();
        let refused = OutboxItemState::Refused.as_str();
        let cancelled = OutboxItemState::Cancelled.as_str();
        let gave_up = OutboxItemState::GaveUp.as_str();
        Ok(sqlx::query!(
            "DELETE FROM outbox_items WHERE item_state IN (?, ?, ?, ?) AND updated_at <= ?",
            answered,
            refused,
            cancelled,
            gave_up,
            cutoff
        )
        .execute_as_transaction(self.store)
        .await?
        .rows_affected())
    }

    /// Writes waiting to be sent (never sent, or sent with a lost answer), with
    /// their receipt marks, to load the flush reducer.
    pub async fn waiting(&mut self) -> Result<Vec<(SyncTime, RequestId)>, OutboxStoreError> {
        let unsent = OutboxItemState::InOutbox.as_str();
        let answer_lost = OutboxItemState::SentAnswerLost.as_str();
        let rows = sqlx::query!(
            r#"SELECT request_id AS "request_id!", flush_at_ms FROM outbox_items
               WHERE item_state IN (?, ?) ORDER BY flush_at_ms, request_id"#,
            unsent,
            answer_lost
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
        let unsent = OutboxItemState::InOutbox.as_str();
        let answer_lost = OutboxItemState::SentAnswerLost.as_str();
        let to = OutboxItemState::Sent.as_str();
        let now_ms = now.unix_millis();
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let mut writes = Vec::with_capacity(requests.len());
        for request in requests {
            let request_id = request.as_str();
            let row = sqlx::query_as!(
                OutboxRow,
                r#"UPDATE outbox_items SET item_state = ?, updated_at = ?
                   WHERE request_id = ? AND item_state IN (?, ?)
                   RETURNING request_id AS "request_id!", caller_json, payload_json,
                             item_state, flush_at_ms, outcome_json"#,
                to,
                now_ms,
                request_id,
                unsent,
                answer_lost
            )
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| OutboxStoreError::UnexpectedState {
                request: request.clone(),
                expected: "in the outbox",
            })?;
            let stored = decode_row(&row)?;
            let StoredPayload::Unanswered(payload) = stored.payload else {
                return Err(corrupt_row(request_id, "payload_json"));
            };
            writes.push(StoredOutboxWrite {
                request: request.clone(),
                caller: stored.caller,
                payload,
            });
        }
        transaction.commit().await?;
        Ok(writes)
    }

    /// Claims the next batch: up to `limit` writes waiting for `mark` or
    /// earlier, oldest first by (created_at, request_id), moved to sent in one
    /// transaction and returned for the batch frame. The table is the only
    /// list of what waits (Lead A, 2026-11-18): nothing is held in memory.
    pub async fn claim_next_batch(
        &mut self,
        mark: SyncTime,
        limit: u16,
        now: SyncTime,
    ) -> Result<Vec<StoredOutboxWrite>, OutboxStoreError> {
        let unsent = OutboxItemState::InOutbox.as_str();
        let answer_lost = OutboxItemState::SentAnswerLost.as_str();
        let to = OutboxItemState::Sent.as_str();
        let mark_ms = mark.unix_millis();
        let now_ms = now.unix_millis();
        let limit = i64::from(limit);
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let due = sqlx::query_scalar!(
            r#"SELECT request_id AS "request_id!" FROM outbox_items
               WHERE item_state IN (?, ?) AND flush_at_ms <= ?
               ORDER BY created_at, request_id LIMIT ?"#,
            unsent,
            answer_lost,
            mark_ms,
            limit
        )
        .fetch_all(&mut *transaction)
        .await?;
        let mut writes = Vec::with_capacity(due.len());
        for request_id in &due {
            let row = sqlx::query_as!(
                OutboxRow,
                r#"UPDATE outbox_items SET item_state = ?, updated_at = ?
                   WHERE request_id = ? AND item_state IN (?, ?)
                   RETURNING request_id AS "request_id!", caller_json, payload_json,
                             item_state, flush_at_ms, outcome_json"#,
                to,
                now_ms,
                request_id,
                unsent,
                answer_lost
            )
            .fetch_one(&mut *transaction)
            .await?;
            let stored = decode_row(&row)?;
            let StoredPayload::Unanswered(payload) = stored.payload else {
                return Err(corrupt_row(request_id, "payload_json"));
            };
            writes.push(StoredOutboxWrite {
                request: parse_request_id(request_id)?,
                caller: stored.caller,
                payload,
            });
        }
        transaction.commit().await?;
        Ok(writes)
    }

    /// The earliest receipt mark among waiting writes, for arming the flush.
    pub async fn earliest_waiting(&mut self) -> Result<Option<SyncTime>, OutboxStoreError> {
        let unsent = OutboxItemState::InOutbox.as_str();
        let answer_lost = OutboxItemState::SentAnswerLost.as_str();
        let earliest = sqlx::query_scalar!(
            r#"SELECT MIN(flush_at_ms) AS "earliest: i64" FROM outbox_items
               WHERE item_state IN (?, ?)"#,
            unsent,
            answer_lost
        )
        .fetch_one(&mut self.store.connection)
        .await?;
        Ok(earliest.map(SyncTime::from_unix_millis))
    }

    /// Keeps central's answer for one sent write and, in the same transaction,
    /// drops its text: the stored payload becomes the body-free answered record
    /// (owner, 2026-11-15: unsent text stays on the Mac only until central
    /// answers).
    pub async fn record_outcome(
        &mut self,
        request: &RequestId,
        outcome: &MemberOutcome,
        now: SyncTime,
    ) -> Result<(), OutboxStoreError> {
        let (state, outcome_json) = match outcome {
            MemberOutcome::Answered(reply) => {
                (OutboxItemState::Answered, serde_json::to_string(reply)?)
            }
            MemberOutcome::Refused(reason) => {
                (OutboxItemState::Refused, serde_json::to_string(reason)?)
            }
        };
        let to = state.as_str();
        let from = OutboxItemState::Sent.as_str();
        let now_ms = now.unix_millis();
        let request_id = request.as_str();
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let row = sqlx::query_as!(
            OutboxRow,
            r#"SELECT request_id AS "request_id!", caller_json, payload_json, item_state,
                      flush_at_ms, outcome_json
               FROM outbox_items WHERE request_id = ? AND item_state = ?"#,
            request_id,
            from
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| OutboxStoreError::UnexpectedState {
            request: request.clone(),
            expected: "sent",
        })?;
        let StoredPayload::Unanswered(payload) = decode_row(&row)?.payload else {
            return Err(corrupt_row(request_id, "payload_json"));
        };
        let answered = AnsweredWrite {
            request_id: request.clone(),
            target: outbox_content(request_id, &payload)?.target,
            content_digest: payload_content_digest(&payload)?,
        };
        let payload_json = serde_json::to_string(&answered)?;
        let changed = sqlx::query!(
            "UPDATE outbox_items SET item_state = ?, outcome_json = ?, payload_json = ?,
                    updated_at = ?
             WHERE request_id = ? AND item_state = ?",
            to,
            outcome_json,
            payload_json,
            now_ms,
            request_id,
            from
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        expect_one(changed, request, "sent")?;
        transaction.commit().await?;
        Ok(())
    }

    /// A sent write central did not apply (link admission refused it) goes back
    /// to the outbox unsent, with its receipt mark, to be resent after a
    /// fresh-token reconnect.
    pub async fn return_unapplied(
        &mut self,
        request: &RequestId,
        now: SyncTime,
    ) -> Result<(), OutboxStoreError> {
        let from = OutboxItemState::Sent.as_str();
        let to = OutboxItemState::InOutbox.as_str();
        let now_ms = now.unix_millis();
        let request_id = request.as_str();
        let changed = sqlx::query!(
            "UPDATE outbox_items SET item_state = ?, updated_at = ?
                 WHERE request_id = ? AND item_state = ?",
            to,
            now_ms,
            request_id,
            from
        )
        .execute_as_transaction(self.store)
        .await?
        .rows_affected();
        expect_one(changed, request, "sent")
    }

    /// Sent writes left unanswered go back to the outbox with their receipt
    /// mark, marked as possibly already in central.
    pub async fn return_unanswered(
        &mut self,
        requests: &[RequestId],
        now: SyncTime,
    ) -> Result<(), OutboxStoreError> {
        let from = OutboxItemState::Sent.as_str();
        let to = OutboxItemState::SentAnswerLost.as_str();
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

/// What `payload_json` holds after central answered: the target and the content
/// digest central's request log uses, no text.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnsweredWrite {
    request_id: RequestId,
    target: OutboxWriteTarget,
    /// Tells a resubmit of the same write from a different write under the same id.
    content_digest: String,
}

/// `payload_json` by row state: the whole write until central answers, then
/// the body-free answered record.
enum StoredPayload {
    Unanswered(RequestPayload),
    Answered(AnsweredWrite),
}

struct DecodedRow {
    state: OutboxItemState,
    caller: Caller,
    payload: StoredPayload,
    status: RequestStatus,
}

fn decode_row(row: &OutboxRow) -> Result<DecodedRow, OutboxStoreError> {
    let corrupt = |field| OutboxStoreError::CorruptItem {
        request_id: row.request_id.clone(),
        field,
    };
    parse_request_id(&row.request_id)?;
    let caller = serde_json::from_str(&row.caller_json).map_err(|_| corrupt("caller_json"))?;
    let state = OutboxItemState::parse(&row.item_state).ok_or_else(|| corrupt("item_state"))?;
    let payload = match state {
        OutboxItemState::InOutbox | OutboxItemState::SentAnswerLost | OutboxItemState::Sent => {
            let payload: RequestPayload =
                serde_json::from_str(&row.payload_json).map_err(|_| corrupt("payload_json"))?;
            if payload.write_request_id().map(RequestId::as_str) != Some(row.request_id.as_str()) {
                return Err(corrupt("payload_json"));
            }
            StoredPayload::Unanswered(payload)
        }
        OutboxItemState::Answered
        | OutboxItemState::Refused
        | OutboxItemState::Cancelled
        | OutboxItemState::GaveUp => {
            let answered: AnsweredWrite =
                serde_json::from_str(&row.payload_json).map_err(|_| corrupt("payload_json"))?;
            if answered.request_id.as_str() != row.request_id {
                return Err(corrupt("payload_json"));
            }
            StoredPayload::Answered(answered)
        }
    };
    let status = match (state, row.outcome_json.as_deref()) {
        (OutboxItemState::InOutbox | OutboxItemState::SentAnswerLost, None) => {
            RequestStatus::InOutbox {
                flush_at: SyncTime::from_unix_millis(row.flush_at_ms),
            }
        }
        (OutboxItemState::Sent, None) => RequestStatus::Sent,
        (OutboxItemState::Answered, Some(outcome)) => RequestStatus::Answered(
            serde_json::from_str::<ReplyPayload>(outcome).map_err(|_| corrupt("outcome_json"))?,
        ),
        (OutboxItemState::Refused, Some(outcome)) => RequestStatus::Refused(
            serde_json::from_str::<LinkAdmissionRefusal>(outcome)
                .map_err(|_| corrupt("outcome_json"))?,
        ),
        (OutboxItemState::Cancelled, None) => RequestStatus::Cancelled {
            reason: OutboxCancellation::NotSentWithin15Min,
        },
        (OutboxItemState::GaveUp, None) => RequestStatus::Cancelled {
            reason: OutboxCancellation::GaveUpAfter24h,
        },
        (
            OutboxItemState::InOutbox
            | OutboxItemState::SentAnswerLost
            | OutboxItemState::Sent
            | OutboxItemState::Cancelled
            | OutboxItemState::GaveUp,
            Some(_),
        )
        | (OutboxItemState::Answered | OutboxItemState::Refused, None) => {
            return Err(corrupt("outcome_json"));
        }
    };
    Ok(DecodedRow {
        state,
        caller,
        payload,
        status,
    })
}

/// Decodes a row read by caller and checks it really is that caller's.
fn decode_owned_row(row: &OutboxRow, caller: &Caller) -> Result<DecodedRow, OutboxStoreError> {
    let stored = decode_row(row)?;
    if stored.caller != *caller {
        return Err(corrupt_row(&row.request_id, "caller_json"));
    }
    Ok(stored)
}

fn outbox_content(
    request_id: &str,
    payload: &RequestPayload,
) -> Result<OutboxWriteContent, OutboxStoreError> {
    OutboxWriteContent::of(payload).ok_or_else(|| corrupt_row(request_id, "payload_json"))
}

/// The digest central's request log keys this write's content by.
pub(crate) fn payload_content_digest(payload: &RequestPayload) -> Result<String, OutboxStoreError> {
    let digest = match payload {
        RequestPayload::Workspace(request) => operation_content_digest(&request.operation),
        RequestPayload::Mail(request) => operation_content_digest(&request.operation),
    };
    digest.map_err(|_| OutboxStoreError::Digest)
}

fn corrupt_row(request_id: &str, field: &'static str) -> OutboxStoreError {
    OutboxStoreError::CorruptItem {
        request_id: request_id.to_owned(),
        field,
    }
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
