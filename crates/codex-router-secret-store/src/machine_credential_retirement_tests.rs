//! Real encrypted files exercise the retirement boundary, including a failed final sync.
use super::*;
use std::{collections::BTreeMap, fs, path::Path};

const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000001";
const OTHER_INSTALLATION: &str = "019f0000-0000-7000-8000-000000000002";
const OLD: &str = "019f0000-0000-7000-8000-000000000011";
const UNREFERENCED: &str = "019f0000-0000-7000-8000-000000000012";
const ACTIVE: &str = "019f0000-0000-7000-8000-000000000013";

fn machine_key(generation: &str) -> FleetSecretKey {
    FleetSecretKey::new(FleetSecretKind::MachineCredential, INSTALLATION, generation).unwrap()
}

fn inventory(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                fs::read(path).unwrap(),
            )
        })
        .collect()
}

fn ready_store(root: &Path) -> EncryptedCredentialStore {
    fs::write(
        root.join("store-id"),
        "019f0000-0000-7000-8000-000000000021",
    )
    .unwrap();
    fs::write(root.join("format-v2.marker"), b"2\n").unwrap();
    EncryptedCredentialStore::open_for_process_with_keychain(
        root,
        &crate::test_support::DeterministicTestKeychainAccess,
    )
    .unwrap()
}

#[test]
fn machine_retirement_removes_old_and_unreferenced_generations_and_retains_other_families() {
    let root = tempfile::tempdir().unwrap();
    let store = ready_store(root.path());
    let active = machine_key(ACTIVE);
    let retired = [machine_key(OLD), machine_key(UNREFERENCED)];
    let retained = [
        active.secret_key().unwrap(),
        FleetSecretKey::new(FleetSecretKind::AccessClientSecret, INSTALLATION, OLD)
            .unwrap()
            .secret_key()
            .unwrap(),
        FleetSecretKey::new(FleetSecretKind::MachineAssertion, INSTALLATION, OLD)
            .unwrap()
            .secret_key()
            .unwrap(),
        FleetSecretKey::new(FleetSecretKind::MachineCredential, OTHER_INSTALLATION, OLD)
            .unwrap()
            .secret_key()
            .unwrap(),
        crate::account_tokens::openai_account_credential_bundle_key(
            &AccountId::new("retained_provider").unwrap(),
            1,
        )
        .unwrap(),
    ];
    for key in retired
        .iter()
        .map(|key| key.secret_key().unwrap())
        .chain(retained.clone())
    {
        store
            .write_staged(&key, &SecretString::new("retirement-secret-canary"))
            .unwrap();
    }
    let before = inventory(root.path());

    store.remove_retired_machine_credentials(&active).unwrap();

    let mut expected = before;
    for key in retired {
        expected.remove(&format!("{}.v2", key.secret_key().unwrap().as_str()));
    }
    assert_eq!(inventory(root.path()), expected);
    for key in retained {
        assert_eq!(
            store.read_secret(&key).unwrap().expose_secret(),
            "retirement-secret-canary"
        );
    }
}

#[test]
fn machine_retirement_requires_a_ready_store_and_machine_active_key_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let store = ready_store(root.path());
    let active = machine_key(ACTIVE);
    store
        .write_staged(&active.secret_key().unwrap(), &SecretString::new("active"))
        .unwrap();
    let unusable =
        EncryptedCredentialStore::key_unavailable(FileSecretStore::open(root.path()).unwrap());
    let before = inventory(root.path());

    assert!(matches!(
        unusable.remove_retired_machine_credentials(&active),
        Err(SecretStoreError::KeyUnavailable)
    ));
    let wrong_purpose =
        FleetSecretKey::new(FleetSecretKind::AccessClientSecret, INSTALLATION, ACTIVE).unwrap();
    assert!(matches!(
        store.remove_retired_machine_credentials(&wrong_purpose),
        Err(SecretStoreError::InvalidFleetSecretKey)
    ));
    assert_eq!(inventory(root.path()), before);
}

#[test]
fn machine_retirement_reports_unconfirmed_sync_after_real_deletion() {
    let root = tempfile::tempdir().unwrap();
    let store = ready_store(root.path());
    let active = machine_key(ACTIVE);
    let old = machine_key(OLD).secret_key().unwrap();
    for key in [active.secret_key().unwrap(), old.clone()] {
        store
            .write_staged(&key, &SecretString::new("encrypted-canary"))
            .unwrap();
    }

    let result = store.remove_retired_machine_credentials_with_sync(&active, |directory| {
        assert_eq!(directory, root.path());
        assert!(!directory.join(format!("{}.v2", old.as_str())).exists());
        Err(std::io::Error::other("injected directory sync failure"))
    });

    assert!(matches!(result, Err(SecretStoreError::Filesystem { .. })));
    assert!(store.read_secret(&active.secret_key().unwrap()).is_ok());
    assert!(!root.path().join(format!("{}.v2", old.as_str())).exists());
}
