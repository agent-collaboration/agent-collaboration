//! Policy-only atomic repository and event-ordered authority; neither proves SQLite or a provider.
use super::*;
use codex_router_state::fleet_session_registration::*;
use fleet_identity_protocol::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

pub(super) const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000001";
pub(super) const PROJECT: &str = "019f0000-0000-7000-8000-000000000002";
pub(super) const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000003";
pub(super) const ORGANIZATION: &str = "019f0000-0000-7000-8000-000000000004";

pub(super) struct MemoryRepository {
    pub records: Mutex<Vec<FleetSessionRegistrationRecord>>,
    pub events: Arc<Mutex<Vec<&'static str>>>,
}
impl MemoryRepository {
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
impl FleetSessionRegistrationRepository for MemoryRepository {
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
        Box::pin(async move {
            self.select(|record| {
                record
                    .lifecycle()
                    .registration()
                    .is_some_and(|registration| &registration.session == session)
            })
        })
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

pub(super) struct Authority {
    pub check: Mutex<Result<SessionStartCheck, FleetSessionConnectionFailure>>,
    pub registrations: Mutex<Vec<Vec<u8>>>,
    pub events: Arc<Mutex<Vec<&'static str>>>,
    answers: AsyncMutex<
        mpsc::UnboundedReceiver<Result<RegisterFleetSessionResult, FleetSessionConnectionFailure>>,
    >,
}
impl FleetSessionConnection for Authority {
    fn check_start<'a>(
        &'a self,
        _: &'a CheckSessionStart,
    ) -> FleetSessionRequest<'a, SessionStartCheck> {
        Box::pin(async {
            self.events
                .lock()
                .map_err(|_| FleetSessionConnectionFailure::Unavailable)?
                .push("startChecked");
            self.check
                .lock()
                .map_err(|_| FleetSessionConnectionFailure::Unavailable)?
                .clone()
        })
    }
    fn register<'a>(
        &'a self,
        request: &'a RegisterFleetSession,
    ) -> FleetSessionRequest<'a, RegisterFleetSessionResult> {
        Box::pin(async move {
            self.registrations
                .lock()
                .map_err(|_| FleetSessionConnectionFailure::Unavailable)?
                .push(
                    serde_json::to_vec(request)
                        .map_err(|_| FleetSessionConnectionFailure::MalformedReply)?,
                );
            self.events
                .lock()
                .map_err(|_| FleetSessionConnectionFailure::Unavailable)?
                .push("registrationSent");
            self.answers
                .lock()
                .await
                .recv()
                .await
                .unwrap_or(Err(FleetSessionConnectionFailure::Unavailable))
        })
    }
    fn end<'a>(&'a self, _: &'a EndFleetSession) -> FleetSessionRequest<'a, ()> {
        Box::pin(async {
            self.events
                .lock()
                .map_err(|_| FleetSessionConnectionFailure::Unavailable)?
                .push("endSent");
            Ok(())
        })
    }
}

struct FixedIds(AtomicU64);
impl runtime_primitives::IdSource for FixedIds {
    fn new_id(&self) -> uuid::Uuid {
        uuid::Uuid::from_u128(
            0x019f0000000070008000000000000100 + u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
        )
    }
}
struct FixedEntropy;
impl SessionBindingEntropy for FixedEntropy {
    fn new_capability(&self) -> Result<[u8; 16], SessionBindingEntropyUnavailable> {
        Ok([0x35; 16])
    }
}
pub(super) struct Mode(pub Mutex<SessionRegistrationMode>);
impl SessionRegistrationModeSource for Mode {
    fn mode(&self) -> SessionRegistrationMode {
        *self.0.lock().unwrap()
    }
}
pub(super) struct Fixture {
    pub registrar: FleetSessionRegistrar,
    pub repository: Arc<MemoryRepository>,
    pub authority: Arc<Authority>,
    pub mode: Arc<Mode>,
    pub answers:
        mpsc::UnboundedSender<Result<RegisterFleetSessionResult, FleetSessionConnectionFailure>>,
}
impl Fixture {
    pub fn new() -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let repository = Arc::new(MemoryRepository {
            records: Mutex::new(Vec::new()),
            events: events.clone(),
        });
        let (answers, receiver) = mpsc::unbounded_channel();
        let authority = Arc::new(Authority {
            check: Mutex::new(Ok(SessionStartCheck::Allowed {
                workspace_id: WORKSPACE.to_owned().try_into().unwrap(),
            })),
            registrations: Mutex::new(Vec::new()),
            events,
            answers: AsyncMutex::new(receiver),
        });
        let mode = Arc::new(Mode(Mutex::new(SessionRegistrationMode::MachineTokens)));
        let registrar = Self::make_registrar(
            repository.clone(),
            authority.clone(),
            mode.clone(),
            "http://127.0.0.1:8788",
        );
        Self {
            registrar,
            repository,
            authority,
            mode,
            answers,
        }
    }
    pub fn make_registrar(
        repository: Arc<MemoryRepository>,
        authority: Arc<Authority>,
        mode: Arc<Mode>,
        origin: &str,
    ) -> FleetSessionRegistrar {
        FleetSessionRegistrar::new(FleetSessionRegistrarProps {
            installation: INSTALLATION.to_owned().try_into().unwrap(),
            listener: SessionBindingListener::Stable(origin.to_owned().try_into().unwrap()),
            repository,
            connection: authority,
            mode,
            ids: Arc::new(FixedIds(AtomicU64::new(0))),
            entropy: Arc::new(FixedEntropy),
        })
    }
    pub fn intent(&self) -> FleetSessionLaunch {
        FleetSessionLaunch {
            operation_id: serde_json::from_value(serde_json::json!(
                "019f0000-0000-7000-8000-000000000020"
            ))
            .unwrap(),
            endpoint: serde_json::from_value(
                serde_json::json!({"serviceId":INSTALLATION,"endpointId":"codex-local"}),
            )
            .unwrap(),
            project_id: Some(PROJECT.to_owned().try_into().unwrap()),
            started_by: StartedBy::RouterOperator {},
            isolation: AgentIsolation {
                kind: IsolationKind::Host,
                instance_id: "019f0000-0000-7000-8000-000000000021"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            },
            launch_context: serde_json::from_value(
                serde_json::json!({"workingDirectory":"/fixture/project"}),
            )
            .unwrap(),
            machine_environment: MachineEnvironment::Physical,
        }
    }
    pub fn ephemeral_registrar(&self) -> FleetSessionRegistrar {
        FleetSessionRegistrar::new(FleetSessionRegistrarProps {
            installation: INSTALLATION.to_owned().try_into().unwrap(),
            listener: SessionBindingListener::Ephemeral,
            repository: self.repository.clone(),
            connection: self.authority.clone(),
            mode: self.mode.clone(),
            ids: Arc::new(FixedIds(AtomicU64::new(0))),
            entropy: Arc::new(FixedEntropy),
        })
    }
    pub fn session(&self) -> message_board::SessionRef {
        serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},"sessionId":"provider-returned-conversation"})).unwrap()
    }
    pub fn grant(&self) {
        self.answers
            .send(Ok(RegisterFleetSessionResult::Registered {
                organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
                workspace_id: WORKSPACE.to_owned().try_into().unwrap(),
            }))
            .unwrap();
    }
}
