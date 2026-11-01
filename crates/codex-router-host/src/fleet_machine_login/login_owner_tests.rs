//! Controlled events exercise observer attachment, terminal publication and the real gate.
use std::sync::atomic::{AtomicUsize, Ordering};

use codex_router_core::redaction::SecretString;
use collaboration_contracts::{MachineLoginGate, MachineLoginState};
use tokio::sync::Mutex;

use super::super::{
    device_code_flow::{DeviceCodeAuthorization, DevicePollResult, IssuerFailure},
    login_contract::FleetLoginUnknown,
    login_files::read_metadata,
    login_publication::sync_directory,
    refresh_credential_store::RefreshCredentialStore,
};
use super::*;

const ORGANIZATION: &str = "019f0000-0000-7000-8000-000000000001";
const ENROLLMENT: &str = "019f0000-0000-7000-8000-000000000010";

struct EventClock(watch::Sender<i64>);
impl FleetLoginClock for EventClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(*self.0.borrow())
    }
    fn wait_until(&self, deadline: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        let mut time = self.0.subscribe();
        Box::pin(async move {
            loop {
                if *time.borrow_and_update() >= deadline.unix_millis() {
                    return Ok(());
                }
                time.changed().await.map_err(|_| ())?;
            }
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
                user_code: "ABCD-EFGH".to_owned().try_into().unwrap(),
                verification_uri: "https://sign-in.example.invalid/activate"
                    .to_owned()
                    .try_into()
                    .unwrap(),
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
                DevicePollResult::Issued(issued) => Ok(issued),
                DevicePollResult::Pending | DevicePollResult::SlowDown => {
                    Err(IssuerFailure::InvalidResponse)
                }
            }
        })
    }
}

struct Fixture {
    root: tempfile::TempDir,
    clock: Arc<EventClock>,
    issuer: Arc<EventIssuer>,
    answers: mpsc::UnboundedSender<Result<DevicePollResult, IssuerFailure>>,
}
impl Fixture {
    fn new() -> Self {
        let (time, _) = watch::channel(0);
        let (answers, receiver) = mpsc::unbounded_channel();
        Self {
            root: tempfile::tempdir().unwrap(),
            clock: Arc::new(EventClock(time)),
            issuer: Arc::new(EventIssuer {
                authorizations: AtomicUsize::new(0),
                polls: AtomicUsize::new(0),
                refreshes: AtomicUsize::new(0),
                answers: Mutex::new(receiver),
            }),
            answers,
        }
    }
    fn start(&self, sync: fn(&std::path::Path) -> std::io::Result<()>) -> RunningMachineLogin {
        self.start_with_credentials(sync, Arc::new(RefreshCredentialStore::MemoryOnly))
    }
    fn start_with_credentials(
        &self,
        sync: fn(&std::path::Path) -> std::io::Result<()>,
        credentials: Arc<RefreshCredentialStore>,
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
            self.issuer.clone(),
            self.clock.clone(),
            Arc::new(MachineLoginPublication {
                root: self.root.path().to_owned(),
                credentials,
                sync_directory: sync,
            }),
            read_metadata(self.root.path()).unwrap(),
        )
    }
    fn observe(&self, running: &RunningMachineLogin) -> mpsc::Receiver<FleetLoginEvent> {
        running.handle.observe(
            ORGANIZATION.to_owned().try_into().unwrap(),
            "Fixture machine".to_owned().try_into().unwrap(),
        )
    }
    fn issue(&self) {
        self.answers
            .send(Ok(DevicePollResult::Issued(IssuedMachineToken {
                token: SecretString::new("access-secret-canary"),
                valid_until: SyncTime::from_unix_millis(300_000),
                credential: RefreshCredentialBundle {
                    machine_client_id: ENROLLMENT.to_owned().try_into().unwrap(),
                    refresh_credential: SecretString::new("refresh-secret-canary"),
                },
            })))
            .unwrap();
        self.clock.0.send_replace(5_000);
    }
}

#[tokio::test]
async fn fleet_machine_login_reattaches_the_same_code_and_only_opens_gate_after_confirmed_publication()
 {
    let fixture = Fixture::new();
    let running = fixture.start(sync_directory);
    let mut first = fixture.observe(&running);
    let code = first.recv().await.unwrap();
    assert!(matches!(code, FleetLoginEvent::Code { .. }));
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    let mut second = fixture.observe(&running);
    assert_eq!(second.recv().await.unwrap(), code);
    let mut attached = running.handle.observe(
        "019f0000-0000-7000-8000-000000000099"
            .to_owned()
            .try_into()
            .unwrap(),
        "Other CLI parameters".to_owned().try_into().unwrap(),
    );
    assert_eq!(
        attached.recv().await.unwrap(),
        code,
        "an unsaved pending request is reattached, never treated as a saved organization"
    );
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 1);

    fixture.issue();

    let terminal = first.recv().await.unwrap();
    assert!(matches!(
        terminal,
        FleetLoginEvent::LoggedIn {
            keeping: RefreshCredentialKeeping::MemoryOnly,
            ..
        }
    ));
    assert_eq!(second.recv().await.unwrap(), terminal);
    assert_eq!(attached.recv().await.unwrap(), terminal);
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::LoggedIn
    );
    let metadata = read_metadata(fixture.root.path()).unwrap().unwrap();
    assert_eq!(metadata.enrollment_id.as_str(), ENROLLMENT);
    let files: Vec<_> = std::fs::read_dir(fixture.root.path().join("fleet"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let bytes = std::fs::read(&files[0]).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for secret in [
        "access-secret-canary",
        "refresh-secret-canary",
        "device-secret-canary",
    ] {
        assert!(!text.contains(secret));
    }
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_unknown_redemption_never_replays_or_opens_gate() {
    for failure in [IssuerFailure::Timeout, IssuerFailure::InvalidResponse] {
        let fixture = Fixture::new();
        let running = fixture.start(sync_directory);
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
        assert!(read_metadata(fixture.root.path()).unwrap().is_none());
        let mut fresh = fixture.observe(&running);
        assert!(matches!(
            fresh.recv().await,
            Some(FleetLoginEvent::Code { .. })
        ));
        assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);
        running.shutdown().await.unwrap();
        assert!(fresh.recv().await.is_none());
    }
}

#[tokio::test]
async fn fleet_machine_login_background_refresh_failure_withdraws_gate_and_requires_new_code() {
    let fixture = Fixture::new();
    let running = fixture.start(sync_directory);
    let mut events = fixture.observe(&running);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    fixture.issue();
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::LoggedIn { .. })
    ));
    let mut changes = running.handle.source.token.clone();
    changes.borrow_and_update();

    fixture
        .answers
        .send(Err(IssuerFailure::InvalidGrant))
        .unwrap();
    fixture.clock.0.send_replace(255_000);
    changes.changed().await.unwrap();

    assert_eq!(fixture.issuer.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    let mut fresh = fixture.observe(&running);
    assert!(matches!(
        fresh.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 2);
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_unusable_store_refuses_before_authorization_without_writes() {
    use codex_router_secret_store::classified_store_open::UnusableCause;
    for cause in [
        UnusableCause::KeychainUnavailable,
        UnusableCause::InvalidCredentialData,
        UnusableCause::MigrationIncomplete,
    ] {
        let fixture = Fixture::new();
        let credentials = Arc::new(RefreshCredentialStore::Unusable { cause });
        let running = fixture.start_with_credentials(sync_directory, credentials);
        let mut events = fixture.observe(&running);

        assert_eq!(
            events.recv().await,
            Some(FleetLoginEvent::Refused {
                reason: FleetLoginRefusal::StoreUnusable { cause }
            })
        );

        assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 0);
        assert_eq!(
            running.handle.source.machine_login_state(),
            MachineLoginState::NotLoggedIn
        );
        assert_eq!(std::fs::read_dir(fixture.root.path()).unwrap().count(), 0);
        running.shutdown().await.unwrap();
    }
}

struct TraceCapture(Arc<std::sync::Mutex<Vec<u8>>>);

#[tokio::test]
async fn fleet_machine_login_cli_disconnect_does_not_cancel_the_host_owned_attempt() {
    let fixture = Fixture::new();
    let running = fixture.start(sync_directory);
    let mut events = fixture.observe(&running);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    let mut token = running.handle.source.token.clone();
    token.borrow_and_update();

    drop(events);
    fixture.issue();
    token.changed().await.unwrap();

    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::LoggedIn
    );
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);
    assert_eq!(
        read_metadata(fixture.root.path())
            .unwrap()
            .unwrap()
            .enrollment_id
            .as_str(),
        ENROLLMENT
    );
    running.shutdown().await.unwrap();
}
impl std::io::Write for TraceCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("trace capture lock poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn fleet_machine_login_records_public_outcomes_and_ids_without_codes_or_credentials() {
    let bytes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let output = bytes.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || TraceCapture(output.clone()))
        .finish();
    let trace_guard = tracing::subscriber::set_default(subscriber);
    let fixture = Fixture::new();
    let running = fixture.start(sync_directory);
    let mut events = fixture.observe(&running);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    fixture.issue();
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::LoggedIn { .. })
    ));
    running.shutdown().await.unwrap();
    let fixture = Fixture::new();
    let running = fixture.start(sync_directory);
    let mut events = fixture.observe(&running);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    fixture.answers.send(Err(IssuerFailure::Timeout)).unwrap();
    fixture.clock.0.send_replace(5_000);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::OutcomeUnknown { .. })
    ));
    running.shutdown().await.unwrap();
    drop(trace_guard);

    let text = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(text.contains("machineLogin"));
    assert!(text.contains("loggedIn"));
    assert!(text.contains("outcomeUnknown"));
    assert!(text.contains(ORGANIZATION));
    assert!(text.contains(ENROLLMENT));
    for secret in [
        "ABCD-EFGH",
        "device-secret-canary",
        "access-secret-canary",
        "refresh-secret-canary",
    ] {
        assert!(!text.contains(secret));
    }
}

#[path = "login_owner_storage_tests.rs"]
mod storage;
