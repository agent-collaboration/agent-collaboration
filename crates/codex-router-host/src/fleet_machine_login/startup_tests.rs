//! Mode selection is independent of storage availability and saved-row presence.
use super::super::{
    issuer_http::tls_fixture, login_clock::LoginFuture, login_owner::MachineLoginIdentity,
};
use super::*;
use codex_router_fleet_state::test_support::MemoryFleetLoginRepository;
use collaboration_contracts::{MachineLoginGate, MachineLoginState};
use runtime_primitives::SyncTime;

struct FixedClock;
impl FleetLoginClock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
    fn wait_until(&self, _: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        Box::pin(std::future::pending())
    }
}

// Oracle: explicit deployment modes (Lead01a1979c/01a197a4). Wrong behavior:
// selecting Access because a required repository is unavailable. No pre-fix
// red observed for these newly pinned selector tests.
#[test]
fn fleet_machine_login_access_mode_needs_no_fleet_repository() {
    assert!(matches!(
        MachineLoginStartup::AccessEnrollment.resolve(),
        Ok(None)
    ));
}

#[test]
fn fleet_machine_login_token_mode_missing_repository_is_typed_unavailable() {
    let selected = MachineLoginStartup::MachineTokens {
        repository: None,
        dependencies: None,
    };
    assert!(matches!(
        selected.resolve(),
        Err(FleetLoginStorageFailure::Unavailable)
    ));
}

#[tokio::test]
async fn fleet_machine_login_empty_token_repository_prepares_no_enrollment_session() {
    // Arrange: a valid explicit token-mode selection with no confirmed row.
    let repository = Arc::new(MemoryFleetLoginRepository::new());
    let selected = MachineLoginStartup::MachineTokens {
        repository: Some(repository),
        dependencies: None,
    };
    let inputs = selected.resolve().unwrap().unwrap();
    let saved = inputs.repository.load_fleet_login().await.unwrap();
    assert!(saved.is_none());
    let clock: Arc<dyn FleetLoginClock> = Arc::new(FixedClock);
    let peer =
        serde_json::from_value(serde_json::json!("019f0000-0000-7000-8000-000000000003")).unwrap();
    let owner = RunningMachineLogin::start(
        MachineLoginIdentity {
            installation: "019f0000-0000-7000-8000-000000000001"
                .to_owned()
                .try_into()
                .unwrap(),
            peer,
        },
        MachineLoginWork {
            issuer: Arc::new(HttpMachineIssuerConnection {
                client: tls_fixture::issuer_client().unwrap(),
                clock: clock.clone(),
            }),
            clock,
            publication: Arc::new(MachineLoginPublication {
                repository: inputs.repository,
                credentials: Arc::new(RefreshCredentialStore::MemoryOnly),
                after_commit: None,
            }),
        },
        saved,
    );
    let login = HostMachineLogin {
        owner,
        edge: EdgeCredential::Ready(AccessServiceToken::NoEdgeCredentials),
        peer,
    };

    // Act / Assert: no SessionPump inputs exist, so no old enrollment
    // connection can be prepared through this owner. Root startup is separate.
    assert_eq!(
        login.binding().source().machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    assert!(matches!(login.route(), MachineLoginRoute::AwaitingLogin));
    assert!(login.session_inputs().unwrap().is_none());
    login.shutdown().await.unwrap();
}
