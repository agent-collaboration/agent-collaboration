//! Generic secret dispatch must keep Fleet plaintext out of production files and temps.
// Assertions prove behavior; Result propagates fixture I/O errors.
#![deny(clippy::too_many_lines)]
use crate::{
    SecretStore,
    encrypted_credential_store::EncryptedCredentialStore,
    file_backend::{FileSecretStore, FileWriteTrace, FileWriteTraceEvent},
    keychain_data_key::load_or_create_pooled_credential_data_key,
    model::SecretKey,
    runtime_credential_store::RuntimeCredentialStore,
    test_support::DeterministicTestKeychainAccess,
};
use codex_router_core::redaction::SecretString;
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;
const INSTALLATION: &str = "550e8400-e29b-41d4-a716-446655440000";
const GENERATION: &str = "550e8400-e29b-41d4-a716-446655440001";
const CANARIES: [&str; 4] = [
    "fleet-access-secret-production-canary",
    "fleet-access-secret-runtime-update-canary",
    "Bearer fleet-machine-production-canary",
    "Bearer fleet-machine-runtime-update-canary",
];

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
fn production_store(
    root: &std::path::Path,
) -> Result<(EncryptedCredentialStore, FileWriteTrace), Box<dyn Error>> {
    // Exercise the production opener with the injected test boundary before tracing writes.
    let opened = EncryptedCredentialStore::open_for_process_with_keychain(
        root,
        &DeterministicTestKeychainAccess,
    )?;
    assert_eq!(
        opened.status(),
        crate::encrypted_credential_store::EncryptedCredentialStoreStatus::Ready
    );
    drop(opened);
    let trace = FileWriteTrace::default();
    let files = FileSecretStore::open_with_write_trace(root, trace.clone())?;
    let key = load_or_create_pooled_credential_data_key(&files, &DeterministicTestKeychainAccess)?;
    Ok((EncryptedCredentialStore::new(files, key), trace))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn fleet_generic_dispatch_encrypts_access_and_machine_secrets_before_every_file_write() -> TestResult
{
    let root = tempfile::tempdir()?;
    let (store, trace) = production_store(root.path())?;
    let runtime = RuntimeCredentialStore::from(store.clone());
    for (purpose, plaintext, updated) in [
        ("fleet_access_client_secret", CANARIES[0], CANARIES[1]),
        ("fleet_machine_credential", CANARIES[2], CANARIES[3]),
    ] {
        let key = SecretKey::new(format!("{purpose}.{INSTALLATION}.{GENERATION}"))?;
        let initial_events = trace.events().len();
        store.write_secret(&key, &SecretString::new(plaintext))?;
        let after_direct = assert_ciphertext_write(root.path(), &trace, initial_events, &key)?;
        assert_eq!(runtime.read_secret(&key)?.expose_secret(), plaintext);
        runtime.write_secret(&key, &SecretString::new(updated))?;
        assert_eq!(store.read_secret(&key)?.expose_secret(), updated);
        assert_eq!(runtime.read_secret(&key)?.expose_secret(), updated);
        assert_ciphertext_write(root.path(), &trace, after_direct, &key)?;
        runtime.delete_staged(&key)?;
        assert!(!root.path().join(format!("{}.v2", key.as_str())).exists());
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
fn assert_ciphertext_write(
    root: &std::path::Path,
    trace: &FileWriteTrace,
    after: usize,
    key: &SecretKey,
) -> Result<usize, Box<dyn Error>> {
    let events = trace.events();
    let new_events = events.get(after..).ok_or("write trace moved backwards")?;
    let mut temporary_writes = 0;
    for event in new_events {
        if let FileWriteTraceEvent::TemporaryFileWritten { contents, .. } = event {
            temporary_writes += 1;
            assert_no_plaintext(contents);
        }
    }
    assert!(
        temporary_writes > 0,
        "secret write emitted no temporary-write evidence"
    );
    assert!(!root.join(format!("{}.secret", key.as_str())).exists());
    assert_no_plaintext(&std::fs::read(root.join(format!("{}.v2", key.as_str())))?);
    Ok(events.len())
}

fn assert_no_plaintext(bytes: &[u8]) {
    for canary in CANARIES {
        assert!(
            !bytes
                .windows(canary.len())
                .any(|window| window == canary.as_bytes()),
            "Fleet plaintext reached a production file write"
        );
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn unmarked_file_store_cannot_write_fleet_secrets() -> TestResult {
    let root = tempfile::tempdir()?;
    let store = FileSecretStore::open(root.path())?;
    let key = SecretKey::new(format!(
        "fleet_machine_credential.{INSTALLATION}.{GENERATION}"
    ))?;
    assert!(
        store
            .write_secret(&key, &SecretString::new("must-remain-secret"))
            .is_err()
    );
    assert!(
        !root
            .path()
            .join(format!("{}.secret", key.as_str()))
            .exists()
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn central_signing_and_handoff_secrets_use_encryption_and_refuse_unmarked_plaintext() -> TestResult
{
    use crate::central_credentials::CentralSecretKind;
    let root = tempfile::tempdir()?;
    let (store, trace) = production_store(root.path())?;
    let files = FileSecretStore::open(root.path())?;
    for kind in [
        CentralSecretKind::SigningKey,
        CentralSecretKind::OwnerCredential,
        CentralSecretKind::EnrollmentCredential,
    ] {
        let key = kind.secret_key()?;
        assert!(
            files
                .write_secret(&key, &SecretString::new(CANARIES[0]))
                .is_err()
        );
        let before = trace.events().len();
        store.write_staged(&key, &SecretString::new(CANARIES[0]))?;
        assert_ciphertext_write(root.path(), &trace, before, &key)?;
        assert_eq!(store.read_secret(&key)?.expose_secret(), CANARIES[0]);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
fn fleet_key_validation_and_generic_dispatch_fail_closed_for_every_backend() -> TestResult {
    use crate::fleet_secret_key::{FleetSecretKey, FleetSecretKind};
    let root = tempfile::tempdir()?;
    let (encrypted, _) = production_store(root.path())?;
    let runtime = RuntimeCredentialStore::from(encrypted.clone());
    let plain = FileSecretStore::open(root.path())?;
    for name in [
        "fleet_unknown",
        "fleet_machine_credential",
        "fleet_machine_credential.invalid.invalid",
        "fleet_access_client_secret.550E8400-e29b-41d4-a716-446655440000.550e8400-e29b-41d4-a716-446655440001",
    ] {
        let key = SecretKey::new(name)?;
        assert!(FleetSecretKey::parse(&key).is_err());
        for backend in [
            &encrypted as &dyn SecretStore,
            &runtime as &dyn SecretStore,
            &plain as &dyn SecretStore,
        ] {
            assert!(
                backend
                    .write_secret(&key, &SecretString::new("canary"))
                    .is_err()
            );
            assert!(backend.read_secret(&key).is_err());
            assert!(backend.delete_staged(&key).is_err());
        }
        assert!(!root.path().join(format!("{name}.secret")).exists());
    }
    let key = FleetSecretKey::new(
        FleetSecretKind::AccessClientSecret,
        INSTALLATION,
        GENERATION,
    )?;
    assert_eq!(FleetSecretKey::parse(&key.secret_key()?)?, Some(key));
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn fleet_ciphertext_authenticates_purpose_installation_and_generation() -> TestResult {
    use crate::fleet_secret_key::{FleetSecretKey, FleetSecretKind};
    let root = tempfile::tempdir()?;
    let (store, _) = production_store(root.path())?;
    let original = FleetSecretKey::new(
        FleetSecretKind::AccessClientSecret,
        INSTALLATION,
        GENERATION,
    )?
    .secret_key()?;
    store.write_secret(&original, &SecretString::new("fleet-purpose-bound-canary"))?;
    let encrypted = std::fs::read(root.path().join(format!("{}.v2", original.as_str())))?;
    for identity in [
        FleetSecretKey::new(FleetSecretKind::MachineCredential, INSTALLATION, GENERATION)?,
        FleetSecretKey::new(FleetSecretKind::AccessClientSecret, GENERATION, GENERATION)?,
        FleetSecretKey::new(
            FleetSecretKind::AccessClientSecret,
            INSTALLATION,
            INSTALLATION,
        )?,
    ] {
        let other = identity.secret_key()?;
        std::fs::write(
            root.path().join(format!("{}.v2", other.as_str())),
            &encrypted,
        )?;
        assert!(store.read_secret(&other).is_err());
    }
    assert_eq!(
        store.read_secret(&original)?.expose_secret(),
        "fleet-purpose-bound-canary"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn fleet_key_unavailable_and_migration_incomplete_never_fall_back_to_plaintext() -> TestResult {
    use crate::model::CredentialMigrationFailure;
    let root = tempfile::tempdir()?;
    let key = SecretKey::new(format!(
        "fleet_machine_credential.{INSTALLATION}.{GENERATION}"
    ))?;
    let unavailable =
        EncryptedCredentialStore::key_unavailable(FileSecretStore::open(root.path())?);
    let incomplete = EncryptedCredentialStore::migration_incomplete(
        FileSecretStore::open(root.path())?,
        Vec::new(),
        CredentialMigrationFailure::MarkerWriteFailed,
    );
    for store in [unavailable, incomplete] {
        assert!(
            store
                .write_secret(&key, &SecretString::new("unavailable-canary"))
                .is_err()
        );
        assert!(store.read_secret(&key).is_err());
        assert!(store.delete_staged(&key).is_err());
    }
    assert!(
        !root
            .path()
            .join(format!("{}.secret", key.as_str()))
            .exists()
    );
    assert!(!root.path().join(format!("{}.v2", key.as_str())).exists());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn account_enumeration_pruning_and_reopen_preserve_fleet_generations() -> TestResult {
    use crate::account_tokens::provider_credential_bundle_key;
    use codex_router_core::{ids::AccountId, provider::Provider};
    let root = tempfile::tempdir()?;
    let (store, _) = production_store(root.path())?;
    let fleet = SecretKey::new(format!(
        "fleet_machine_credential.{INSTALLATION}.{GENERATION}"
    ))?;
    let next = SecretKey::new(format!(
        "fleet_machine_credential.{INSTALLATION}.{INSTALLATION}"
    ))?;
    store.write_secret(&fleet, &SecretString::new("fleet-old-generation"))?;
    store.write_secret(&next, &SecretString::new("fleet-next-generation"))?;
    let account = AccountId::new("fleet-enumeration-account")?;
    for generation in [1, 2] {
        for provider in [Provider::Openai, Provider::Claude] {
            store.write_secret(
                &provider_credential_bundle_key(provider, &account, generation)?,
                &SecretString::new("account-characterization"),
            )?;
        }
    }
    let files = FileSecretStore::open(root.path())?;
    assert_eq!(files.list_pooled_credential_files("v2")?.len(), 4);
    assert!(files.has_any_v2_files()?);
    assert_eq!(
        store.prune_obsolete_generations(Provider::Openai, &account, 2)?,
        vec![1]
    );
    assert_eq!(
        store.read_secret(&fleet)?.expose_secret(),
        "fleet-old-generation"
    );
    let reopened = EncryptedCredentialStore::open_for_process_with_keychain(
        root.path(),
        &DeterministicTestKeychainAccess,
    )?;
    assert_eq!(
        reopened.status(),
        crate::encrypted_credential_store::EncryptedCredentialStoreStatus::Ready
    );
    assert_eq!(
        reopened.read_secret(&next)?.expose_secret(),
        "fleet-next-generation"
    );
    reopened.delete_staged(&next)?;
    assert_eq!(
        reopened.read_secret(&fleet)?.expose_secret(),
        "fleet-old-generation"
    );
    for provider in [Provider::Openai, Provider::Claude] {
        assert_eq!(
            reopened
                .read_secret(&provider_credential_bundle_key(provider, &account, 2)?)?
                .expose_secret(),
            "account-characterization"
        );
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[cfg(debug_assertions)]
#[test]
fn declared_debug_is_the_only_fleet_plaintext_path_and_production_refuses_it() -> TestResult {
    let root = tempfile::tempdir()?;
    let files = FileSecretStore::initialize_debug_plaintext(root.path())?;
    let runtime = RuntimeCredentialStore::DebugPlaintext(files);
    let key = SecretKey::new(format!(
        "fleet_access_client_secret.{INSTALLATION}.{GENERATION}"
    ))?;
    runtime.write_secret(&key, &SecretString::new("explicit-debug-fleet-canary"))?;
    assert_eq!(
        runtime.read_secret(&key)?.expose_secret(),
        "explicit-debug-fleet-canary"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join(format!("{}.secret", key.as_str())))?,
        "explicit-debug-fleet-canary"
    );
    assert!(
        EncryptedCredentialStore::open_for_process_with_keychain(
            root.path(),
            &DeterministicTestKeychainAccess
        )
        .is_err()
    );
    let reopened = FileSecretStore::open_declared_debug_plaintext(root.path())?
        .ok_or("debug declaration not reopened")?;
    assert_eq!(
        reopened.read_secret(&key)?.expose_secret(),
        "explicit-debug-fleet-canary"
    );
    reopened.delete_staged(&key)?;
    assert!(
        !root
            .path()
            .join(format!("{}.secret", key.as_str()))
            .exists()
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn fleet_only_ciphertext_prevents_replacing_a_missing_keychain_data_key() -> TestResult {
    use crate::keychain_data_key::{KeychainAccess, KeychainAccessError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct MissingKeychain {
        additions: AtomicUsize,
    }
    impl KeychainAccess for MissingKeychain {
        fn read_secret(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, KeychainAccessError> {
            Ok(None)
        }
        fn add_secret(&self, _: &str, _: &str, _: &[u8]) -> Result<(), KeychainAccessError> {
            self.additions.fetch_add(1, Ordering::SeqCst);
            Err(KeychainAccessError::TestKeychainAccessForbidden)
        }
    }
    let root = tempfile::tempdir()?;
    let (store, _) = production_store(root.path())?;
    let key = SecretKey::new(format!(
        "fleet_machine_credential.{INSTALLATION}.{GENERATION}"
    ))?;
    store.write_secret(&key, &SecretString::new("fleet-ciphertext-remains-owned"))?;
    let path = root.path().join(format!("{}.v2", key.as_str()));
    let before = std::fs::read(&path)?;
    let keychain = MissingKeychain {
        additions: AtomicUsize::new(0),
    };
    let reopened =
        EncryptedCredentialStore::open_for_process_with_keychain(root.path(), &keychain)?;
    assert_eq!(
        reopened.status(),
        crate::encrypted_credential_store::EncryptedCredentialStoreStatus::KeyUnavailable
    );
    assert!(reopened.read_secret(&key).is_err());
    assert_eq!(keychain.additions.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(path)?, before);
    Ok(())
}
