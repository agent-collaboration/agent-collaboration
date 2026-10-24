//! One private enrollment-file boundary for production Host startup and debug overrides.
use board_fleet_sync::{AccessServiceToken, BoardSessionInputs};
use codex_router_secret_store::{
    SecretStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    model::SecretKey,
    runtime_credential_store::RuntimeCredentialStore,
};
use fleet_sync_protocol::{PeerId, ReceiverId};
use fleet_sync_session::{Endpoint, PeerCredential};
use serde::{Deserialize, Serialize};
use std::{
    io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetEnrollmentDocument {
    pub endpoint: String,
    pub access: FleetAccessReference,
    pub credential_key: String,
    pub peer: PeerId,
    pub receiver: ReceiverId,
    pub relay: PeerId,
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetAccessReference {
    pub client_id: String,
    pub client_secret_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetEnrollmentMetadata {
    pub endpoint: String,
    pub peer: PeerId,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl FleetEnrollmentDocument {
    pub fn metadata(&self) -> FleetEnrollmentMetadata {
        FleetEnrollmentMetadata {
            endpoint: self.endpoint.clone(),
            peer: self.peer,
            expires_at: self.expires_at,
        }
    }
    pub fn secret_keys(&self) -> io::Result<(SecretKey, SecretKey)> {
        let invalid = || io::Error::other("invalid Fleet enrollment secret references");
        let access = SecretKey::new(&self.access.client_secret_key).map_err(|_| invalid())?;
        let credential = SecretKey::new(&self.credential_key).map_err(|_| invalid())?;
        let access_identity = FleetSecretKey::parse(&access)
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        let credential_identity = FleetSecretKey::parse(&credential)
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        if access_identity.kind() != FleetSecretKind::AccessClientSecret
            || credential_identity.kind() != FleetSecretKind::MachineCredential
            || access_identity.installation() != credential_identity.installation()
            || access_identity.generation() != credential_identity.generation()
        {
            return Err(invalid());
        }
        Ok((access, credential))
    }
    pub fn session_inputs(&self, store: &dyn SecretStore) -> io::Result<BoardSessionInputs> {
        let (access_key, credential_key) = self.secret_keys()?;
        let access = store.read_secret(&access_key).map_err(io::Error::other)?;
        let credential = store
            .read_secret(&credential_key)
            .map_err(io::Error::other)?;
        Ok(BoardSessionInputs {
            endpoint: Endpoint::new(&self.endpoint).map_err(io::Error::other)?,
            access: AccessServiceToken::new(&self.access.client_id, access.expose_secret())
                .map_err(io::Error::other)?,
            credential: PeerCredential::new(credential.expose_secret().to_owned())
                .map_err(io::Error::other)?,
            peer: self.peer,
            receiver: self.receiver,
            relay: self.relay,
        })
    }
}

/// Reads an explicit enrollment file inside its owning Router root.
pub fn load_fleet_enrollment(path: &Path, root: &Path) -> io::Result<BoardSessionInputs> {
    let document = read_fleet_enrollment_document(path, root)?;
    let store = open_fleet_enrollment_store(root)?;
    document.session_inputs(&store)
}

/// Reads and validates the metadata and typed secret references, without opening a secret store.
pub fn read_fleet_enrollment_document(
    path: &Path,
    root: &Path,
) -> io::Result<FleetEnrollmentDocument> {
    let invalid = || {
        io::Error::other("fleet enrollment requires a private owner-only file in the Router root")
    };
    if !path.is_absolute() {
        return Err(invalid());
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || !path.canonicalize()?.starts_with(root.canonicalize()?)
    {
        return Err(invalid());
    }
    let document: FleetEnrollmentDocument = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|_| io::Error::other("invalid fleet enrollment inputs"))?;
    document.secret_keys()?;
    Ok(document)
}

pub fn open_fleet_enrollment_store(root: &Path) -> io::Result<RuntimeCredentialStore> {
    let secret_root = crate::HostCoordinationPaths::for_router_root(root)
        .router_secret_root()
        .ok_or_else(|| io::Error::other("Router secret root unavailable"))?;
    if let Some(store) =
        codex_router_secret_store::file_backend::FileSecretStore::open_declared_debug_plaintext(
            &secret_root,
        )
        .map_err(io::Error::other)?
    {
        #[cfg(debug_assertions)]
        return Ok(RuntimeCredentialStore::DebugPlaintext(store));
        #[cfg(not(debug_assertions))]
        {
            let _ = store;
            return Err(io::Error::other("debug plaintext credentials unavailable"));
        }
    }
    #[cfg(any(test, feature = "keychain-test-support"))]
    return codex_router_secret_store::test_support::open_encrypted_credential_store(secret_root)
        .map(Into::into)
        .map_err(io::Error::other);
    #[cfg(not(any(test, feature = "keychain-test-support")))]
    codex_router_secret_store::encrypted_credential_store::EncryptedCredentialStore::open_production_for_process(secret_root).map(Into::into).map_err(io::Error::other)
}

#[derive(Debug, thiserror::Error)]
pub enum FleetEnrollmentPublishError {
    #[error("enrollment metadata was not published: {0}")]
    BeforeRename(#[source] io::Error),
    #[error(
        "enrollment metadata was published but its directory could not be synced: {0}; inspect fleet status before retrying"
    )]
    Renamed(#[source] io::Error),
}

/// Publishes metadata only. Newly staged secrets must remain live after the rename succeeds.
pub fn persist_fleet_enrollment(
    root: &Path,
    document: &FleetEnrollmentDocument,
    store: &dyn SecretStore,
) -> Result<(), FleetEnrollmentPublishError> {
    persist_fleet_enrollment_with_rename(root, document, store, |from, to| {
        std::fs::rename(from, to)
    })
}

fn persist_fleet_enrollment_with_rename(
    root: &Path,
    document: &FleetEnrollmentDocument,
    store: &dyn SecretStore,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), FleetEnrollmentPublishError> {
    use std::{
        io::Write,
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    };
    document
        .session_inputs(store)
        .map_err(FleetEnrollmentPublishError::BeforeRename)?;
    let path = crate::router_fleet_enrollment_file(root);
    let parent = path.parent().ok_or_else(|| {
        FleetEnrollmentPublishError::BeforeRename(io::Error::other("enrollment parent unavailable"))
    })?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(FleetEnrollmentPublishError::BeforeRename)?;
    let parent_metadata =
        std::fs::symlink_metadata(parent).map_err(FleetEnrollmentPublishError::BeforeRename)?;
    if !root.is_absolute()
        || !parent_metadata.is_dir()
        || parent_metadata.uid() != rustix::process::geteuid().as_raw()
        || !parent
            .canonicalize()
            .map_err(FleetEnrollmentPublishError::BeforeRename)?
            .starts_with(
                root.canonicalize()
                    .map_err(FleetEnrollmentPublishError::BeforeRename)?,
            )
    {
        return Err(FleetEnrollmentPublishError::BeforeRename(io::Error::other(
            "enrollment parent must be inside the owning Router root",
        )));
    }
    let temporary = parent.join(format!(
        ".enrollment-{}.tmp",
        collaboration_protocol::OperationId::generate().as_str()
    ));
    let before_rename = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(document).map_err(io::Error::other)?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        rename(&temporary, &path)
    })();
    if let Err(error) = before_rename {
        let _cleanup = std::fs::remove_file(&temporary);
        return Err(FleetEnrollmentPublishError::BeforeRename(error));
    }
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(FleetEnrollmentPublishError::Renamed)
}

pub(crate) fn load_router_fleet_enrollment(root: &Path) -> io::Result<Option<BoardSessionInputs>> {
    let path = crate::router_fleet_enrollment_file(root);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => load_fleet_enrollment(&path, root).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "fleet_enrollment_publication_tests.rs"]
mod publication_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn write_fixture(root: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
        use codex_router_core::redaction::SecretString;
        let store = open_fleet_enrollment_store(root)?;
        let installation = "550e8400-e29b-41d4-a716-446655440000";
        let generation = "550e8400-e29b-41d4-a716-446655440001";
        let access = FleetSecretKey::new(
            FleetSecretKind::AccessClientSecret,
            installation,
            generation,
        )?
        .secret_key()?;
        let credential =
            FleetSecretKey::new(FleetSecretKind::MachineCredential, installation, generation)?
                .secret_key()?;
        store.write_secret(&access, &SecretString::new("fixture-secret"))?;
        store.write_secret(&credential, &SecretString::new("Bearer fixture-machine"))?;
        let file = root.join("fixture-session.json");
        std::fs::write(
            &file,
            serde_json::to_vec(&serde_json::json!({
                "endpoint":"ws://127.0.0.1:1/session",
                "access":{"clientId":"fixture-access", "clientSecretKey":access.as_str()},
                "credentialKey":credential.as_str(),
                "peer":uuid::Uuid::now_v7().to_string(),
                "receiver":uuid::Uuid::now_v7().to_string(),
                "relay":"0192a1b2-c3d4-7000-8000-000000000003",
            }))?,
        )?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn production_metadata_refuses_inline_access_and_machine_secrets() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let file = write_fixture(root.path())?;
        std::fs::write(
            &file,
            serde_json::to_vec(&serde_json::json!({
                "endpoint":"ws://127.0.0.1:1/session",
                "access":{"clientId":"fixture-access", "clientSecret":"fixture-secret"},
                "credential":"Bearer fixture-machine",
                "peer":uuid::Uuid::now_v7().to_string(),
                "receiver":"0192a1b2-c3d4-7000-8000-000000000002",
                "relay":"0192a1b2-c3d4-7000-8000-000000000003"
            }))?,
        )?;
        assert!(
            load_fleet_enrollment(&file, root.path()).is_err(),
            "production metadata accepted inline plaintext secrets"
        );
        Ok(())
    }

    #[test]
    fn enrollment_accepts_optional_typed_expiry_without_owning_expiry_policy()
    -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let file = write_fixture(root.path())?;
        let mut fixture: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
        for expiry in [
            "2000-01-01T00:00:00Z",
            "2033-05-18T03:33:20Z",
            "2033-05-18T04:33:20+01:00",
        ] {
            fixture["expiresAt"] = serde_json::json!(expiry);
            std::fs::write(&file, serde_json::to_vec(&fixture)?)?;
            load_fleet_enrollment(&file, &root.path().canonicalize()?)?;
        }
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn private_fixture_preserves_typed_peer_receiver_relay_and_credentials()
    -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let file = write_fixture(root.path())?;
        let raw: FleetEnrollmentDocument = serde_json::from_slice(&std::fs::read(&file)?)?;
        let loaded = load_fleet_enrollment(&file, &root.path().canonicalize()?)?;
        assert_eq!(loaded.peer, raw.peer);
        assert_eq!(loaded.receiver, raw.receiver);
        assert_eq!(loaded.relay, raw.relay);
        assert_eq!(loaded.endpoint.as_str(), raw.endpoint);
        assert_eq!(loaded.credential.expose(), "Bearer fixture-machine");
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn fixture_requires_an_explicit_canonical_relay_instead_of_deriving_one()
    -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let owned = root.path().canonicalize()?;
        let file = write_fixture(root.path())?;
        let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
        for relay in [
            None,
            Some(serde_json::json!("550e8400-e29b-41d4-a716-446655440000")),
        ] {
            let mut invalid = original.clone();
            let object = invalid.as_object_mut().ok_or("fixture is not an object")?;
            object.remove("relay");
            if let Some(relay) = relay {
                object.insert("relay".to_owned(), relay);
            }
            std::fs::write(&file, serde_json::to_vec(&invalid)?)?;
            assert!(load_fleet_enrollment(&file, &owned).is_err());
        }
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn fixture_refuses_public_files_external_paths_and_noncanonical_identity()
    -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let owned = root.path().canonicalize()?;
        let file = write_fixture(root.path())?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
        assert!(load_fleet_enrollment(&file, &owned).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
        let other = tempfile::tempdir()?;
        assert!(load_fleet_enrollment(&file, &other.path().canonicalize()?).is_err());
        let mut invalid: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
        let object = invalid.as_object_mut().ok_or("fixture is not an object")?;
        object.insert(
            "peer".to_owned(),
            serde_json::json!("550e8400-e29b-41d4-a716-446655440000"),
        );
        std::fs::write(&file, serde_json::to_vec(&invalid)?)?;
        assert!(load_fleet_enrollment(&file, &owned).is_err());
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn absent_router_enrollment_is_the_only_normal_fallback() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        assert!(load_router_fleet_enrollment(root.path())?.is_none());
        std::fs::create_dir(root.path().join("fleet"))?;
        assert!(load_router_fleet_enrollment(root.path())?.is_none());
        std::os::unix::fs::symlink(
            root.path().join("absent.json"),
            root.path().join("fleet/enrollment.json"),
        )?;
        assert!(load_router_fleet_enrollment(root.path()).is_err());
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn enrollment_refuses_a_symlink_even_when_its_target_is_private() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let file = write_fixture(root.path())?;
        let link = root.path().join("linked-enrollment.json");
        std::os::unix::fs::symlink(file, &link)?;
        assert!(load_fleet_enrollment(&link, root.path()).is_err());
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn enrollment_refuses_relative_paths_and_directories() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        assert!(load_fleet_enrollment(Path::new("enrollment.json"), root.path()).is_err());
        assert!(load_fleet_enrollment(root.path(), root.path()).is_err());
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn enrollment_refuses_a_parent_link_that_escapes_the_router_root() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let external = tempfile::tempdir()?;
        let file = write_fixture(external.path())?;
        let link = root.path().join("external");
        std::os::unix::fs::symlink(external.path(), &link)?;
        assert!(
            load_fleet_enrollment(&link.join(file.file_name().ok_or("filename")?), root.path())
                .is_err()
        );
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn enrollment_refuses_missing_unknown_and_malformed_fields_without_echoing_secrets()
    -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let file = write_fixture(root.path())?;
        let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
        for field in [
            "endpoint",
            "access",
            "credentialKey",
            "peer",
            "receiver",
            "relay",
        ] {
            let mut invalid = original.clone();
            invalid.as_object_mut().ok_or("object")?.remove(field);
            std::fs::write(&file, serde_json::to_vec(&invalid)?)?;
            assert!(
                load_fleet_enrollment(&file, root.path()).is_err(),
                "missing {field} accepted"
            );
        }
        for (field, value) in [
            ("unknown", serde_json::json!(true)),
            ("expiresAt", serde_json::json!("not-a-timestamp")),
            ("expiresAt", serde_json::json!(123)),
            (
                "peer",
                serde_json::json!("550e8400-e29b-41d4-a716-446655440000"),
            ),
            ("receiver", serde_json::json!("invalid-uuid")),
            ("relay", serde_json::json!("invalid-uuid")),
            ("endpoint", serde_json::json!("not-a-url")),
            ("credentialKey", serde_json::json!("")),
            (
                "access",
                serde_json::json!({"clientId":"", "clientSecretKey":original["access"]["clientSecretKey"]}),
            ),
            (
                "access",
                serde_json::json!({"clientId":"client", "clientSecretKey":""}),
            ),
            (
                "access",
                serde_json::json!({"clientId":"client", "clientSecretKey":"secret-sentinel\r\ninvalid"}),
            ),
            (
                "access",
                serde_json::json!({"clientId":"client", "clientSecretKey":original["access"]["clientSecretKey"], "unknown":true}),
            ),
        ] {
            let mut invalid = original.clone();
            invalid[field] = value;
            std::fs::write(&file, serde_json::to_vec(&invalid)?)?;
            let error = load_fleet_enrollment(&file, root.path())
                .err()
                .ok_or("invalid enrollment accepted")?;
            assert!(!error.to_string().contains("secret-sentinel"));
            assert!(!error.to_string().contains("fixture-machine"));
        }
        std::fs::write(&file, "{invalid-json-secret-sentinel")?;
        let error = load_fleet_enrollment(&file, root.path())
            .err()
            .ok_or("malformed JSON accepted")?;
        assert!(!error.to_string().contains("secret-sentinel"));
        Ok(())
    }
}
