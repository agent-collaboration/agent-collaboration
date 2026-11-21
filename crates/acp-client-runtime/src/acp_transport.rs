//! Ordered ACP output and the transport's authorization fence.

use crate::callback_router::CallbackRouter;
pub use crate::callback_router::{CallbackKind, CallbackResolution};
use crate::in_folder_allowance::{InFolderGrant, InFolderPermission};
pub use crate::transport_run::TransportRun;
use agent_client_protocol::schema::v1::PermissionOptionId;
use provider_session_machines::{
    SessionGeneration,
    session_lifecycle_machine::CloseAttempt,
    session_owner::{
        AcceptedRunSettings, AuthorizationTarget, DispatchAuthorization, DispatchResult,
        OwnerIncarnation, TurnMembership,
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
    folders: Option<InFolderGrant>,
    settings: Option<AcceptedRunSettings>,
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

impl<Frame> TransportState<Frame> {
    fn route_for_session(&self, session: &str) -> Option<TransportRun> {
        let mut matches = self
            .routes
            .iter()
            .filter(|(_, route)| route.session == session);
        let (run, _) = matches.next()?;
        matches.next().is_none().then_some(*run)
    }
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

    #[cfg(feature = "test-observation")]
    pub(crate) fn hold_ordered_capacity(&self) -> Vec<mpsc::OwnedPermit<OrderedOutput<Frame>>> {
        std::iter::from_fn(|| self.ordered.clone().try_reserve_owned().ok()).collect()
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
            folders: None,
            settings: None,
        });
        state.callbacks.open(run);
    }

    pub fn activate_route(&self, run: TransportRun) {
        self.state().callbacks.activate(run);
    }

    #[must_use]
    pub fn current_turn(&self, run: TransportRun) -> Option<TurnId> {
        self.state().callbacks.current_turn(run)
    }

    pub(crate) fn provider_started_turn(&self, run: TransportRun) -> Option<TurnId> {
        self.state().callbacks.provider_started_turn(run)
    }

    pub(crate) fn route_for_session(&self, session: &str) -> Option<TransportRun> {
        self.state().route_for_session(session)
    }

    pub(crate) fn session_for_route(&self, run: TransportRun) -> Option<String> {
        self.state()
            .routes
            .get(&run)
            .map(|route| route.session.clone())
    }

    pub(crate) fn routed_sessions(&self) -> Vec<String> {
        self.state()
            .routes
            .values()
            .map(|route| route.session.clone())
            .collect()
    }

    pub(crate) fn allows_explicit_tool_request(&self, session: &str, tool: &str) -> bool {
        let state = self.state();
        let Some(run) = state.route_for_session(session) else {
            return false;
        };
        state
            .callbacks
            .allows_explicit_tool_request(session, run, tool)
    }

    pub(crate) fn current_scope(&self, session: &str) -> Option<(TransportRun, TurnId)> {
        let state = self.state();
        let run = state.route_for_session(session)?;
        Some((run, state.callbacks.current_turn(run)?))
    }

    pub(crate) fn install_folders(&self, run: TransportRun, folders: InFolderGrant) {
        if let Some(route) = self.state().routes.get_mut(&run) {
            route.folders = Some(folders);
        }
    }

    pub(crate) fn accept_settings(&self, run: TransportRun, settings: AcceptedRunSettings) {
        if let Some(route) = self.state().routes.get_mut(&run) {
            route.settings = Some(settings);
        }
    }

    pub(crate) fn catalogue(
        &self,
        run: TransportRun,
    ) -> Option<provider_session_machines::ProviderSettingsCatalog> {
        let state = self.state();
        let settings = state.routes.get(&run)?.settings.as_ref()?;
        Some(settings.catalogue.as_ref().clone())
    }

    pub(crate) fn select_in_folder_permission(
        &self,
        session: &str,
        tool: &str,
        (run, turn): (TransportRun, TurnId),
        permission: InFolderPermission,
        auto: crate::AutoPermissionMode,
    ) -> Option<PermissionOptionId> {
        let state = self.state();
        let route = state.routes.get(&run)?;
        if route.closing.is_some()
            || state.route_for_session(session) != Some(run)
            || state.callbacks.current_turn(run) != Some(turn)
            || !state
                .callbacks
                .allows_explicit_tool_request(session, run, tool)
        {
            return None;
        }
        let settings = route.settings.as_ref()?;
        if !crate::provider_setting_application::current_mode_grants_auto(settings, auto) {
            return None;
        }
        route.folders.as_ref()?.select(permission)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn router_auto_grant_for_test(
        &self,
        run: TransportRun,
        auto: crate::AutoPermissionMode,
    ) -> Option<bool> {
        let state = self.state();
        let route = state.routes.get(&run)?;
        route.folders.as_ref()?;
        Some(
            crate::provider_setting_application::current_mode_grants_auto(
                route.settings.as_ref()?,
                auto,
            ),
        )
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
            TurnMembership::ObservedEligible { run, turn } => {
                state.callbacks.observe_eligible_turn(
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

    pub(crate) fn tool_in_current_turn(&self, run: TransportRun, tool: &str) -> bool {
        let state = self.state();
        state.routes.get(&run).is_some_and(|route| {
            state
                .callbacks
                .tool_in_current_turn(&route.session, run, tool)
        })
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
        self.take_when(authorization, || true)
    }

    pub(crate) fn take_when(
        &self,
        authorization: &DispatchAuthorization,
        still_requested: impl FnOnce() -> bool,
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
        if !still_requested() {
            state.revoke_slot(authorization);
            return Ok(DispatchResult::DefinitelyUnsent(authorization.clone()));
        }
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

    pub(crate) fn runs_of(&self, incarnation: OwnerIncarnation) -> Vec<SessionGeneration> {
        self.state()
            .routes
            .keys()
            .filter_map(|run| (run.incarnation == incarnation).then_some(run.generation))
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

    pub(crate) fn enqueue_control(
        &self,
        run: TransportRun,
        frame: Frame,
    ) -> Result<(), TransportRefusal> {
        let _ordered_with_authorizations = self.state();
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
