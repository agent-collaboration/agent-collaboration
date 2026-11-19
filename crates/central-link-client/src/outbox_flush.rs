//! When the outbox flushes, as a pure reducer: `transition(state, event) ->
//! (state, effects)`. The Mac outbox table is the only truth for what waits,
//! is sent and is answered (Lead A, 2026-11-18: one owner per truth); this
//! reducer holds no request ids. It decides only WHEN a batch may go: on a
//! 1-minute mark (±3 s, the Host picks the jitter), at most one batch in
//! flight, only on a connected link. On SendBatch the Host claims the next due
//! rows from the store; a store error is not a link event, and the next mark
//! retries from the store.
use crate::flush_marks::{earliest_open_mark, mark_in_window, next_mark_after};
use crate::link_calls::LinkAdmissionRefusal;
use central_link_protocol::ReplyPayload;
use runtime_primitives::SyncTime;
use workspace_model::{
    RefusalReason, WorkspaceResult,
    central_mail::{CentralMailRefusal, CentralMailResult},
};

/// The link batch frame's cap (Lead A, 2026-11-15).
pub const MAX_BATCH_WRITES: usize = 64;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutboxFlushState {
    link: LinkAvailability,
    last_flushed: Option<SyncTime>,
    /// The mark of the batch on the wire, if any.
    in_flight: Option<SyncTime>,
    armed_wake: Option<SyncTime>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LinkAvailability {
    #[default]
    Disconnected,
    Connected,
}

/// `earliest_due` is the store's earliest receipt mark among waiting writes,
/// read by the Host just before the event; None when nothing waits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlushEvent {
    /// The Host stored a write for this mark.
    Stored {
        flush_at: SyncTime,
    },
    /// The Host's wake fired, normally on a mark plus its jitter.
    Tick {
        now: SyncTime,
        earliest_due: Option<SyncTime>,
    },
    LinkConnected {
        earliest_due: Option<SyncTime>,
    },
    LinkLost,
    /// The batch for this flush ended: answered, returned to the store, or
    /// never sent because the claim found nothing or the store failed.
    BatchEnded {
        earliest_due: Option<SyncTime>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemberOutcome {
    /// Central's whole answer, domain refusals included.
    Answered(ReplyPayload),
    /// Central never admitted the frame.
    Refused(LinkAdmissionRefusal),
}

impl MemberOutcome {
    /// Whether central applied or decided this write. A MachineRevoked refusal
    /// (the machine is revoked, unknown, or its credential lapsed) is link
    /// admission, not a domain answer: central did not apply the write, so it
    /// returns to the outbox and is resent after a fresh-token reconnect
    /// (Lead A, 2026-11-17). Only domain refusals inside a reply are final.
    #[must_use]
    pub fn is_final(&self) -> bool {
        match self {
            Self::Answered(ReplyPayload::Workspace(reply)) => {
                reply.result != WorkspaceResult::Refused(RefusalReason::MachineRevoked)
            }
            Self::Answered(ReplyPayload::Mail(result)) => {
                *result
                    != CentralMailResult::Refused(CentralMailRefusal::Workspace(
                        RefusalReason::MachineRevoked,
                    ))
            }
            Self::Refused(
                LinkAdmissionRefusal::CredentialRefused | LinkAdmissionRefusal::MachineRevoked,
            ) => false,
            Self::Refused(LinkAdmissionRefusal::RequestIdConflict) => true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlushEffect {
    /// Claim the next due writes (up to MAX_BATCH_WRITES) from the store and
    /// send them as one batch frame.
    SendBatch { mark: SyncTime },
    /// Wake at this mark plus the Host's ±3 s jitter and send a Tick.
    WakeAt { mark: SyncTime },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FlushTransitionError {
    #[error("no batch is in flight")]
    NoBatchInFlight,
}

impl OutboxFlushState {
    /// The mark a write stored at `now` is promised on its receipt.
    #[must_use]
    pub fn receipt_mark(&self, now: SyncTime) -> SyncTime {
        earliest_open_mark(now, self.last_flushed)
    }

    #[must_use]
    pub const fn batch_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    #[must_use]
    pub const fn last_flushed(&self) -> Option<SyncTime> {
        self.last_flushed
    }
}

pub fn transition(
    mut state: OutboxFlushState,
    event: FlushEvent,
) -> Result<(OutboxFlushState, Vec<FlushEffect>), FlushTransitionError> {
    let mut effects = Vec::new();
    let mut tick_now = None;
    let earliest_due = match event {
        FlushEvent::Stored { flush_at } => {
            // Something waits at least this early; keep an earlier armed wake.
            Some(
                state
                    .armed_wake
                    .map_or(flush_at, |armed| armed.min(flush_at)),
            )
        }
        FlushEvent::Tick { now, earliest_due } => {
            tick_now = Some(now);
            state.armed_wake = None;
            if let Some(mark) = flush_on_mark(&mut state, now, earliest_due) {
                effects.push(FlushEffect::SendBatch { mark });
            }
            earliest_due
        }
        FlushEvent::LinkConnected { earliest_due } => {
            state.link = LinkAvailability::Connected;
            earliest_due
        }
        FlushEvent::LinkLost => {
            state.link = LinkAvailability::Disconnected;
            state.in_flight = None;
            state.armed_wake = None;
            None
        }
        FlushEvent::BatchEnded { earliest_due } => {
            state
                .in_flight
                .take()
                .ok_or(FlushTransitionError::NoBatchInFlight)?;
            earliest_due
        }
    };
    if let Some(mark) = rearm(&mut state, earliest_due, tick_now) {
        effects.push(FlushEffect::WakeAt { mark });
    }
    Ok((state, effects))
}

fn flush_on_mark(
    state: &mut OutboxFlushState,
    now: SyncTime,
    earliest_due: Option<SyncTime>,
) -> Option<SyncTime> {
    let mark = mark_in_window(now)?;
    let open = state.last_flushed.is_none_or(|flushed| flushed < mark);
    let ready = state.link == LinkAvailability::Connected && state.in_flight.is_none();
    let has_due = earliest_due.is_some_and(|due| due <= mark);
    if !(open && ready && has_due) {
        return None;
    }
    state.last_flushed = Some(mark);
    state.in_flight = Some(mark);
    Some(mark)
}

/// The next mark worth waking for, if it differs from the armed one. Nothing
/// is armed while the link is down, a batch is in flight or nothing waits;
/// the event that ends each of those re-arms.
fn rearm(
    state: &mut OutboxFlushState,
    earliest_due: Option<SyncTime>,
    tick_now: Option<SyncTime>,
) -> Option<SyncTime> {
    let target = match (state.link, state.in_flight, earliest_due) {
        (LinkAvailability::Connected, None, Some(earliest_due)) => {
            let after_last = state.last_flushed.map(next_mark_after);
            let still_open = tick_now.map(|now| earliest_open_mark(now, state.last_flushed));
            [Some(earliest_due), after_last, still_open]
                .into_iter()
                .flatten()
                .max()
        }
        (LinkAvailability::Disconnected, _, _) | (_, Some(_), _) | (_, None, None) => None,
    };
    if target == state.armed_wake {
        return None;
    }
    state.armed_wake = target;
    target
}

#[cfg(test)]
#[path = "outbox_flush_test.rs"]
mod tests;
