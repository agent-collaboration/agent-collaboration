//! Real protected envelopes retain one rotating credential beside its public client identifier.
use codex_router_secret_store::{
    encrypted_credential_store::EncryptedCredentialStore, file_backend::FileSecretStore,
    keychain_data_key::PooledCredentialDataKey,
};

use super::super::login_publication::MachineLoginPublication;
use super::*;
use codex_router_fleet_state::fleet_login::{SavedFleetCentral, SavedFleetLogin};
use codex_router_fleet_state::{
    fleet_login_repository::FleetLoginRepository, test_support::MemoryFleetLoginRepository,
};

const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000001";
const FIRST: &str = "019f0000-0000-7000-8000-000000000011";
const UNCERTAIN: &str = "019f0000-0000-7000-8000-000000000012";
const LATEST: &str = "019f0000-0000-7000-8000-000000000013";

fn encrypted(root: &std::path::Path) -> Arc<RefreshCredentialStore> {
    let files = FileSecretStore::open(root.join("secrets")).unwrap();
    Arc::new(RefreshCredentialStore::Encrypted {
        store: Arc::new(EncryptedCredentialStore::new(
            files,
            PooledCredentialDataKey::from_bytes([0x54; 32]),
        )),
        installation: INSTALLATION.to_owned().try_into().unwrap(),
    })
}
fn bundle(enrollment: &str, secret: &str) -> RefreshCredentialBundle {
    RefreshCredentialBundle {
        machine_client_id: enrollment.to_owned().try_into().unwrap(),
        refresh_credential: SecretString::new(secret),
    }
}
fn saved(enrollment: &str) -> SavedFleetLogin {
    SavedFleetLogin {
        sign_in: "https://issuer.example.invalid/issuer"
            .to_owned()
            .try_into()
            .unwrap(),
        organization_id: "019f0000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
        enrollment_id: enrollment.to_owned().try_into().unwrap(),
        machine_environment: fleet_identity_protocol::MachineEnvironment::Physical,
        central: SavedFleetCentral {
            receiver: serde_json::from_value(serde_json::json!(
                "019f0000-0000-7000-8000-000000000021"
            ))
            .unwrap(),
            relay: serde_json::from_value(serde_json::json!(
                "019f0000-0000-7000-8000-000000000022"
            ))
            .unwrap(),
        },
    }
}
fn login_candidate(enrollment: &str) -> codex_router_fleet_state::fleet_login::FleetLoginCandidate {
    let saved = saved(enrollment);
    codex_router_fleet_state::fleet_login::FleetLoginCandidate {
        sign_in: saved.sign_in,
        organization_id: saved.organization_id,
        enrollment_id: saved.enrollment_id,
        requested_machine_environment: Some(saved.machine_environment),
        central: saved.central,
    }
}

fn generation_names(root: &std::path::Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(root.join("secrets"))
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().into_string().unwrap();
            name.ends_with(".v2").then_some(name)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn fleet_machine_login_refresh_rotation_replaces_one_envelope_and_reopens_with_the_saved_client() {
    let root = tempfile::tempdir().unwrap();
    let credentials = encrypted(root.path());
    credentials
        .write(&bundle(FIRST, "first-refresh-secret-canary"))
        .unwrap();
    credentials
        .write(&bundle(FIRST, "rotated-refresh-secret-canary"))
        .unwrap();

    let reopened = encrypted(root.path());
    let restored = reopened
        .read(&FIRST.to_owned().try_into().unwrap())
        .unwrap()
        .unwrap();

    assert_eq!(restored.machine_client_id.as_str(), FIRST);
    assert_eq!(
        restored.refresh_credential.expose_secret(),
        "rotated-refresh-secret-canary"
    );
    assert_eq!(generation_names(root.path()).len(), 1);
    let RefreshCredentialStore::Encrypted {
        store,
        installation,
    } = reopened.as_ref()
    else {
        panic!("encrypted fixture");
    };
    let key = credential_key(installation, &restored.machine_client_id).unwrap();
    let stored: serde_json::Value =
        serde_json::from_str(store.read_secret(&key).unwrap().expose_secret()).unwrap();
    assert_eq!(stored.as_object().unwrap().len(), 2);
    assert_eq!(stored["machineClientId"], FIRST);
    assert_eq!(stored["refreshCredential"], "rotated-refresh-secret-canary");
    for name in generation_names(root.path()) {
        let bytes = std::fs::read(root.path().join("secrets").join(name)).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("first-refresh-secret-canary"));
        assert!(!text.contains("rotated-refresh-secret-canary"));
    }
    assert!(
        reopened
            .read(&LATEST.to_owned().try_into().unwrap())
            .unwrap()
            .is_none(),
        "a missing credential requires a fresh code, without repairing files"
    );
}

#[tokio::test]
async fn fleet_machine_login_confirmed_login_retires_an_uncertain_attempt_without_an_unknown_handler_sweep()
 {
    let root = tempfile::tempdir().unwrap();
    let credentials = encrypted(root.path());
    let repository = Arc::new(MemoryFleetLoginRepository::new());
    let publication = MachineLoginPublication {
        repository: repository.clone(),
        credentials: credentials.clone(),
        after_commit: None,
    };
    publication
        .publish_login(
            &login_candidate(FIRST),
            &bundle(FIRST, "original-secret"),
            RefreshCredentialKeeping::EncryptedStore,
        )
        .await
        .unwrap();
    let uncertain = MachineLoginPublication {
        repository: repository.clone(),
        credentials: credentials.clone(),
        after_commit: Some(|| Err(())),
    };

    assert!(
        uncertain
            .publish_login(
                &login_candidate(UNCERTAIN),
                &bundle(UNCERTAIN, "uncertain-secret"),
                RefreshCredentialKeeping::EncryptedStore
            )
            .await
            .is_err()
    );

    assert_eq!(
        generation_names(root.path()).len(),
        2,
        "the unknown ending leaves committed files untouched"
    );
    assert_eq!(
        repository.load_fleet_login().await.unwrap(),
        Some(saved(UNCERTAIN))
    );
    publication
        .publish_login(
            &login_candidate(LATEST),
            &bundle(LATEST, "latest-secret"),
            RefreshCredentialKeeping::EncryptedStore,
        )
        .await
        .unwrap();
    let expected = credential_key(
        &INSTALLATION.to_owned().try_into().unwrap(),
        &LATEST.to_owned().try_into().unwrap(),
    )
    .unwrap();
    assert_eq!(
        generation_names(root.path()),
        vec![format!("{}.v2", expected.as_str())]
    );
    assert_eq!(
        repository.load_fleet_login().await.unwrap(),
        Some(saved(LATEST))
    );
    assert_eq!(
        credentials
            .read(&LATEST.to_owned().try_into().unwrap())
            .unwrap()
            .unwrap()
            .refresh_credential
            .expose_secret(),
        "latest-secret"
    );
}

#[test]
fn fleet_machine_login_memory_or_unusable_storage_never_writes_a_refresh_credential() {
    let root = tempfile::tempdir().unwrap();
    let credential = bundle(FIRST, "never-write-secret-canary");
    let memory = RefreshCredentialStore::MemoryOnly;
    memory.write(&credential).unwrap();
    assert!(
        memory
            .read(&credential.machine_client_id)
            .unwrap()
            .is_none()
    );
    let unusable = RefreshCredentialStore::Unusable {
        cause: UnusableCause::KeychainUnavailable {},
    };
    assert!(unusable.write(&credential).is_err());
    assert!(unusable.read(&credential.machine_client_id).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
