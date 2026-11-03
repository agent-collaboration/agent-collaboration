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
    pub access: FleetAccess,
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FleetAccess {
    Cloudflare(FleetAccessReference),
    SelfHosted(SelfHostedAccessReference),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SelfHostedAccessReference {
    SelfHosted { assertion_key: String },
}
impl From<FleetAccessReference> for FleetAccess {
    fn from(value: FleetAccessReference) -> Self {
        Self::Cloudflare(value)
    }
}
impl FleetAccess {
    fn key_and_kind(&self) -> (&str, FleetSecretKind) {
        match self {
            Self::Cloudflare(reference) => (
                &reference.client_secret_key,
                FleetSecretKind::AccessClientSecret,
            ),
            Self::SelfHosted(SelfHostedAccessReference::SelfHosted { assertion_key }) => {
                (assertion_key, FleetSecretKind::MachineAssertion)
            }
        }
    }
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
        let (reference, kind) = self.access.key_and_kind();
        let access = SecretKey::new(reference).map_err(|_| invalid())?;
        let credential = SecretKey::new(&self.credential_key).map_err(|_| invalid())?;
        let access_identity = FleetSecretKey::parse(&access)
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        let credential_identity = FleetSecretKey::parse(&credential)
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        if access_identity.kind() != kind
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
            access: match &self.access {
                FleetAccess::Cloudflare(reference) => {
                    AccessServiceToken::new(&reference.client_id, access.expose_secret())
                }
                FleetAccess::SelfHosted(_) => {
                    AccessServiceToken::self_hosted(access.expose_secret())
                }
            }
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
        let _reset_publication = crate::enrollment_reset::record_pending_enrollment_reset(
            &crate::router_collaboration_directory(root),
        )?;
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
#[path = "fleet_enrollment_file_test.rs"]
mod tests;
