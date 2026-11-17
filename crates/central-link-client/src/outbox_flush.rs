//! The outbox flush as a pure reducer: `transition(state, event) -> (state,
//! effects)`. The Host owns the clock, the store and the socket: it runs each
//! effect and feeds back what happened. On each 1-minute mark (±3 s, the Host
//! picks the jitter) at most one batch of up to 64 due writes is sent; the rest
//! waits for the next flush. Nothing is sent before its mark, nothing in flight
//! or answered is sent again, and members a batch leaves unanswered return to
//! the outbox to be resent under the same RequestId, which central replays.
use crate::flush_marks::{earliest_open_mark, mark_in_window, next_mark_after};
use crate::link_calls::LinkAdmissionRefusal;
use central_link_protocol::ReplyPayload;
use runtime_primitives::SyncTime;
use std::collections::{BTreeMap, BTreeSet};
use workspace_model::RequestId;

/// The link batch frame's cap (Lead, 2026-11-15).
pub const MAX_BATCH_WRITES: usize = 64;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutboxFlushState {
    link: LinkAvailability,
    last_flushed: Option<SyncTime>,
    /// Writes in the outbox, ordered by the earliest mark they may send on.
    waiting: BTreeSet<(SyncTime, RequestId)>,
    in_flight: Option<InFlightBatch>,
    armed_wake: Option<SyncTime>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LinkAvailability {
    #[default]
    Disconnected,
    Connected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InFlightBatch {
    mark: SyncTime,
    /// Each unanswered member with the mark it was stored for.
    members: BTreeMap<RequestId, SyncTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlushEvent {
    /// The Host loaded or stored this write in the outbox.
    Stored {
        request: RequestId,
        flush_at: SyncTime,
    },
    /// The Host's wake fired, normally on a mark plus its jitter.
    Tick {
        now: SyncTime,
    },
    LinkConnected,
    /// Every unanswered member of the batch in flight returns to the outbox.
    LinkLost,
    /// Central answered one member of the batch in flight.
    MemberAnswered {
        request: RequestId,
        outcome: MemberOutcome,
    },
    /// The batch's deadline passed on a live link: unanswered members return.
    BatchExpired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemberOutcome {
    /// Central's whole answer, domain refusals included.
    Answered(ReplyPayload),
    /// Central never admitted the frame.
    Refused(LinkAdmissionRefusal),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlushEffect {
    /// Mark these writes sent and send them as one batch frame.
    SendBatch {
        mark: SyncTime,
        requests: Vec<RequestId>,
    },
    /// Persist central's answer for one write.
    RecordOutcome {
        request: RequestId,
        outcome: MemberOutcome,
    },
    /// These writes were not answered; they are back in the outbox.
    ReturnToOutbox { requests: Vec<RequestId> },
    /// Wake at this mark plus the Host's ±3 s jitter and send a Tick.
    WakeAt { mark: SyncTime },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FlushTransitionError {
    #[error("request {request} is already in the outbox or in flight")]
    AlreadyStored { request: RequestId },
    #[error("request {request} is not a member of the batch in flight")]
    NotInFlight { request: RequestId },
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
    pub fn is_held(&self, request: &RequestId) -> bool {
        self.waiting.iter().any(|(_, held)| held == request) || self.in_flight_contains(request)
    }

    #[must_use]
    pub fn in_flight_contains(&self, request: &RequestId) -> bool {
        self.in_flight
            .as_ref()
            .is_some_and(|batch| batch.members.contains_key(request))
    }

    pub fn waiting(&self) -> impl Iterator<Item = &(SyncTime, RequestId)> {
        self.waiting.iter()
    }

    pub fn in_flight_members(&self) -> impl Iterator<Item = &RequestId> {
        self.in_flight.iter().flat_map(|batch| batch.members.keys())
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
    match event {
        FlushEvent::Stored { request, flush_at } => {
            if state.is_held(&request) {
                return Err(FlushTransitionError::AlreadyStored { request });
            }
            state.waiting.insert((flush_at, request));
        }
        FlushEvent::Tick { now } => {
            tick_now = Some(now);
            state.armed_wake = None;
            if let Some(effect) = flush_on_mark(&mut state, now) {
                effects.push(effect);
            }
        }
        FlushEvent::LinkConnected => state.link = LinkAvailability::Connected,
        FlushEvent::LinkLost => {
            state.link = LinkAvailability::Disconnected;
            state.armed_wake = None;
            if let Some(effect) = return_unanswered(&mut state) {
                effects.push(effect);
            }
        }
        FlushEvent::MemberAnswered { request, outcome } => {
            let batch = state
                .in_flight
                .as_mut()
                .ok_or(FlushTransitionError::NoBatchInFlight)?;
            if batch.members.remove(&request).is_none() {
                return Err(FlushTransitionError::NotInFlight { request });
            }
            if batch.members.is_empty() {
                state.in_flight = None;
            }
            effects.push(FlushEffect::RecordOutcome { request, outcome });
        }
        FlushEvent::BatchExpired => {
            let effect =
                return_unanswered(&mut state).ok_or(FlushTransitionError::NoBatchInFlight)?;
            effects.push(effect);
        }
    }
    if let Some(mark) = rearm(&mut state, tick_now) {
        effects.push(FlushEffect::WakeAt { mark });
    }
    Ok((state, effects))
}

fn flush_on_mark(state: &mut OutboxFlushState, now: SyncTime) -> Option<FlushEffect> {
    let mark = mark_in_window(now)?;
    let open = state.last_flushed.is_none_or(|flushed| flushed < mark);
    let ready = state.link == LinkAvailability::Connected && state.in_flight.is_none();
    let has_due = state
        .waiting
        .first()
        .is_some_and(|(flush_at, _)| *flush_at <= mark);
    if !(open && ready && has_due) {
        return None;
    }
    let mut members = BTreeMap::new();
    while members.len() < MAX_BATCH_WRITES {
        match state.waiting.first() {
            Some((flush_at, _)) if *flush_at <= mark => {}
            Some(_) | None => break,
        }
        if let Some((flush_at, request)) = state.waiting.pop_first() {
            members.insert(request, flush_at);
        }
    }
    let requests = members.keys().cloned().collect();
    state.last_flushed = Some(mark);
    state.in_flight = Some(InFlightBatch { mark, members });
    Some(FlushEffect::SendBatch { mark, requests })
}

fn return_unanswered(state: &mut OutboxFlushState) -> Option<FlushEffect> {
    let batch = state.in_flight.take()?;
    let requests = batch.members.keys().cloned().collect();
    state.waiting.extend(
        batch
            .members
            .into_iter()
            .map(|(request, flush_at)| (flush_at, request)),
    );
    Some(FlushEffect::ReturnToOutbox { requests })
}

/// The next mark worth waking for, if it differs from the armed one. Nothing
/// is armed while the outbox is empty, the link is down, or a batch is in
/// flight; the event that ends each of those re-arms.
fn rearm(state: &mut OutboxFlushState, tick_now: Option<SyncTime>) -> Option<SyncTime> {
    let earliest_due = state.waiting.first().map(|(flush_at, _)| *flush_at);
    let target = match (state.link, &state.in_flight, earliest_due) {
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
