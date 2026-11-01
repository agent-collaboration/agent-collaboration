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
        Box::pin(pending())
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
                answers: Mutex::new(receiver),
            }),
            answers,
        }
    }
    fn start(&self, sync: fn(&std::path::Path) -> std::io::Result<()>) -> RunningMachineLogin {
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
                credentials: Arc::new(RefreshCredentialStore::MemoryOnly),
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
async fn fleet_machine_login_unconfirmed_publication_keeps_real_file_and_gate_closed_across_restart()
 {
    let fixture = Fixture::new();
    let running = fixture.start(|_| Err(std::io::Error::other("injected directory sync failure")));
    let mut events = fixture.observe(&running);
    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    fixture.issue();

    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::OutcomeUnknown {
            context: FleetLoginUnknown::CredentialPublication {
                keeping: RefreshCredentialKeeping::MemoryOnly,
                ..
            }
        })
    ));
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    assert_eq!(
        read_metadata(fixture.root.path())
            .unwrap()
            .unwrap()
            .enrollment_id
            .as_str(),
        ENROLLMENT
    );
    running.shutdown().await.unwrap();
    let restarted = fixture.start(sync_directory);
    assert_eq!(
        restarted.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    let mut other = restarted.handle.observe(
        "019f0000-0000-7000-8000-000000000099"
            .to_owned()
            .try_into()
            .unwrap(),
        "Other organization".to_owned().try_into().unwrap(),
    );
    assert!(matches!(
        other.recv().await,
        Some(FleetLoginEvent::Refused {
            reason: FleetLoginRefusal::AlreadyInOrganization { .. }
        })
    ));
    let mut fresh = fixture.observe(&restarted);
    assert!(matches!(
        fresh.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 2);
    restarted.shutdown().await.unwrap();
}
