//! Real publication faults are observed through the lifetime owner and its gate.
use super::*;

fn encrypted_credentials(
    root: &std::path::Path,
) -> codex_router_secret_store::encrypted_credential_store::EncryptedCredentialStore {
    use codex_router_secret_store::{
        encrypted_credential_store::EncryptedCredentialStore, file_backend::FileSecretStore,
        keychain_data_key::PooledCredentialDataKey,
    };
    EncryptedCredentialStore::new(
        FileSecretStore::open(root.join("secrets")).unwrap(),
        PooledCredentialDataKey::from_bytes([0x54; 32]),
    )
}
fn repository(
    store: codex_router_secret_store::encrypted_credential_store::EncryptedCredentialStore,
) -> Arc<RefreshCredentialStore> {
    Arc::new(RefreshCredentialStore::Encrypted {
        store: Arc::new(store),
        installation: "019f0000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
    })
}

#[tokio::test]
async fn fleet_machine_login_first_credential_rename_then_failed_sync_is_unknown_and_gate_closed() {
    use codex_router_secret_store::{
        SecretStore,
        fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    };
    let fixture = Fixture::new();
    let encrypted = encrypted_credentials(fixture.root.path());
    let failing = codex_router_secret_store::test_support::with_confirmed_credential_sync_failure(
        encrypted.clone(),
    );
    let running = fixture.start_with_credentials(sync_directory, repository(failing));
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
        read_metadata(fixture.root.path()).unwrap().is_none(),
        "public metadata waits for a confirmed credential write"
    );
    let key = FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        "019f0000-0000-7000-8000-000000000002",
        ENROLLMENT,
    )
    .unwrap()
    .secret_key()
    .unwrap();
    assert!(
        encrypted
            .read_secret(&key)
            .unwrap()
            .expose_secret()
            .contains("refresh-secret-canary"),
        "a rename already committed is never rolled back"
    );
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_rotation_sync_failure_withdraws_gate_without_replaying_or_reverting() {
    let fixture = Fixture::new();
    let encrypted = encrypted_credentials(fixture.root.path());
    let credentials = repository(encrypted.clone());
    let initial = RefreshCredentialBundle {
        machine_client_id: ENROLLMENT.to_owned().try_into().unwrap(),
        refresh_credential: SecretString::new("original-rotation-secret"),
    };
    let metadata = MachineLoginMetadata {
        organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
        enrollment_id: initial.machine_client_id.clone(),
    };
    let publication = MachineLoginPublication {
        root: fixture.root.path().to_owned(),
        credentials: credentials.clone(),
        sync_directory,
    };
    publication
        .publish_login(
            &metadata,
            &initial,
            RefreshCredentialKeeping::EncryptedStore,
        )
        .unwrap();
    let failing = codex_router_secret_store::test_support::with_confirmed_credential_directory_sync(
        encrypted,
        |directory| {
            if directory.join("fixture-fail-confirmed-sync").is_file() {
                Err(std::io::Error::other(
                    "event-injected rotation sync failure",
                ))
            } else {
                std::fs::File::open(directory)?.sync_all()
            }
        },
    );
    let running = fixture.start_with_credentials(sync_directory, repository(failing));
    let mut changes = running.handle.source.token.clone();
    changes.borrow_and_update();
    fixture.issue();

    changes.changed().await.unwrap();
    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::LoggedIn
    );
    changes.borrow_and_update();
    std::fs::write(
        fixture
            .root
            .path()
            .join("secrets/fixture-fail-confirmed-sync"),
        b"enabled by test event",
    )
    .unwrap();
    fixture
        .answers
        .send(Ok(DevicePollResult::Issued(IssuedMachineToken {
            token: SecretString::new("unpublished-rotation-access-canary"),
            valid_until: SyncTime::from_unix_millis(600_000),
            credential: RefreshCredentialBundle {
                machine_client_id: initial.machine_client_id.clone(),
                refresh_credential: SecretString::new("renamed-rotation-refresh-canary"),
            },
        })))
        .unwrap();
    fixture.clock.0.send_replace(255_000);
    changes.changed().await.unwrap();

    assert_eq!(
        running.handle.source.machine_login_state(),
        MachineLoginState::NotLoggedIn
    );
    assert_eq!(fixture.issuer.refreshes.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.issuer.authorizations.load(Ordering::SeqCst), 0);
    assert_eq!(
        credentials
            .read(&initial.machine_client_id)
            .unwrap()
            .unwrap()
            .refresh_credential
            .expose_secret(),
        "renamed-rotation-refresh-canary"
    );
    assert_eq!(read_metadata(fixture.root.path()).unwrap(), Some(metadata));
    let mut next = fixture.observe(&running);
    assert!(matches!(
        next.recv().await,
        Some(FleetLoginEvent::Code { .. })
    ));
    assert_eq!(
        fixture.issuer.refreshes.load(Ordering::SeqCst),
        2,
        "the retired refresh credential is never replayed automatically"
    );
    running.shutdown().await.unwrap();
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

#[tokio::test]
async fn fleet_machine_login_failed_retirement_sync_is_unknown_after_real_deletion_and_keeps_gate_closed()
 {
    use codex_router_secret_store::{
        SecretStore,
        encrypted_credential_store::EncryptedCredentialStore,
        file_backend::FileSecretStore,
        fleet_secret_key::{FleetSecretKey, FleetSecretKind},
        keychain_data_key::PooledCredentialDataKey,
    };
    let fixture = Fixture::new();
    let files = FileSecretStore::open(fixture.root.path().join("secrets")).unwrap();
    let encrypted =
        EncryptedCredentialStore::new(files, PooledCredentialDataKey::from_bytes([0x54; 32]));
    let installation = "019f0000-0000-7000-8000-000000000002";
    let old = FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        installation,
        "019f0000-0000-7000-8000-000000000009",
    )
    .unwrap()
    .secret_key()
    .unwrap();
    encrypted
        .write_staged(&old, &SecretString::new("old-enrollment-canary"))
        .unwrap();
    let failing = codex_router_secret_store::test_support::with_machine_retirement_sync_failure(
        encrypted.clone(),
    );
    let credentials = Arc::new(RefreshCredentialStore::Encrypted {
        store: Arc::new(failing),
        installation: installation.to_owned().try_into().unwrap(),
    });
    let running = fixture.start_with_credentials(sync_directory, credentials);
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
        !fixture
            .root
            .path()
            .join("secrets")
            .join(format!("{}.v2", old.as_str()))
            .exists()
    );
    let active = FleetSecretKey::new(FleetSecretKind::MachineCredential, installation, ENROLLMENT)
        .unwrap()
        .secret_key()
        .unwrap();
    assert!(
        encrypted
            .read_secret(&active)
            .unwrap()
            .expose_secret()
            .contains("refresh-secret-canary")
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
}
