//! R41: classification observes the real file/key boundary without repairing it.
use std::{collections::BTreeMap, fs, path::Path, sync::Mutex};

use crate::{
    SecretStore,
    classified_store_open::{ClassifiedCredentialStore, UnusableCause},
    encrypted_credential_store::EncryptedCredentialStore,
    file_backend::FileSecretStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    keychain_data_key::{KeychainAccess, KeychainAccessError, PooledCredentialDataKey},
};
use codex_router_core::redaction::SecretString;

const STORE_ID: &str = "019f0000-0000-7000-8000-000000000001";

struct RecordedKeychain {
    response: Result<Option<Vec<u8>>, KeychainAccessError>,
    calls: Mutex<Vec<&'static str>>,
}

impl RecordedKeychain {
    fn key(bytes: Vec<u8>) -> Self {
        Self {
            response: Ok(Some(bytes)),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl KeychainAccess for RecordedKeychain {
    fn read_secret(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, KeychainAccessError> {
        self.calls
            .lock()
            .map_err(|_| KeychainAccessError::TestKeychainAccessForbidden)?
            .push("read");
        self.response.clone()
    }

    fn add_secret(&self, _: &str, _: &str, _: &[u8]) -> Result<(), KeychainAccessError> {
        self.calls
            .lock()
            .map_err(|_| KeychainAccessError::TestKeychainAccessForbidden)?
            .push("add");
        Err(KeychainAccessError::TestKeychainAccessForbidden)
    }
}

fn initialized_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("store-id"), STORE_ID).unwrap();
    fs::write(root.path().join("format-v2.marker"), b"2\n").unwrap();
    root
}

fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        assert!(
            entry.file_type().unwrap().is_file(),
            "fixture uses only files"
        );
        files.insert(entry.file_name().into(), fs::read(entry.path()).unwrap());
    }
    files
}

fn assert_unusable_untouched(root: &Path, keychain: &RecordedKeychain, expected: UnusableCause) {
    let before = snapshot(root);
    for _ in 0..2 {
        let classified = ClassifiedCredentialStore::open_read_only_with_keychain(root, keychain);
        assert!(
            matches!(classified, ClassifiedCredentialStore::Unusable { cause } if cause == expected),
            "expected {expected:?}, received {classified:?}"
        );
        assert_eq!(snapshot(root), before);
    }
    assert!(!keychain.calls.lock().unwrap().contains(&"add"));
}

// Wrong build: reusing the mutating opener creates store-id, a lock or a marker.
#[test]
fn classified_open_empty_root_creates_no_files() {
    let root = tempfile::tempdir().unwrap();
    let keychain = RecordedKeychain::key(vec![0x42; 32]);
    for _ in 0..2 {
        assert!(matches!(
            ClassifiedCredentialStore::open_read_only_with_keychain(root.path(), &keychain),
            ClassifiedCredentialStore::Absent
        ));
        assert!(snapshot(root.path()).is_empty());
    }
    assert!(keychain.calls.lock().unwrap().is_empty());
}

#[test]
fn classified_open_missing_root_stays_missing() {
    let parent = tempfile::tempdir().unwrap();
    let missing = parent.path().join("absent");
    let keychain = RecordedKeychain::key(vec![0x42; 32]);
    assert!(matches!(
        ClassifiedCredentialStore::open_read_only_with_keychain(&missing, &keychain),
        ClassifiedCredentialStore::Absent
    ));
    assert!(!missing.exists());
}

// Wrong build: KeyUnavailable is treated as absent, or the open creates .store.lock.
#[test]
fn classified_open_denied_keychain_is_unusable_without_writes() {
    let root = initialized_root();
    let keychain = RecordedKeychain {
        response: Err(KeychainAccessError::Unavailable),
        calls: Mutex::new(Vec::new()),
    };
    assert_unusable_untouched(root.path(), &keychain, UnusableCause::KeychainUnavailable);
}

#[test]
fn classified_open_invalid_key_keeps_its_cause_without_writes() {
    let root = initialized_root();
    assert_unusable_untouched(
        root.path(),
        &RecordedKeychain::key(vec![0x42; 31]),
        UnusableCause::InvalidDataKey,
    );
}

#[test]
fn classified_open_missing_key_is_not_absent_storage() {
    let root = initialized_root();
    let keychain = RecordedKeychain {
        response: Ok(None),
        calls: Mutex::new(Vec::new()),
    };
    assert_unusable_untouched(root.path(), &keychain, UnusableCause::KeyMissing);
}

#[test]
fn classified_open_rejected_service_is_unusable() {
    let root = initialized_root();
    let keychain = RecordedKeychain {
        response: Err(KeychainAccessError::ServiceRejected),
        calls: Mutex::new(Vec::new()),
    };
    assert_unusable_untouched(
        root.path(),
        &keychain,
        UnusableCause::KeychainServiceRejected,
    );
}

#[test]
fn classified_open_test_guard_is_unusable() {
    let root = initialized_root();
    let keychain = RecordedKeychain {
        response: Err(KeychainAccessError::TestKeychainAccessForbidden),
        calls: Mutex::new(Vec::new()),
    };
    assert_unusable_untouched(
        root.path(),
        &keychain,
        UnusableCause::TestKeychainAccessForbidden,
    );
}

#[test]
fn classified_open_absent_backend_stays_absent_without_writes() {
    let root = initialized_root();
    let keychain = RecordedKeychain {
        response: Err(KeychainAccessError::BackendAbsent),
        calls: Mutex::new(Vec::new()),
    };
    let before = snapshot(root.path());
    for _ in 0..2 {
        assert!(matches!(
            ClassifiedCredentialStore::open_read_only_with_keychain(root.path(), &keychain),
            ClassifiedCredentialStore::Absent
        ));
        assert_eq!(snapshot(root.path()), before);
    }
    assert!(!keychain.calls.lock().unwrap().contains(&"add"));
}

#[test]
fn classified_open_unreadable_metadata_is_unusable() {
    let root = initialized_root();
    fs::write(root.path().join("store-id"), [0xff]).unwrap();
    assert_unusable_untouched(
        root.path(),
        &RecordedKeychain::key(vec![0x42; 32]),
        UnusableCause::MetadataUnavailable,
    );
}

#[test]
fn classified_open_debug_plaintext_declaration_is_not_an_encrypted_store() {
    let root = initialized_root();
    fs::write(
        root.path().join("debug-plaintext.marker"),
        b"debug-plaintext-v1\n",
    )
    .unwrap();
    assert_unusable_untouched(
        root.path(),
        &RecordedKeychain::key(vec![0x42; 32]),
        UnusableCause::DebugPlaintextConfigured,
    );
}

#[test]
fn classified_open_unsafe_root_is_never_absent() {
    let parent = tempfile::tempdir().unwrap();
    let unsafe_root = parent.path().join("not-a-directory");
    fs::write(&unsafe_root, b"unchanged").unwrap();
    let keychain = RecordedKeychain::key(vec![0x42; 32]);
    let before = snapshot(parent.path());
    for _ in 0..2 {
        assert!(matches!(
            ClassifiedCredentialStore::open_read_only_with_keychain(&unsafe_root, &keychain),
            ClassifiedCredentialStore::Unusable {
                cause: UnusableCause::UnsafeStoreRoot
            }
        ));
        assert_eq!(snapshot(parent.path()), before);
    }
    assert!(keychain.calls.lock().unwrap().is_empty());
}

#[test]
fn classified_open_invalid_marker_is_unusable() {
    let root = initialized_root();
    fs::write(root.path().join("format-v2.marker"), b"invalid").unwrap();
    assert_unusable_untouched(
        root.path(),
        &RecordedKeychain::key(vec![0x42; 32]),
        UnusableCause::InvalidStoreMetadata,
    );
}

#[test]
fn classified_open_invalid_store_id_is_unusable() {
    let root = initialized_root();
    fs::write(root.path().join("store-id"), b"invalid").unwrap();
    assert_unusable_untouched(
        root.path(),
        &RecordedKeychain::key(vec![0x42; 32]),
        UnusableCause::InvalidStoreMetadata,
    );
}

#[test]
fn classified_open_half_migrated_store_is_not_repaired() {
    let root = initialized_root();
    fs::remove_file(root.path().join("format-v2.marker")).unwrap();
    assert_unusable_untouched(
        root.path(),
        &RecordedKeychain::key(vec![0x42; 32]),
        UnusableCause::MigrationIncomplete,
    );
}

#[test]
fn classified_open_damaged_envelope_is_unusable() {
    let root = initialized_root();
    let key = FleetSecretKey::new(FleetSecretKind::MachineCredential, STORE_ID, STORE_ID)
        .unwrap()
        .secret_key()
        .unwrap();
    fs::write(root.path().join(format!("{}.v2", key.as_str())), b"damaged").unwrap();
    assert_unusable_untouched(
        root.path(),
        &RecordedKeychain::key(vec![0x42; 32]),
        UnusableCause::InvalidCredentialData,
    );
}

#[test]
fn classified_open_usable_store_reads_real_encryption_without_mutation_or_secret_debug() {
    let root = initialized_root();
    let key = FleetSecretKey::new(FleetSecretKind::MachineCredential, STORE_ID, STORE_ID)
        .unwrap()
        .secret_key()
        .unwrap();
    let secret = SecretString::new("f11-refresh-canary");
    let initialized = EncryptedCredentialStore::new(
        FileSecretStore::open(root.path()).unwrap(),
        PooledCredentialDataKey::from_bytes([0x42; 32]),
    );
    initialized.write_secret(&key, &secret).unwrap();
    let before = snapshot(root.path());
    let keychain = RecordedKeychain::key(vec![0x42; 32]);
    for _ in 0..2 {
        let classified =
            ClassifiedCredentialStore::open_read_only_with_keychain(root.path(), &keychain);
        assert_eq!(format!("{classified:?}"), "Usable");
        let ClassifiedCredentialStore::Usable(store) = classified else {
            panic!("initialized store must be usable");
        };
        assert_eq!(
            store.read_secret(&key).unwrap().expose_secret(),
            secret.expose_secret()
        );
        assert_eq!(snapshot(root.path()), before);
    }
    assert!(!keychain.calls.lock().unwrap().contains(&"add"));
}
