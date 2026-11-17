//! Dev-only stand-in for the link below the seam. It honours the frozen
//! contract so tools can be built before the real link lands: outbox kinds
//! answer InOutbox and become Accepted after a flush on their mark; immediate
//! kinds answer, or are Uncertain when the answer is lost; another caller's
//! request id answers NotFound. It proves nothing about the real link.
use crate::flush_marks::{earliest_open_mark, mark_in_window};
use crate::link_calls::{
    LinkCallFuture, LinkCalls, OutboxReceipt, RequestStatus, Submission, SubmitRefusal,
};
use crate::outbox_flush::MAX_BATCH_WRITES;
use crate::submission_path::{SubmissionPath, submission_path};
use central_link_protocol::{
    CorrelationId, LinkRequest, ProtocolError, ReplyPayload, RequestPayload,
};
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use workspace_model::{Caller, RefusalReason, RequestId};

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
    payload: RequestPayload,
    status: RequestStatus,
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
            sent += 1;
            request.status = match central {
                FakeCentral::Answering => {
                    RequestStatus::Accepted((self.responder)(&request.caller, &request.payload))
                }
                FakeCentral::AnswerLost | FakeCentral::Unavailable | FakeCentral::NotEnrolled => {
                    RequestStatus::Sent
                }
            };
        }
        sent
    }

    /// Central refuses a request it was sent or will be sent.
    pub fn refuse(&self, request: &RequestId, reason: RefusalReason) {
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
                if let Some(existing) = state.requests.get(&request_id) {
                    return resubmitted_outbox_write(existing, &caller, &payload, request_id);
                }
                let flush_at = earliest_open_mark(now, state.last_flushed);
                state.requests.insert(
                    request_id.clone(),
                    FakeRequest {
                        caller,
                        payload,
                        status: RequestStatus::InOutbox { flush_at },
                    },
                );
                Submission::InOutbox(OutboxReceipt {
                    id: request_id,
                    flush_at,
                })
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
        (FakeCentral::Unavailable | FakeCentral::NotEnrolled, _)
        | (FakeCentral::AnswerLost, None) => Submission::Refused(SubmitRefusal::CentralUnavailable),
        (FakeCentral::AnswerLost, Some(request_id)) => {
            state.requests.insert(
                request_id.clone(),
                FakeRequest {
                    caller,
                    payload,
                    status: RequestStatus::Sent,
                },
            );
            Submission::Uncertain {
                request: request_id,
            }
        }
        (FakeCentral::Answering, request_id) => {
            let reply = responder(&caller, &payload);
            if let Some(request_id) = request_id {
                state.requests.insert(
                    request_id,
                    FakeRequest {
                        caller,
                        payload,
                        status: RequestStatus::Accepted(reply.clone()),
                    },
                );
            }
            Submission::Answered(reply)
        }
    }
}

/// Resubmitting the same id and content returns the first receipt; the same id
/// with different content or from another caller is refused.
fn resubmitted_outbox_write(
    existing: &FakeRequest,
    caller: &Caller,
    payload: &RequestPayload,
    request_id: RequestId,
) -> Submission {
    if existing.caller != *caller || existing.payload != *payload {
        return Submission::Refused(SubmitRefusal::RequestIdConflict);
    }
    match existing.status {
        RequestStatus::InOutbox { flush_at } => Submission::InOutbox(OutboxReceipt {
            id: request_id,
            flush_at,
        }),
        RequestStatus::Accepted(ref reply) => Submission::Answered(reply.clone()),
        RequestStatus::Sent | RequestStatus::Refused(_) | RequestStatus::NotFound => {
            Submission::Uncertain {
                request: request_id,
            }
        }
    }
}

const fn request_id_policy() -> ProtocolError {
    ProtocolError::RequestIdPolicy { field: "requestId" }
}

impl LinkCalls for FakeLinkCalls {
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
