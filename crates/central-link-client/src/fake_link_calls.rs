//! Dev-only stand-in for the link below the seam. It honours the frozen
//! contract so tools can be built before the real link lands: outbox kinds
//! answer InOutbox and become Answered after a flush on their mark; immediate
//! kinds answer, or are Uncertain when the answer is lost; another caller's
//! request id answers NotFound. It proves nothing about the real link.
use crate::flush_marks::{earliest_open_mark, mark_in_window};
use crate::link_calls::{
    LinkCallFuture, LinkCalls, OutboxReceipt, RequestStatus, Submission, SubmitRefusal,
};
use crate::outbox_flush::MAX_BATCH_WRITES;
use crate::submission_path::{SubmissionPath, submission_path};
use crate::{QueuedWrite, ToolSendMessage};
use central_link_protocol::{
    CorrelationId, LinkRequest, ProtocolError, ReplyPayload, RequestPayload,
};
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use workspace_model::{Caller, RequestId, Timestamp};

/// How the fake central behaves for the next calls and flushes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FakeCentral {
    /// Every request gets the responder's reply.
    Answering,
    /// Frames leave the Mac but no answer comes back.
    AnswerLost,
    /// Nothing leaves the Mac; outbox items stay put.
    Unavailable,
    /// The Router has no enrollment; every submission is refused.
    NotEnrolled,
}

/// Central's answer for a request, chosen by the test.
pub type FakeResponder = Arc<dyn Fn(&Caller, &RequestPayload) -> ReplyPayload + Send + Sync>;

pub struct FakeLinkCalls {
    clock: Arc<dyn RuntimeClock>,
    responder: FakeResponder,
    state: Mutex<FakeState>,
}

struct FakeState {
    central: FakeCentral,
    last_flushed: Option<SyncTime>,
    requests: HashMap<RequestId, FakeRequest>,
}

struct FakeRequest {
    caller: Caller,
    payload: FakePayload,
    status: RequestStatus,
}

enum FakePayload {
    Immediate,
    Queued {
        write: Box<QueuedWrite>,
        created_at: Timestamp,
    },
}

impl FakeLinkCalls {
    pub fn new(clock: Arc<dyn RuntimeClock>, responder: FakeResponder) -> Self {
        Self {
            clock,
            responder,
            state: Mutex::new(FakeState {
                central: FakeCentral::Answering,
                last_flushed: None,
                requests: HashMap::new(),
            }),
        }
    }

    pub fn set_central(&self, central: FakeCentral) {
        self.lock().central = central;
    }

    /// Run the flush for the mark whose ±3 s window holds the clock's now:
    /// at most 64 due outbox writes are sent, oldest mark first; the rest wait
    /// for the next flush. Returns how many were sent.
    pub fn flush_on_mark(&self) -> usize {
        let now = self.clock.now();
        let Some(mark) = mark_in_window(now) else {
            return 0;
        };
        let mut state = self.lock();
        if state.last_flushed.is_some_and(|flushed| flushed >= mark)
            || matches!(
                state.central,
                FakeCentral::Unavailable | FakeCentral::NotEnrolled
            )
        {
            return 0;
        }
        state.last_flushed = Some(mark);
        let mut due: Vec<(SyncTime, RequestId)> = state
            .requests
            .iter()
            .filter_map(|(id, request)| match request.status {
                RequestStatus::InOutbox { flush_at } if flush_at <= mark => {
                    Some((flush_at, id.clone()))
                }
                _ => None,
            })
            .collect();
        due.sort();
        let central = state.central;
        let mut sent = 0;
        for (_, id) in due.into_iter().take(MAX_BATCH_WRITES) {
            let Some(request) = state.requests.get_mut(&id) else {
                continue;
            };
            let FakePayload::Queued { write, created_at } = &request.payload else {
                continue;
            };
            let wire = write.as_ref().clone().into_wire(*created_at);
            sent += 1;
            request.status = match central {
                FakeCentral::Answering => {
                    RequestStatus::Answered((self.responder)(&request.caller, &wire))
                }
                FakeCentral::AnswerLost | FakeCentral::Unavailable | FakeCentral::NotEnrolled => {
                    RequestStatus::Sent
                }
            };
        }
        sent
    }

    /// Central refuses a request it was sent or will be sent.
    pub fn refuse(&self, request: &RequestId, reason: crate::LinkAdmissionRefusal) {
        if let Some(entry) = self.lock().requests.get_mut(request) {
            entry.status = RequestStatus::Refused(reason);
        }
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn submit_now(&self, caller: Caller, payload: RequestPayload) -> Submission {
        if let Err(error) = self.validate(&caller, &payload) {
            return Submission::Refused(SubmitRefusal::InvalidRequest(error));
        }
        let now = self.clock.now();
        let mut state = self.lock();
        if state.central == FakeCentral::NotEnrolled {
            return Submission::Refused(SubmitRefusal::NotEnrolled);
        }
        let request_id = payload.write_request_id().cloned();
        match (submission_path(&payload), request_id) {
            (SubmissionPath::Outbox, Some(request_id)) => {
                let RequestPayload::Workspace(request) = payload else {
                    return Submission::Refused(SubmitRefusal::UnstampedMessageRequired);
                };
                queue_write(
                    &mut state,
                    caller,
                    QueuedWrite::Workspace(request),
                    request_id,
                    now,
                    self.clock.utc_now(),
                )
            }
            (SubmissionPath::Outbox, None) => {
                Submission::Refused(SubmitRefusal::InvalidRequest(request_id_policy()))
            }
            (SubmissionPath::Immediate, request_id) => {
                let central = state.central;
                immediate_submission(
                    &mut state,
                    central,
                    &self.responder,
                    caller,
                    payload,
                    request_id,
                )
            }
        }
    }

    fn validate(&self, caller: &Caller, payload: &RequestPayload) -> Result<(), ProtocolError> {
        let now = self.clock.now().unix_millis();
        let seconds = u64::try_from(now.div_euclid(1000)).unwrap_or(0);
        let nanos = u32::try_from(now.rem_euclid(1000) * 1_000_000).unwrap_or(0);
        let correlation =
            CorrelationId::mint(uuid::Timestamp::from_unix(uuid::NoContext, seconds, nanos));
        LinkRequest::new(correlation, caller.clone(), payload.clone()).map(|_| ())
    }
}

fn immediate_submission(
    state: &mut FakeState,
    central: FakeCentral,
    responder: &FakeResponder,
    caller: Caller,
    payload: RequestPayload,
    request_id: Option<RequestId>,
) -> Submission {
    match (central, request_id) {
        (FakeCentral::Unavailable | FakeCentral::NotEnrolled, _) => {
            Submission::Refused(SubmitRefusal::CentralUnavailable)
        }
        (FakeCentral::AnswerLost, None) => Submission::ReadAnswerLost,
        (FakeCentral::AnswerLost, Some(request_id)) => {
            let scope = payload.request_scope();
            state.requests.insert(
                request_id.clone(),
                FakeRequest {
                    caller,
                    payload: FakePayload::Immediate,
                    status: RequestStatus::Sent,
                },
            );
            Submission::Uncertain {
                request: request_id,
                scope,
            }
        }
        (FakeCentral::Answering, request_id) => {
            let reply = responder(&caller, &payload);
            if let Some(request_id) = request_id {
                state.requests.insert(
                    request_id,
                    FakeRequest {
                        caller,
                        payload: FakePayload::Immediate,
                        status: RequestStatus::Answered(reply.clone()),
                    },
                );
            }
            Submission::Answered(reply)
        }
    }
}

fn queue_write(
    state: &mut FakeState,
    caller: Caller,
    write: QueuedWrite,
    request_id: RequestId,
    now: SyncTime,
    created_at: Timestamp,
) -> Submission {
    if let Some(existing) = state.requests.get(&request_id) {
        return resubmitted_outbox_write(existing, &caller, &write, request_id);
    }
    let flush_at = earliest_open_mark(now, state.last_flushed);
    state.requests.insert(
        request_id.clone(),
        FakeRequest {
            caller,
            payload: FakePayload::Queued {
                write: Box::new(write),
                created_at,
            },
            status: RequestStatus::InOutbox { flush_at },
        },
    );
    Submission::InOutbox(OutboxReceipt {
        id: request_id,
        flush_at,
    })
}

/// Resubmitting the same id and content returns the first receipt; the same id
/// with different content or from another caller is refused.
fn resubmitted_outbox_write(
    existing: &FakeRequest,
    caller: &Caller,
    payload: &QueuedWrite,
    request_id: RequestId,
) -> Submission {
    if existing.caller != *caller
        || !matches!(&existing.payload, FakePayload::Queued { write, .. } if write.as_ref() == payload)
    {
        return Submission::Refused(SubmitRefusal::RequestIdConflict);
    }
    match existing.status {
        RequestStatus::InOutbox { flush_at } => Submission::InOutbox(OutboxReceipt {
            id: request_id,
            flush_at,
        }),
        RequestStatus::Answered(ref reply) => Submission::Answered(reply.clone()),
        RequestStatus::Sent
        | RequestStatus::Refused(_)
        | RequestStatus::Cancelled { .. }
        | RequestStatus::StatusUnavailable
        | RequestStatus::NotFound => Submission::Uncertain {
            request: request_id,
            scope: payload.request_scope(),
        },
    }
}

const fn request_id_policy() -> ProtocolError {
    ProtocolError::RequestIdPolicy { field: "requestId" }
}

impl LinkCalls for FakeLinkCalls {
    fn submit_message(
        &self,
        caller: Caller,
        message: ToolSendMessage,
    ) -> LinkCallFuture<'_, Submission> {
        let created_at = self.clock.utc_now();
        let now = SyncTime::from_unix_millis(created_at.timestamp_millis());
        let request_id = message.request_id.clone();
        let mut state = self.lock();
        let submission = if state.central == FakeCentral::NotEnrolled {
            Submission::Refused(SubmitRefusal::NotEnrolled)
        } else {
            queue_write(
                &mut state,
                caller,
                QueuedWrite::SendMessage(message),
                request_id,
                now,
                created_at,
            )
        };
        Box::pin(async move { submission })
    }

    fn submit(&self, caller: Caller, payload: RequestPayload) -> LinkCallFuture<'_, Submission> {
        let submission = self.submit_now(caller, payload);
        Box::pin(async move { submission })
    }

    fn request_status(
        &self,
        caller: Caller,
        request: RequestId,
    ) -> LinkCallFuture<'_, RequestStatus> {
        let status = self
            .lock()
            .requests
            .get(&request)
            .filter(|entry| entry.caller == caller)
            .map_or(RequestStatus::NotFound, |entry| entry.status.clone());
        Box::pin(async move { status })
    }
}

#[cfg(test)]
#[path = "fake_link_calls_test.rs"]
mod tests;
