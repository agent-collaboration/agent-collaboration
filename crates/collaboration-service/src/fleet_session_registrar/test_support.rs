//! Event-controlled atomic store stand-in for focused real-client/Host/registrar integration.
//! It proves no SQLite durability or process-restart persistence.
use super::{
    FleetSessionRegistrationRepository, FleetSessionStorageFailure, FleetSessionStorageRequest,
    RegistrationInsert,
};
use codex_router_fleet_state::fleet_session_registration::{
    FleetSessionBindingLifecycle, FleetSessionRegistrationRecord, SessionBindingAddress,
};
use std::sync::{Arc, Mutex};

pub struct PendingSetupWriteControl {
    pub entered: tokio::sync::oneshot::Sender<()>,
    pub outcome: tokio::sync::oneshot::Receiver<Result<(), FleetSessionStorageFailure>>,
}

pub struct MemoryRegistrationRepository {
    pub pending_setup: Mutex<Option<PendingSetupWriteControl>>,
    pub records: Mutex<Vec<FleetSessionRegistrationRecord>>,
    pub events: Arc<Mutex<Vec<&'static str>>>,
}
impl MemoryRegistrationRepository {
    #[must_use]
    pub fn new(events: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            events,
            pending_setup: Mutex::new(None),
        }
    }

    fn select(
        &self,
        predicate: impl Fn(&FleetSessionRegistrationRecord) -> bool,
    ) -> Result<Option<FleetSessionRegistrationRecord>, FleetSessionStorageFailure> {
        Ok(self
            .records
            .lock()
            .map_err(|_| FleetSessionStorageFailure::Unavailable)?
            .iter()
            .find(|record| predicate(record))
            .cloned())
    }
}
impl FleetSessionRegistrationRepository for MemoryRegistrationRepository {
    fn operation<'a>(
        &'a self,
        operation: &'a collaboration_protocol::OperationId,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>> {
        Box::pin(
            async move { self.select(|record| &record.preparation().operation_id == operation) },
        )
    }
    fn conversation<'a>(
        &'a self,
        session: &'a message_board::SessionRef,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>> {
        Box::pin(async move { self.select(|record| record.lifecycle().session() == Some(session)) })
    }
    fn binding<'a>(
        &'a self,
        address: &'a SessionBindingAddress,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>> {
        Box::pin(
            async move { self.select(|record| &record.preparation().binding_address == address) },
        )
    }
    fn insert<'a>(
        &'a self,
        record: &'a FleetSessionRegistrationRecord,
    ) -> FleetSessionStorageRequest<'a, RegistrationInsert> {
        Box::pin(async move {
            let mut records = self
                .records
                .lock()
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?;
            if let Some(existing) = records.iter().find(|existing| {
                existing.preparation().operation_id == record.preparation().operation_id
            }) {
                return Ok(RegistrationInsert::Existing(Box::new(existing.clone())));
            }
            if records.iter().any(|existing| {
                existing.preparation().binding_address == record.preparation().binding_address
            }) {
                return Err(FleetSessionStorageFailure::Conflict);
            }
            records.push(record.clone());
            self.events
                .lock()
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?
                .push("preparedStored");
            Ok(RegistrationInsert::Inserted)
        })
    }
    fn replace<'a>(
        &'a self,
        expected: &'a FleetSessionRegistrationRecord,
        next: &'a FleetSessionRegistrationRecord,
    ) -> FleetSessionStorageRequest<'a, bool> {
        Box::pin(async move {
            if matches!(next.lifecycle(), FleetSessionBindingLifecycle::Prepared {
                creation: codex_router_fleet_state::fleet_session_registration::PreparedSessionCreation::CreatedPendingSetup { .. }
            }) {
                let control = { self.pending_setup.lock().map_err(|_| FleetSessionStorageFailure::Unavailable)?.take() };
                if let Some(control) = control {
                    let _entered = control.entered.send(());
                    control.outcome.await.map_err(|_| FleetSessionStorageFailure::Unavailable)??;
                }
            }
            let mut records = self
                .records
                .lock()
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?;
            let Some(current) = records.iter_mut().find(|record| {
                record.preparation().registration_id == expected.preparation().registration_id
            }) else {
                return Ok(false);
            };
            if &*current != expected || expected.preparation() != next.preparation() {
                return Ok(false);
            }
            *current = next.clone();
            self.events
                .lock()
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?
                .push("lifecycleStored");
            Ok(true)
        })
    }
    fn reconcile_candidates<'a>(
        &'a self,
    ) -> FleetSessionStorageRequest<'a, Vec<FleetSessionRegistrationRecord>> {
        Box::pin(async {
            Ok(self
                .records
                .lock()
                .map_err(|_| FleetSessionStorageFailure::Unavailable)?
                .iter()
                .filter(|record| {
                    matches!(
                        record.lifecycle(),
                        FleetSessionBindingLifecycle::Prepared { .. }
                            | FleetSessionBindingLifecycle::Active { .. }
                    )
                })
                .cloned()
                .collect())
        })
    }
}
