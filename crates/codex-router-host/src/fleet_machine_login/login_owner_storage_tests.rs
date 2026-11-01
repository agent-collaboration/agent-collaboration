//! Real encrypted files and memory repository publication determine fault outcomes.
use super::*;
use codex_router_secret_store::{
    SecretStore,
    encrypted_credential_store::EncryptedCredentialStore,
    file_backend::FileSecretStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    keychain_data_key::PooledCredentialDataKey,
};

const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000002";
const OLD: &str = "019f0000-0000-7000-8000-000000000009";
const LATEST: &str = "019f0000-0000-7000-8000-000000000019";
fn encrypted(root: &std::path::Path) -> EncryptedCredentialStore {
    EncryptedCredentialStore::new(
        FileSecretStore::open(root.join("secrets")).unwrap(),
        PooledCredentialDataKey::from_bytes([0x54; 32]),
    )
}
fn key(enrollment: &str) -> codex_router_secret_store::model::SecretKey {
    FleetSecretKey::new(FleetSecretKind::MachineCredential, INSTALLATION, enrollment)
        .unwrap()
        .secret_key()
        .unwrap()
}
fn repository(store: EncryptedCredentialStore) -> Arc<RefreshCredentialStore> {
    Arc::new(RefreshCredentialStore::Encrypted {
        store: Arc::new(store),
        installation: INSTALLATION.to_owned().try_into().unwrap(),
    })
}
fn generations(root: &std::path::Path) -> usize {
    std::fs::read_dir(root.join("secrets"))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "v2")
        })
        .count()
}

#[tokio::test]
async fn fleet_machine_login_first_credential_sync_failure_keeps_old_credential_and_gate_closed() {
    let fixture = Fixture::new();
    let store = encrypted(fixture.root.path());
    store
        .write_staged(
            &key(OLD),
            &SecretString::new("old-confirmed-enrollment-canary"),
        )
        .unwrap();
    let failing = codex_router_secret_store::test_support::with_confirmed_credential_sync_failure(
        store.clone(),
    );
    let running = fixture.start_with(repository(failing), None).await;
    let mut events = fixture.observe(&running);
    events.recv().await.unwrap();
    fixture.issue();

    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::OutcomeUnknown {
            context: FleetLoginUnknown::CredentialPublication {
                keeping: RefreshCredentialKeeping::EncryptedStore,
                ..
            }
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
    assert_eq!(
        store.read_secret(&key(OLD)).unwrap().expose_secret(),
        "old-confirmed-enrollment-canary"
    );
    assert!(
        store
            .read_secret(&key(ENROLLMENT))
            .unwrap()
            .expose_secret()
            .contains("refresh-secret-canary")
    );
    assert_eq!(
        generations(fixture.root.path()),
        2,
        "no early retirement or uncertain-ending sweep"
    );
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_active_rotation_sync_failure_never_reverts_or_replays_the_credential()
{
    let fixture = Fixture::new();
    let store = encrypted(fixture.root.path());
    let failing = codex_router_secret_store::test_support::with_confirmed_credential_directory_sync(
        store.clone(),
        |directory| {
            if directory.join("fixture-fail-sync").is_file() {
                Err(std::io::Error::other("event-injected sync failure"))
            } else {
                std::fs::File::open(directory)?.sync_all()
            }
        },
    );
    let running = fixture.start_with(repository(failing), None).await;
    let mut events = fixture.observe(&running);
    events.recv().await.unwrap();
    fixture.issue();
    assert!(matches!(
        fixture.next(&running, &mut events).await,
        Some(FleetLoginEvent::LoggedIn { .. })
    ));
    let original = fixture.repository.load_fleet_login().await.unwrap();
    let mut token = running.handle.source.token.clone();
    token.borrow_and_update();
    std::fs::write(
        fixture.root.path().join("secrets/fixture-fail-sync"),
        b"test event",
    )
    .unwrap();
    fixture.issue_with(ENROLLMENT, "renamed-rotation-refresh-canary", 600_000);
    fixture.clock.0.send_replace(255_000);

    token.changed().await.unwrap();

    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    assert_eq!(fixture.issuer.refreshes.load(Ordering::SeqCst), 1);
    assert!(
        store
            .read_secret(&key(ENROLLMENT))
            .unwrap()
            .expose_secret()
            .contains("renamed-rotation-refresh-canary")
    );
    assert_eq!(
        fixture.repository.load_fleet_login().await.unwrap(),
        original
    );
    let mut fresh = running.handle.observe(
        None,
        ORGANIZATION.to_owned().try_into().unwrap(),
        "New code".to_owned().try_into().unwrap(),
        None,
    );
    assert!(matches!(
        fresh.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    assert_eq!(fixture.issuer.refreshes.load(Ordering::SeqCst), 1);
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_lost_commit_ack_keeps_real_row_and_retirement_waits_for_next_confirmed_login()
 {
    let fixture = Fixture::new();
    let store = encrypted(fixture.root.path());
    store
        .write_staged(&key(OLD), &SecretString::new("retired-enrollment-canary"))
        .unwrap();
    let running = fixture
        .start_with(repository(store.clone()), Some(|| Err(())))
        .await;
    let mut events = fixture.observe(&running);
    events.recv().await.unwrap();
    fixture.issue();

    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::OutcomeUnknown {
            context: FleetLoginUnknown::CredentialPublication { .. }
        })
    ));

    assert_eq!(
        fixture.repository.load_fleet_login().await.unwrap(),
        Some(saved(ENROLLMENT))
    );
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    assert_eq!(generations(fixture.root.path()), 2);
    running.shutdown().await.unwrap();

    let restarted = fixture.start_with(repository(store.clone()), None).await;
    let mut token = restarted.handle.source.token.clone();
    token.borrow_and_update();
    restarted.handle.route_adopted(Arc::new(saved(ENROLLMENT)));
    fixture.issue_with(ENROLLMENT, "restart-rotated-credential-canary", 300_000);
    token.changed().await.unwrap();
    assert_eq!(
        restarted.handle.source.machine_login_state(),
        MachineLoginState::LoggedIn
    );
    assert_eq!(
        fixture.issuer.authorizations.load(Ordering::SeqCst),
        1,
        "restart never resumes a device code"
    );
    assert_eq!(
        generations(fixture.root.path()),
        2,
        "startup does not sweep unreferenced generations"
    );
    let mut next = restarted.handle.observe(
        None,
        ORGANIZATION.to_owned().try_into().unwrap(),
        "Confirmed login".to_owned().try_into().unwrap(),
        None,
    );
    assert!(matches!(
        next.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    fixture.issue_with(LATEST, "latest-confirmed-refresh-canary", 600_000);
    fixture.clock.0.send_replace(10_000);
    assert!(matches!(
        fixture.next(&restarted, &mut next).await,
        Some(FleetLoginEvent::LoggedIn { .. })
    ));
    assert_eq!(
        fixture.repository.load_fleet_login().await.unwrap(),
        Some(saved(LATEST))
    );
    assert_eq!(generations(fixture.root.path()), 1);
    assert!(
        store
            .read_secret(&key(LATEST))
            .unwrap()
            .expose_secret()
            .contains("latest-confirmed-refresh-canary")
    );
    assert!(store.read_secret(&key(OLD)).is_err());
    assert!(store.read_secret(&key(ENROLLMENT)).is_err());
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_memory_restart_reuses_public_address_but_requires_a_new_code() {
    let fixture = Fixture::new();
    let first = fixture.start().await;
    let mut events = fixture.observe(&first);
    events.recv().await.unwrap();
    fixture.issue();
    assert!(matches!(
        fixture.next(&first, &mut events).await,
        Some(FleetLoginEvent::LoggedIn {
            keeping: RefreshCredentialKeeping::MemoryOnly,
            ..
        })
    ));
    first.shutdown().await.unwrap();
    let restarted = fixture.start().await;
    assert_eq!(
        restarted.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    let mut other = restarted.handle.observe(
        None,
        "019f0000-0000-7000-8000-000000000099"
            .to_owned()
            .try_into()
            .unwrap(),
        "Other org".to_owned().try_into().unwrap(),
        None,
    );
    assert!(matches!(
        other.recv().await,
        Some(FleetLoginEvent::Refused {
            reason: FleetLoginRefusal::AlreadyInOrganization { .. }
        })
    ));
    let mut fresh = restarted.handle.observe(
        None,
        ORGANIZATION.to_owned().try_into().unwrap(),
        "Saved issuer".to_owned().try_into().unwrap(),
        None,
    );
    assert!(matches!(
        fresh.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
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
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.issuer.refreshes.load(Ordering::SeqCst), 0);
    assert!(!fixture.root.path().join("secrets").exists());
    assert_eq!(
        fixture.repository.load_fleet_login().await.unwrap(),
        Some(saved(ENROLLMENT))
    );
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_retirement_sync_failure_after_real_deletion_is_unknown() {
    let fixture = Fixture::new();
    let store = encrypted(fixture.root.path());
    store
        .write_staged(&key(OLD), &SecretString::new("old-enrollment-canary"))
        .unwrap();
    let failing = codex_router_secret_store::test_support::with_machine_retirement_sync_failure(
        store.clone(),
    );
    let running = fixture.start_with(repository(failing), None).await;
    let mut events = fixture.observe(&running);
    events.recv().await.unwrap();
    fixture.issue();

    assert!(matches!(
        events.recv().await,
        Some(FleetLoginEvent::OutcomeUnknown {
            context: FleetLoginUnknown::CredentialPublication { .. }
        })
    ));

    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    assert!(store.read_secret(&key(OLD)).is_err());
    assert!(store.read_secret(&key(ENROLLMENT)).is_ok());
    assert_eq!(
        fixture.repository.load_fleet_login().await.unwrap(),
        Some(saved(ENROLLMENT))
    );
    running.shutdown().await.unwrap();
}
