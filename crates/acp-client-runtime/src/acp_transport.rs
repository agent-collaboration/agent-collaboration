//! Ordered ACP output and the transport's authorization fence.

use crate::callback_router::CallbackRouter;
pub use crate::callback_router::{CallbackKind, CallbackResolution};
use provider_session_machines::{
    SessionGeneration,
    session_lifecycle_machine::CloseAttempt,
    session_owner::{
        AuthorizationTarget, DispatchAuthorization, DispatchResult, OwnerIncarnation,
        TurnMembership,
    },
    session_turn_machine::TurnId,
    tool_ownership::{ToolObservation, ToolOwnershipEffect},
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{Mutex, MutexGuard},
};
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TransportRun {
    pub incarnation: OwnerIncarnation,
    pub generation: SessionGeneration,
}

impl TransportRun {
    fn of(authorization: &DispatchAuthorization) -> Self {
        Self {
            incarnation: authorization.incarnation,
            generation: authorization.run,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotEvidence {
    Offered,
    Taken,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TransportRefusal {
    #[error("the exact ACP route is unavailable")]
    RouteUnavailable,
    #[error("the ACP route is already closing")]
    Closing,
    #[error("the ACP authorization does not match the slot")]
    AuthorizationMismatch,
    #[error("the ACP writer queue is full")]
    WriterFull,
    #[error("the ACP writer stopped")]
    WriterStopped,
}

#[derive(Debug)]
pub struct OrderedOutput<Frame> {
    pub run: TransportRun,
    pub frame: Frame,
}

struct ConnectionRoute {
    session: String,
    closing: Option<CloseAttempt>,
}

enum AuthorizationSlot<Frame> {
    Offered {
        frame: Frame,
        new_turn: Option<TurnId>,
    },
    Taken,
    Revoked,
}

impl<Frame> AuthorizationSlot<Frame> {
    fn evidence(&self) -> SlotEvidence {
        match self {
            Self::Offered { .. } => SlotEvidence::Offered,
            Self::Taken => SlotEvidence::Taken,
            Self::Revoked => SlotEvidence::Revoked,
        }
    }

    fn revoke(&mut self) -> Option<TurnId> {
        if matches!(self, Self::Taken) {
            return None;
        }
        match std::mem::replace(self, Self::Revoked) {
            Self::Offered { new_turn, .. } => new_turn,
            _ => None,
        }
    }
}

struct TransportState<Frame> {
    routes: BTreeMap<TransportRun, ConnectionRoute>,
    slot: BTreeMap<DispatchAuthorization, AuthorizationSlot<Frame>>,
    callbacks: CallbackRouter,
}

/// One instance and output receiver per ACP connection. The same short lock
/// protects route, slot, callback membership and enqueue. It never spans I/O.
pub struct AcpTransport<Frame> {
    state: Mutex<TransportState<Frame>>,
    ordered: mpsc::Sender<OrderedOutput<Frame>>,
}

impl<Frame> AcpTransport<Frame> {
    pub fn new(capacity: NonZeroUsize) -> (Self, mpsc::Receiver<OrderedOutput<Frame>>) {
        let (ordered, output) = mpsc::channel(capacity.get());
        let state = Mutex::new(TransportState {
            routes: BTreeMap::new(),
            slot: BTreeMap::new(),
            callbacks: CallbackRouter::default(),
        });
        (Self { state, ordered }, output)
    }

    fn state(&self) -> MutexGuard<'_, TransportState<Frame>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn open_route(&self, run: TransportRun, session: String) {
        let mut state = self.state();
        state.routes.entry(run).or_insert(ConnectionRoute {
            session,
            closing: None,
        });
        state.callbacks.open(run);
    }

    pub fn activate_route(&self, run: TransportRun) {
        self.state().callbacks.activate(run);
    }

    pub fn apply_membership(&self, incarnation: OwnerIncarnation, change: TurnMembership) {
        let mut state = self.state();
        match change {
            TurnMembership::Provisional { run, turn } => {
                state.callbacks.begin_turn(
                    TransportRun {
                        incarnation,
                        generation: run,
                    },
                    turn,
                );
            }
            TurnMembership::RemoveOnRevoke { run, turn } => {
                state.callbacks.revoke_new_turn(
                    TransportRun {
                        incarnation,
                        generation: run,
                    },
                    turn,
                );
            }
            TurnMembership::End { run, turn } => {
                let run = TransportRun {
                    incarnation,
                    generation: run,
                };
                if let Some(session) = state.routes.get(&run).map(|route| route.session.clone()) {
                    state.callbacks.end_turn(&session, run, turn);
                }
            }
            TurnMembership::Retire(generation) => {
                let run = TransportRun {
                    incarnation,
                    generation,
                };
                if let Some(route) = state.routes.remove(&run) {
                    state.callbacks.retire(&route.session, run);
                }
            }
        }
    }

    pub fn observe_tool(
        &self,
        run: TransportRun,
        tool_id: &str,
        observation: ToolObservation,
    ) -> ToolOwnershipEffect {
        let mut state = self.state();
        let Some(session) = state.routes.get(&run).map(|route| route.session.clone()) else {
            return ToolOwnershipEffect::None;
        };
        state.callbacks.observe(&session, run, tool_id, observation)
    }

    pub fn resolve_callback(&self, tool_id: &str, kind: CallbackKind) -> CallbackResolution {
        self.state().callbacks.resolve(tool_id, kind)
    }

    pub fn offer(
        &self,
        authorization: DispatchAuthorization,
        new_turn: Option<TurnId>,
        frame: Frame,
    ) -> Result<(), TransportRefusal> {
        let mut state = self.state();
        let run = TransportRun::of(&authorization);
        let route = state
            .routes
            .get(&run)
            .ok_or(TransportRefusal::RouteUnavailable)?;
        if route.closing.is_some() {
            return Err(TransportRefusal::Closing);
        }
        if (authorization.target == AuthorizationTarget::NewTurn) != new_turn.is_some()
            || state.slot.iter().any(|(key, slot)| {
                TransportRun::of(key) == run
                    && key.input_id == authorization.input_id
                    && (slot.evidence() == SlotEvidence::Offered
                        || key.authorization_seq == authorization.authorization_seq)
            })
        {
            return Err(TransportRefusal::AuthorizationMismatch);
        }
        state.slot.insert(
            authorization,
            AuthorizationSlot::Offered { frame, new_turn },
        );
        Ok(())
    }

    /// Only the exact live token can enqueue. A full writer leaves the offer
    /// available for the same token until revoke or the owner's deadline.
    pub fn take(
        &self,
        authorization: &DispatchAuthorization,
    ) -> Result<DispatchResult, TransportRefusal> {
        let mut state = self.state();
        let slot = state
            .slot
            .get(authorization)
            .ok_or(TransportRefusal::AuthorizationMismatch)?;
        let new_turn = match slot {
            AuthorizationSlot::Taken => {
                return Ok(DispatchResult::AlreadyTaken(authorization.clone()));
            }
            AuthorizationSlot::Revoked => {
                return Ok(DispatchResult::DefinitelyUnsent(authorization.clone()));
            }
            AuthorizationSlot::Offered { new_turn, .. } => *new_turn,
        };
        let run = TransportRun::of(authorization);
        if state
            .routes
            .get(&run)
            .is_none_or(|route| route.closing.is_some())
        {
            state.revoke_slot(authorization);
            return Ok(DispatchResult::DefinitelyUnsent(authorization.clone()));
        }
        if !authorization.matches_route(run.generation, state.callbacks.current_turn(run))
            || new_turn.is_some_and(|turn| !state.callbacks.can_take_new_turn(run, turn))
        {
            state.revoke_slot(authorization);
            return Ok(DispatchResult::StaleTarget(authorization.clone()));
        }
        let permit = self.ordered.try_reserve().map_err(writer_refusal)?;
        let Some(AuthorizationSlot::Offered { frame, .. }) = state
            .slot
            .insert(authorization.clone(), AuthorizationSlot::Taken)
        else {
            return Err(TransportRefusal::AuthorizationMismatch);
        };
        if let Some(turn) = new_turn {
            state.callbacks.take_new_turn(run, turn);
        }
        permit.send(OrderedOutput { run, frame });
        Ok(DispatchResult::Taken(authorization.clone()))
    }

    pub fn revoke(
        &self,
        authorization: &DispatchAuthorization,
    ) -> Result<DispatchResult, TransportRefusal> {
        self.state()
            .revoke_slot(authorization)
            .ok_or(TransportRefusal::AuthorizationMismatch)
    }

    /// Revokes pending work while preserving proof of earlier takes, including
    /// takes belonging to a route which has already retired.
    pub fn revoke_incarnation(
        &self,
        incarnation: OwnerIncarnation,
    ) -> Vec<(DispatchAuthorization, SlotEvidence)> {
        let mut state = self.state();
        let TransportState {
            slot, callbacks, ..
        } = &mut *state;
        slot.iter_mut()
            .filter(|(key, _)| key.incarnation == incarnation)
            .map(|(key, entry)| {
                if let Some(turn) = entry.revoke() {
                    callbacks.revoke_new_turn(TransportRun::of(key), turn);
                }
                (key.clone(), entry.evidence())
            })
            .collect()
    }

    /// Called after terminal settlement no longer needs the slot's evidence.
    pub fn forget(&self, authorization: &DispatchAuthorization) {
        self.state().slot.remove(authorization);
    }

    pub fn enqueue_request(&self, run: TransportRun, frame: Frame) -> Result<(), TransportRefusal> {
        let state = self.state();
        let route = state
            .routes
            .get(&run)
            .ok_or(TransportRefusal::RouteUnavailable)?;
        if route.closing.is_some() {
            return Err(TransportRefusal::Closing);
        }
        self.enqueue(run, frame)
    }

    pub fn enqueue_close(
        &self,
        run: TransportRun,
        attempt: CloseAttempt,
        frame: Frame,
    ) -> Result<(), TransportRefusal> {
        let mut state = self.state();
        let route = state
            .routes
            .get_mut(&run)
            .ok_or(TransportRefusal::RouteUnavailable)?;
        if route.closing.is_some() {
            return Err(TransportRefusal::Closing);
        }
        self.enqueue(run, frame)?;
        route.closing = Some(attempt);
        Ok(())
    }

    pub fn reopen_route(&self, run: TransportRun, attempt: CloseAttempt) {
        if let Some(route) = self.state().routes.get_mut(&run)
            && route.closing == Some(attempt)
        {
            route.closing = None;
        }
    }

    fn enqueue(&self, run: TransportRun, frame: Frame) -> Result<(), TransportRefusal> {
        self.ordered
            .try_send(OrderedOutput { run, frame })
            .map_err(writer_refusal)
    }
}

fn writer_refusal<T>(error: mpsc::error::TrySendError<T>) -> TransportRefusal {
    match error {
        mpsc::error::TrySendError::Full(_) => TransportRefusal::WriterFull,
        mpsc::error::TrySendError::Closed(_) => TransportRefusal::WriterStopped,
    }
}

impl<Frame> TransportState<Frame> {
    fn revoke_slot(&mut self, key: &DispatchAuthorization) -> Option<DispatchResult> {
        let slot = self.slot.get_mut(key)?;
        if let Some(turn) = slot.revoke() {
            self.callbacks.revoke_new_turn(TransportRun::of(key), turn);
        }
        Some(if matches!(slot, AuthorizationSlot::Taken) {
            DispatchResult::AlreadyTaken(key.clone())
        } else {
            DispatchResult::Revoked(key.clone())
        })
    }
}

#[cfg(test)]
#[path = "acp_transport_test.rs"]
mod tests;
