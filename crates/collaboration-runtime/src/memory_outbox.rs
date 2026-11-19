//! The outbox in memory, for simulations (test-support only): MacOutbox's
//! behaviour without a database thread, so every store call completes inside
//! the simulated host. The shared contract suite (outbox_storage_contract)
//! holds it to MachineLocalStore's behaviour.
use crate::outbox_deadline::{DepartureAtDeadline, end_reason, expires_at_ms};
use crate::outbox_storage::{OutboxFuture, OutboxStorage};
use crate::outbox_store::{
    ANSWERED_RETENTION_MILLIS, ClaimedBatch, NewOutboxItem, OutboxInsert, OutboxStoreError,
    StoredOutboxWrite, payload_content_digest, with_send_time,
};
use central_link_client::{
    EndedWrite, LinkAdmissionRefusal, MemberOutcome, OutboxCancellation, OutboxWriteContent,
    OutboxWriteTarget, OutgoingOutcome, OutgoingWrite, PendingWrite, PendingWriteState,
    RequestStatus,
};
use central_link_protocol::{ReplyPayload, RequestPayload};
use runtime_primitives::SyncTime;
use std::collections::BTreeMap;
use workspace_model::{Caller, RequestId};

#[derive(Clone, Debug)]
enum RowState {
    InOutbox,
    SentAnswerLost,
    /// First attempt on the wire (claimed from InOutbox).
    Sent,
    /// A replay on the wire (claimed from SentAnswerLost).
    Resent,
    Answered(ReplyPayload),
    Refused(LinkAdmissionRefusal),
    /// Past its deadline (outbox_deadline), its text gone.
    Ended(OutboxCancellation),
}

#[derive(Clone, Debug)]
struct Row {
    caller: Caller,
    /// The whole write until it ends; None once answered, refused or cancelled.
    payload: Option<RequestPayload>,
    target: OutboxWriteTarget,
    content_digest: String,
    flush_at: SyncTime,
    expires_at: i64,
    state: RowState,
    created_at: i64,
    updated_at: i64,
}

impl Row {
    fn status(&self) -> RequestStatus {
        match &self.state {
            RowState::InOutbox | RowState::SentAnswerLost => RequestStatus::InOutbox {
                flush_at: self.flush_at,
            },
            RowState::Sent | RowState::Resent => RequestStatus::Sent,
            RowState::Answered(reply) => RequestStatus::Answered(reply.clone()),
            RowState::Refused(reason) => RequestStatus::Refused(*reason),
            RowState::Ended(reason) => RequestStatus::Cancelled { reason: *reason },
        }
    }
}

#[derive(Default)]
pub struct MemoryOutbox {
    rows: BTreeMap<RequestId, Row>,
}

impl MemoryOutbox {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn expect_state(
        &mut self,
        request: &RequestId,
        allowed: fn(&RowState) -> bool,
        expected: &'static str,
    ) -> Result<&mut Row, OutboxStoreError> {
        match self.rows.get_mut(request) {
            Some(row) if allowed(&row.state) => Ok(row),
            Some(_) | None => Err(OutboxStoreError::UnexpectedState {
                request: request.clone(),
                expected,
            }),
        }
    }
}

fn unsent(state: &RowState) -> bool {
    matches!(state, RowState::InOutbox | RowState::SentAnswerLost)
}

fn sent(state: &RowState) -> bool {
    matches!(state, RowState::Sent | RowState::Resent)
}

fn content(
    request: &RequestId,
    payload: &RequestPayload,
) -> Result<OutboxWriteContent, OutboxStoreError> {
    OutboxWriteContent::of(payload).ok_or_else(|| OutboxStoreError::CorruptItem {
        request_id: request.as_str().to_owned(),
        field: "payload_json",
    })
}

impl OutboxStorage for MemoryOutbox {
    fn insert<'call>(
        &'call mut self,
        item: &'call NewOutboxItem,
        now: SyncTime,
    ) -> OutboxFuture<'call, OutboxInsert> {
        Box::pin(async move {
            if item.payload.write_request_id() != Some(&item.request) {
                return Err(OutboxStoreError::RequestIdMismatch {
                    request: item.request.clone(),
                });
            }
            let digest = payload_content_digest(&item.payload)?;
            if let Some(row) = self.rows.get(&item.request) {
                let same_write = match &row.payload {
                    Some(payload) => payload == &item.payload,
                    None => {
                        OutboxWriteContent::of(&item.payload)
                            .is_some_and(|written| written.target == row.target)
                            && digest == row.content_digest
                    }
                };
                return Ok(if row.caller == item.caller && same_write {
                    OutboxInsert::AlreadyStored(row.status())
                } else {
                    OutboxInsert::ConflictingContent
                });
            }
            let target = content(&item.request, &item.payload)?.target;
            let expires_at = expires_at_ms(now.unix_millis());
            self.rows.insert(
                item.request.clone(),
                Row {
                    caller: item.caller.clone(),
                    payload: Some(item.payload.clone()),
                    target,
                    content_digest: digest,
                    flush_at: item.flush_at,
                    expires_at,
                    state: RowState::InOutbox,
                    created_at: now.unix_millis(),
                    updated_at: now.unix_millis(),
                },
            );
            Ok(OutboxInsert::Stored)
        })
    }

    fn request_status<'call>(
        &'call mut self,
        caller: &'call Caller,
        request: &'call RequestId,
    ) -> OutboxFuture<'call, RequestStatus> {
        Box::pin(async move {
            Ok(match self.rows.get(request) {
                Some(row) if &row.caller == caller => row.status(),
                Some(_) | None => RequestStatus::NotFound,
            })
        })
    }

    fn pending_for<'call>(
        &'call mut self,
        caller: &'call Caller,
    ) -> OutboxFuture<'call, Vec<PendingWrite>> {
        Box::pin(async move {
            let mut rows: Vec<(&RequestId, &Row)> = self
                .rows
                .iter()
                .filter(|(_, row)| &row.caller == caller)
                .filter(|(_, row)| !matches!(row.state, RowState::Refused(_)))
                .collect();
            // (created_at, request_id), exactly as the SQLite store orders.
            rows.sort_by_key(|(id, row)| (row.created_at, (*id).clone()));
            rows.into_iter()
                .map(|(id, row)| {
                    let body = || -> Result<_, OutboxStoreError> {
                        let payload =
                            row.payload
                                .as_ref()
                                .ok_or_else(|| OutboxStoreError::CorruptItem {
                                    request_id: id.as_str().to_owned(),
                                    field: "payload_json",
                                })?;
                        Ok(content(id, payload)?.body)
                    };
                    let state = match &row.state {
                        RowState::InOutbox => PendingWriteState::Unsent { body: body()? },
                        RowState::SentAnswerLost | RowState::Sent | RowState::Resent => {
                            PendingWriteState::SentAnswerLost { body: body()? }
                        }
                        RowState::Answered(reply) => PendingWriteState::Answered(reply.clone()),
                        RowState::Ended(reason) => PendingWriteState::Cancelled { reason: *reason },
                        RowState::Refused(_) => {
                            return Err(OutboxStoreError::CorruptItem {
                                request_id: id.as_str().to_owned(),
                                field: "item_state",
                            });
                        }
                    };
                    Ok(PendingWrite {
                        request_id: id.clone(),
                        target: row.target.clone(),
                        flush_at: row.flush_at,
                        state,
                    })
                })
                .collect()
        })
    }

    fn outgoing_for<'call>(
        &'call mut self,
        caller: &'call Caller,
        limit: u16,
    ) -> OutboxFuture<'call, Vec<OutgoingWrite>> {
        Box::pin(async move {
            let mut rows: Vec<(&RequestId, &Row)> = self
                .rows
                .iter()
                .filter(|(_, row)| &row.caller == caller)
                .filter(|(_, row)| {
                    matches!(row.state, RowState::Answered(_) | RowState::Refused(_))
                })
                .collect();
            rows.sort_by(|(left_id, left), (right_id, right)| {
                (right.updated_at, *right_id).cmp(&(left.updated_at, *left_id))
            });
            Ok(rows
                .into_iter()
                .take(usize::from(limit))
                .filter_map(|(id, row)| {
                    let outcome = match &row.state {
                        RowState::Answered(reply) => OutgoingOutcome::Answered(reply.clone()),
                        RowState::Refused(reason) => OutgoingOutcome::Refused(*reason),
                        RowState::InOutbox
                        | RowState::SentAnswerLost
                        | RowState::Sent
                        | RowState::Resent
                        | RowState::Ended(_) => {
                            return None;
                        }
                    };
                    Some(OutgoingWrite {
                        request_id: id.clone(),
                        target: row.target.clone(),
                        outcome,
                        answered_at: SyncTime::from_unix_millis(row.updated_at),
                    })
                })
                .collect())
        })
    }

    fn delete_ended_before_retention(&mut self, now: SyncTime) -> OutboxFuture<'_, u64> {
        Box::pin(async move {
            let cutoff = now.unix_millis().saturating_sub(ANSWERED_RETENTION_MILLIS);
            let before = self.rows.len();
            self.rows.retain(|_, row| {
                let ended = matches!(
                    row.state,
                    RowState::Answered(_) | RowState::Refused(_) | RowState::Ended(_)
                );
                !(ended && row.updated_at <= cutoff)
            });
            Ok(u64::try_from(before - self.rows.len()).unwrap_or(u64::MAX))
        })
    }

    fn waiting(&mut self) -> OutboxFuture<'_, Vec<(SyncTime, RequestId)>> {
        Box::pin(async move {
            let mut waiting: Vec<(SyncTime, RequestId)> = self
                .rows
                .iter()
                .filter(|(_, row)| unsent(&row.state))
                .map(|(id, row)| (row.flush_at, id.clone()))
                .collect();
            waiting.sort();
            Ok(waiting)
        })
    }

    fn earliest_waiting(&mut self) -> OutboxFuture<'_, Option<SyncTime>> {
        Box::pin(async move {
            Ok(self
                .rows
                .values()
                .filter(|row| unsent(&row.state))
                .map(|row| row.flush_at)
                .min())
        })
    }

    fn earliest_deadline(&mut self) -> OutboxFuture<'_, Option<SyncTime>> {
        Box::pin(async move {
            Ok(self
                .rows
                .values()
                .filter(|row| unsent(&row.state))
                .map(|row| SyncTime::from_unix_millis(row.expires_at))
                .min())
        })
    }

    fn claim_next_batch(
        &mut self,
        mark: SyncTime,
        limit: u16,
        now: SyncTime,
    ) -> OutboxFuture<'_, ClaimedBatch> {
        Box::pin(async move {
            let ended = self.end_overdue_waiting(now)?;
            let mut due: Vec<(i64, RequestId)> = self
                .rows
                .iter()
                .filter(|(_, row)| {
                    unsent(&row.state) && row.flush_at <= mark && row.expires_at > now.unix_millis()
                })
                .map(|(id, row)| (row.created_at, id.clone()))
                .collect();
            due.sort();
            due.truncate(usize::from(limit));
            let requests: Vec<RequestId> = due.into_iter().map(|(_, id)| id).collect();
            let writes = self.mark_sent(&requests, now).await?;
            Ok(ClaimedBatch { writes, ended })
        })
    }

    fn end_overdue(&mut self, now: SyncTime) -> OutboxFuture<'_, Vec<EndedWrite>> {
        Box::pin(async move { self.end_overdue_waiting(now) })
    }

    fn sent_without_answer(&mut self) -> OutboxFuture<'_, Vec<RequestId>> {
        Box::pin(async move {
            Ok(self
                .rows
                .iter()
                .filter(|(_, row)| sent(&row.state))
                .map(|(id, _)| id.clone())
                .collect())
        })
    }

    fn mark_sent<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, Vec<StoredOutboxWrite>> {
        Box::pin(async move {
            let sendable = |row: &Row| unsent(&row.state) && row.expires_at > now.unix_millis();
            for request in requests {
                if !self.rows.get(request).is_some_and(sendable) {
                    return Err(OutboxStoreError::UnexpectedState {
                        request: request.clone(),
                        expected: "in the outbox before its deadline",
                    });
                }
            }
            let mut writes = Vec::with_capacity(requests.len());
            for request in requests {
                let row = self.expect_state(request, unsent, "in the outbox")?;
                row.state = if matches!(row.state, RowState::InOutbox) {
                    RowState::Sent
                } else {
                    RowState::Resent
                };
                row.updated_at = now.unix_millis();
                let payload = row
                    .payload
                    .clone()
                    .ok_or_else(|| OutboxStoreError::CorruptItem {
                        request_id: request.as_str().to_owned(),
                        field: "payload_json",
                    })?;
                writes.push(StoredOutboxWrite {
                    request: request.clone(),
                    caller: row.caller.clone(),
                    payload: with_send_time(payload, row.created_at)?,
                });
            }
            Ok(writes)
        })
    }

    fn record_outcome<'call>(
        &'call mut self,
        request: &'call RequestId,
        outcome: &'call MemberOutcome,
        now: SyncTime,
    ) -> OutboxFuture<'call, ()> {
        Box::pin(async move {
            let row = self.expect_state(request, sent, "sent")?;
            row.state = match outcome {
                MemberOutcome::Answered(reply) => RowState::Answered(reply.clone()),
                MemberOutcome::Refused(reason) => RowState::Refused(*reason),
            };
            // The text stays only until central answers (owner, record:210).
            row.payload = None;
            row.updated_at = now.unix_millis();
            Ok(())
        })
    }

    fn return_unapplied<'call>(
        &'call mut self,
        request: &'call RequestId,
        now: SyncTime,
    ) -> OutboxFuture<'call, ()> {
        Box::pin(async move {
            let row = self.expect_state(request, sent, "sent")?;
            row.state = if matches!(row.state, RowState::Sent) {
                RowState::InOutbox
            } else {
                RowState::SentAnswerLost
            };
            row.updated_at = now.unix_millis();
            Ok(())
        })
    }

    fn return_unanswered<'call>(
        &'call mut self,
        requests: &'call [RequestId],
        now: SyncTime,
    ) -> OutboxFuture<'call, Vec<EndedWrite>> {
        Box::pin(async move {
            for request in requests {
                self.expect_state(request, sent, "sent")?;
            }
            let mut ended = Vec::new();
            for request in requests {
                let row = self.expect_state(request, sent, "sent")?;
                if row.expires_at <= now.unix_millis() {
                    ended.push(end_row(
                        request,
                        row,
                        DepartureAtDeadline::MayHaveArrived,
                        now,
                    ));
                } else {
                    row.state = RowState::SentAnswerLost;
                    row.updated_at = now.unix_millis();
                }
            }
            Ok(ended)
        })
    }
}

impl MemoryOutbox {
    /// Every waiting row past its deadline ends, oldest first, as SQLite does.
    fn end_overdue_waiting(&mut self, now: SyncTime) -> Result<Vec<EndedWrite>, OutboxStoreError> {
        let mut overdue: Vec<(i64, RequestId)> = self
            .rows
            .iter()
            .filter(|(_, row)| unsent(&row.state) && row.expires_at <= now.unix_millis())
            .map(|(id, row)| (row.created_at, id.clone()))
            .collect();
        overdue.sort();
        let mut ended = Vec::with_capacity(overdue.len());
        for (_, request) in overdue {
            let row = self.expect_state(&request, unsent, "waiting")?;
            let departure = match row.state {
                RowState::InOutbox => DepartureAtDeadline::NeverLeft,
                RowState::SentAnswerLost
                | RowState::Sent
                | RowState::Resent
                | RowState::Answered(_)
                | RowState::Refused(_)
                | RowState::Ended(_) => DepartureAtDeadline::MayHaveArrived,
            };
            ended.push(end_row(&request, row, departure, now));
        }
        Ok(ended)
    }
}

/// Ends one overdue row and drops its text, keeping target and digest.
fn end_row(
    request: &RequestId,
    row: &mut Row,
    departure: DepartureAtDeadline,
    now: SyncTime,
) -> EndedWrite {
    let reason = end_reason(departure);
    row.state = RowState::Ended(reason);
    row.payload = None;
    row.updated_at = now.unix_millis();
    EndedWrite {
        caller: row.caller.clone(),
        request_id: request.clone(),
        target: row.target.clone(),
        reason,
    }
}

#[cfg(test)]
#[path = "memory_outbox_test.rs"]
mod tests;
