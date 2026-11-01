//! Controlled issuer/adoption events exercise the lifetime owner against the memory repository.
use super::super::{
    device_code_flow::{
        DeviceCodeAuthorization, DevicePollResult, IssuedMachineLogin, IssuedMachineToken,
        IssuerFailure, MachineTokenIssuer, RefreshCredentialBundle,
    },
    issuer_connection::MachineIssuerConnection,
    login_contract::FleetLoginUnknown,
    login_publication::MachineLoginPublication,
    refresh_credential_store::RefreshCredentialStore,
    token_response::LearnedFleetCentral,
};
use super::*;
use codex_router_core::redaction::SecretString;
use codex_router_fleet_state::{
    fleet_login_repository::FleetLoginRepository, test_support::MemoryFleetLoginRepository,
};
use collaboration_contracts::{MachineLoginGate, MachineLoginState};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

const ORGANIZATION: &str = "019f0000-0000-7000-8000-000000000001";
const ENROLLMENT: &str = "019f0000-0000-7000-8000-000000000010";
const SIGN_IN: &str = "https://issuer.example.invalid/issuer";

struct EventClock(watch::Sender<i64>);
impl FleetLoginClock for EventClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(*self.0.borrow())
    }
    fn wait_until(&self, deadline: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        let mut time = self.0.subscribe();
        Box::pin(async move {
            while *time.borrow_and_update() < deadline.unix_millis() {
                time.changed().await.map_err(|_| ())?;
            }
            Ok(())
        })
    }
}
struct EventIssuer {
    authorizations: AtomicUsize,
    polls: AtomicUsize,
    refreshes: AtomicUsize,
    answers: Mutex<mpsc::UnboundedReceiver<Result<DevicePollResult, IssuerFailure>>>,
}
impl MachineTokenIssuer for EventIssuer {
    fn authorize<'a>(
        &'a self,
        _: &'a DeviceLoginRequest,
    ) -> LoginFuture<'a, Result<DeviceCodeAuthorization, IssuerFailure>> {
        Box::pin(async move {
            let attempt = self.authorizations.fetch_add(1, Ordering::SeqCst);
            Ok(DeviceCodeAuthorization {
                device_code: SecretString::new(format!("device-secret-canary-{attempt}")),
                user_code: "ABCD-EFGH"
                    .to_owned()
                    .try_into()
                    .map_err(|_| IssuerFailure::InvalidResponse)?,
                verification_uri: "https://issuer.example.invalid/activate"
                    .to_owned()
                    .try_into()
                    .map_err(|_| IssuerFailure::InvalidResponse)?,
                expires_at: SyncTime::from_unix_millis(600_000),
                interval_seconds: 5,
            })
        })
    }
    fn poll<'a>(
        &'a self,
        _: &'a SecretString,
    ) -> LoginFuture<'a, Result<DevicePollResult, IssuerFailure>> {
        Box::pin(async move {
            self.polls.fetch_add(1, Ordering::SeqCst);
            self.answers
                .lock()
                .await
                .recv()
                .await
                .unwrap_or(Err(IssuerFailure::Unavailable))
        })
    }
    fn refresh<'a>(
        &'a self,
        _: &'a RefreshCredentialBundle,
    ) -> LoginFuture<'a, Result<IssuedMachineToken, IssuerFailure>> {
        Box::pin(async move {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            match self
                .answers
                .lock()
                .await
                .recv()
                .await
                .unwrap_or(Err(IssuerFailure::Unavailable))?
            {
                DevicePollResult::Issued(issued) => Ok(issued.token),
                DevicePollResult::Pending | DevicePollResult::SlowDown => {
                    Err(IssuerFailure::InvalidResponse)
                }
            }
        })
    }
}
struct EventConnection {
    issuer: Arc<EventIssuer>,
    addresses: std::sync::Mutex<Vec<SignInAddress>>,
}
impl MachineIssuerConnection for EventConnection {
    fn connect<'a>(
        &'a self,
        address: &'a SignInAddress,
    ) -> LoginFuture<'a, Result<Arc<dyn MachineTokenIssuer>, IssuerFailure>> {
        Box::pin(async move {
            self.addresses
                .lock()
                .map_err(|_| IssuerFailure::Unavailable)?
                .push(address.clone());
            Ok(self.issuer.clone() as Arc<dyn MachineTokenIssuer>)
        })
    }
}
fn central() -> LearnedFleetCentral {
    serde_json::from_value(serde_json::json!({
        "receiver":"019f0000-0000-7000-8000-000000000011",
        "relay":"019f0000-0000-7000-8000-000000000012"
    }))
    .unwrap()
}
fn saved(enrollment: &str) -> SavedFleetLogin {
    SavedFleetLogin {
        sign_in: SIGN_IN.to_owned().try_into().unwrap(),
        organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
        enrollment_id: enrollment.to_owned().try_into().unwrap(),
        machine_environment: fleet_identity_protocol::MachineEnvironment::Physical,
        central: central().into(),
    }
}
struct Fixture {
    root: tempfile::TempDir,
    repository: Arc<MemoryFleetLoginRepository>,
    clock: Arc<EventClock>,
    issuer: Arc<EventIssuer>,
    connection: Arc<EventConnection>,
    answers: mpsc::UnboundedSender<Result<DevicePollResult, IssuerFailure>>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repository = Arc::new(MemoryFleetLoginRepository::new());
        let (time, _) = watch::channel(0);
        let (answers, receiver) = mpsc::unbounded_channel();
        let issuer = Arc::new(EventIssuer {
            authorizations: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
            refreshes: AtomicUsize::new(0),
            answers: Mutex::new(receiver),
        });
        let connection = Arc::new(EventConnection {
            issuer: issuer.clone(),
            addresses: std::sync::Mutex::new(Vec::new()),
        });
        Self {
            root,
            repository,
            clock: Arc::new(EventClock(time)),
            issuer,
            connection,
            answers,
        }
    }
    async fn start(&self) -> RunningMachineLogin {
        self.start_with(Arc::new(RefreshCredentialStore::MemoryOnly), None)
            .await
    }
    async fn start_with(
        &self,
        credentials: Arc<RefreshCredentialStore>,
        after_commit: Option<fn() -> Result<(), ()>>,
    ) -> RunningMachineLogin {
        RunningMachineLogin::start(
            MachineLoginIdentity {
                installation: "019f0000-0000-7000-8000-000000000002"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                peer: serde_json::from_value(serde_json::json!(
                    "019f0000-0000-7000-8000-000000000003"
                ))
                .unwrap(),
            },
            MachineLoginWork {
                issuer: self.connection.clone(),
                clock: self.clock.clone(),
                publication: Arc::new(MachineLoginPublication {
                    repository: self.repository.clone(),
                    credentials,
                    after_commit,
                }),
            },
            self.repository.load_fleet_login().await.unwrap(),
        )
    }
    fn observe(&self, running: &RunningMachineLogin) -> mpsc::Receiver<FleetLoginEvent> {
        running.handle.observe(
            Some(SIGN_IN.to_owned().try_into().unwrap()),
            ORGANIZATION.to_owned().try_into().unwrap(),
            "Fixture machine".to_owned().try_into().unwrap(),
            None,
        )
    }
    fn issue_with(&self, enrollment: &str, refresh: &str, expiry: i64) {
        self.answers
            .send(Ok(DevicePollResult::Issued(IssuedMachineLogin {
                token: IssuedMachineToken {
                    token: SecretString::new("access-secret-canary"),
                    valid_until: SyncTime::from_unix_millis(expiry),
                    credential: RefreshCredentialBundle {
                        machine_client_id: enrollment.to_owned().try_into().unwrap(),
                        refresh_credential: SecretString::new(refresh),
                    },
                },
                central: central(),
            })))
            .unwrap();
    }
    fn issue(&self) {
        self.issue_with(ENROLLMENT, "refresh-secret-canary", 300_000);
        self.clock
            .0
            .send_replace(self.clock.now().unix_millis() + 5_000);
    }
    async fn next(
        &self,
        running: &RunningMachineLogin,
        events: &mut mpsc::Receiver<FleetLoginEvent>,
    ) -> Option<FleetLoginEvent> {
        // This is a named runtime-adoption stand-in for core tests. The actual
        // process journey must exercise CollaborationLifecycle's real reassembly;
        // that journey is held until the real Fleet repository is available.
        let mut routes = running.handle.route_changes();
        loop {
            let route = routes.borrow_and_update().clone();
            if let MachineLoginRoute::Learned(saved) = route {
                running.handle.route_adopted(saved);
            }
            tokio::select! {
                event = events.recv() => return event,
                changed = routes.changed() => if changed.is_err() { return events.recv().await; },
            }
        }
    }
    async fn learned(&self, running: &RunningMachineLogin) -> Arc<SavedFleetLogin> {
        let mut routes = running.handle.route_changes();
        loop {
            if let MachineLoginRoute::Learned(saved) = routes.borrow_and_update().clone() {
                return saved;
            }
            routes.changed().await.unwrap();
        }
    }
}

#[tokio::test]
async fn fleet_machine_login_reattaches_code_but_waits_for_exact_runtime_route_adoption() {
    let fixture = Fixture::new();
    let running = fixture.start().await;
    let mut first = fixture.observe(&running);
    let code = first.recv().await.unwrap();
    let mut second = running.handle.observe(
        None,
        ORGANIZATION.to_owned().try_into().unwrap(),
        "Other CLI".to_owned().try_into().unwrap(),
        None,
    );
    assert_eq!(second.recv().await.unwrap(), code);
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 1);
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    fixture.issue();
    let route = fixture.learned(&running).await;
    assert_eq!(
        fixture.repository.load_fleet_login().await.unwrap(),
        Some((*route).clone())
    );
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    assert!(matches!(
        first.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    let mut wrong = (*route).clone();
    wrong.enrollment_id = "019f0000-0000-7000-8000-000000000099"
        .to_owned()
        .try_into()
        .unwrap();
    running.handle.route_adopted(Arc::new(wrong));
    running.handle.suspend_route().await;
    assert!(
        matches!(first.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
        "a stale adoption cannot emit a login terminal"
    );
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    running.handle.route_adopted(route);
    let terminal = first.recv().await.unwrap();
    assert!(matches!(
        terminal,
        FleetLoginEvent::LoggedIn {
            keeping: RefreshCredentialKeeping::MemoryOnly,
            ..
        }
    ));
    assert_eq!(second.recv().await.unwrap(), terminal);
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::LoggedIn
    );
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_empty_token_repository_stays_logged_out_and_never_persists_pending_login()
 {
    let fixture = Fixture::new();
    let running = fixture.start().await;
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    let mut missing = running.handle.observe(
        None,
        ORGANIZATION.to_owned().try_into().unwrap(),
        "No address".to_owned().try_into().unwrap(),
        None,
    );
    assert_eq!(
        missing.recv().await.unwrap(),
        FleetLoginEvent::Refused {
            reason: FleetLoginRefusal::NotConfigured {}
        }
    );
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    let mut pending = fixture.observe(&running);
    assert!(matches!(
        pending.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    running.shutdown().await.unwrap();
    assert!(pending.recv().await.is_none());
    assert!(
        fixture
            .repository
            .load_fleet_login()
            .await
            .unwrap()
            .is_none()
    );
    let restarted = fixture.start().await;
    assert_eq!(
        restarted.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    let mut fresh = fixture.observe(&restarted);
    assert!(matches!(
        fresh.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 0);
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_unknown_redemption_never_replays_or_opens_the_gate() {
    for failure in [IssuerFailure::Timeout, IssuerFailure::InvalidResponse] {
        let fixture = Fixture::new();
        let running = fixture.start().await;
        let mut events = fixture.observe(&running);
        assert!(matches!(
            events.recv().await,
            Some(FleetLoginEvent::Code { .. })
        ));
        fixture.answers.send(Err(failure)).unwrap();
        fixture.clock.0.send_replace(5_000);
        assert!(matches!(
            events.recv().await,
            Some(FleetLoginEvent::OutcomeUnknown {
                context: FleetLoginUnknown::DeviceRedemption { .. }
            })
        ));
        assert_eq!(
            running.handle.source.machine_login_state(),
            MachineLoginState::NotLoggedIn
        );
        assert!(
            fixture
                .repository
                .load_fleet_login()
                .await
                .unwrap()
                .is_none()
        );
        let mut fresh = fixture.observe(&running);
        assert!(matches!(
            fresh.recv().await,
            Some(FleetLoginEvent::Code { .. })
        ));
        assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);
        running.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn fleet_machine_login_unusable_store_refuses_before_discovery_or_credential_writes() {
    use codex_router_secret_store::classified_store_open::UnusableCause;
    for cause in [
        UnusableCause::KeychainUnavailable {},
        UnusableCause::InvalidCredentialData {},
        UnusableCause::MigrationIncomplete {},
    ] {
        let fixture = Fixture::new();
        let running = fixture
            .start_with(Arc::new(RefreshCredentialStore::Unusable { cause }), None)
            .await;
        let mut events = fixture.observe(&running);
        assert_eq!(
            events.recv().await,
            Some(FleetLoginEvent::Refused {
                reason: FleetLoginRefusal::StoreUnusable { cause }
            })
        );
        assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 0);
        assert!(fixture.connection.addresses.lock().unwrap().is_empty());
        assert_eq!(
            running.handle.source.machine_login_state(),
            MachineLoginState::NotLoggedIn
        );
        assert!(!fixture.root.path().join("secrets").exists());
        assert!(
            fixture
                .repository
                .load_fleet_login()
                .await
                .unwrap()
                .is_none()
        );
        running.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn fleet_machine_login_background_failure_requires_a_new_code_and_reuses_saved_address() {
    let fixture = Fixture::new();
    let running = fixture.start().await;
    let mut events = fixture.observe(&running);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    fixture.issue();
    assert!(matches!(
        fixture.next(&running, &mut events).await,
        Some(FleetLoginEvent::LoggedIn { .. })
    ));
    let mut token = running.handle.source.token.clone();
    token.borrow_and_update();
    fixture
        .answers
        .send(Err(IssuerFailure::InvalidGrant))
        .unwrap();
    fixture.clock.0.send_replace(255_000);
    token.changed().await.unwrap();
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    let mut fresh = running.handle.observe(
        None,
        ORGANIZATION.to_owned().try_into().unwrap(),
        "Reuse saved".to_owned().try_into().unwrap(),
        None,
    );
    assert!(matches!(
        fresh.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    assert_eq!(fixture.issuer.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .connection
            .addresses
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .as_str(),
        SIGN_IN
    );
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_disconnected_observer_does_not_cancel_host_owned_redemption() {
    let fixture = Fixture::new();
    let running = fixture.start().await;
    let mut events = fixture.observe(&running);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    drop(events);
    fixture.issue();
    let route = fixture.learned(&running).await;
    let mut changes = running.handle.source.token.clone();
    changes.borrow_and_update();
    running.handle.route_adopted(route);
    changes.changed().await.unwrap();
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::LoggedIn
    );
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);
    running.shutdown().await.unwrap();
}

#[path = "login_owner_storage_tests.rs"]
mod storage;

#[path = "login_owner_environment_tests.rs"]
mod environment;

#[path = "login_owner_trace_tests.rs"]
mod trace;
