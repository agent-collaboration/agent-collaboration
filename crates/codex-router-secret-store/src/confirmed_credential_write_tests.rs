//! Confirmed publication syncs the owning directory after the real envelope rename.
use super::*;

fn key() -> SecretKey {
    FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        "019f0000-0000-7000-8000-000000000001",
        "019f0000-0000-7000-8000-000000000002",
    )
    .unwrap()
    .secret_key()
    .unwrap()
}
fn store(root: &std::path::Path) -> EncryptedCredentialStore {
    EncryptedCredentialStore::new(
        FileSecretStore::open(root).unwrap(),
        PooledCredentialDataKey::from_bytes([0x54; 32]),
    )
}

#[test]
fn confirmed_machine_credential_write_keeps_only_the_rotated_envelope() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let key = key();
    store
        .write_staged_confirmed(&key, &SecretString::new("first-refresh-canary"))
        .unwrap();

    store
        .write_staged_confirmed(&key, &SecretString::new("rotated-refresh-canary"))
        .unwrap();

    assert_eq!(
        store.read_secret(&key).unwrap().expose_secret(),
        "rotated-refresh-canary"
    );
    let names: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2, "one envelope and the existing store lock");
    assert!(names.contains(&std::ffi::OsString::from(format!("{}.v2", key.as_str()))));
}

#[test]
fn confirmed_machine_credential_write_reports_sync_failure_after_rename_without_rollback() {
    let root = tempfile::tempdir().unwrap();
    let ready = store(root.path());
    let key = key();
    ready
        .write_staged(&key, &SecretString::new("original-refresh-canary"))
        .unwrap();
    let failing = crate::test_support::with_confirmed_credential_sync_failure(ready.clone());

    let result = failing.write_staged_confirmed(&key, &SecretString::new("rotated-refresh-canary"));

    assert!(matches!(result, Err(SecretStoreError::Filesystem { .. })));
    assert_eq!(
        ready.read_secret(&key).unwrap().expose_secret(),
        "rotated-refresh-canary",
        "uncertain publication never rolls back a committed rename"
    );
}

#[test]
fn confirmed_machine_credential_write_requires_ready_encryption_before_any_file_change() {
    let root = tempfile::tempdir().unwrap();
    let unavailable =
        EncryptedCredentialStore::key_unavailable(FileSecretStore::open(root.path()).unwrap());

    assert!(matches!(
        unavailable.write_staged_confirmed(&key(), &SecretString::new("must-not-write")),
        Err(SecretStoreError::KeyUnavailable)
    ));

    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
