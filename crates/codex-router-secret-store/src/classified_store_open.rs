//! Read-only classification for the machine login's lifetime storage choice.
use std::{fmt, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    backend::SecretStore,
    encrypted_credential_store::EncryptedCredentialStore,
    file_backend::FileSecretStore,
    keychain_data_key::{
        ExistingCredentialDataKey, KeychainAccess, read_existing_pooled_credential_data_key,
        with_platform_keychain_for_production,
    },
    model::SecretStoreError,
};

/// The store observation made once when the Host starts.
pub enum ClassifiedCredentialStore {
    Usable(EncryptedCredentialStore),
    Absent,
    Unusable { cause: UnusableCause },
}

impl fmt::Debug for ClassifiedCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usable(_) => formatter.write_str("Usable"),
            Self::Absent => formatter.write_str("Absent"),
            Self::Unusable { cause } => formatter
                .debug_struct("Unusable")
                .field("cause", cause)
                .finish(),
        }
    }
}

/// Classified failures contain no backend text, item data or credential values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum UnusableCause {
    KeychainUnavailable,
    KeychainServiceRejected,
    KeyMissing,
    InvalidDataKey,
    InvalidStoreMetadata,
    MigrationIncomplete,
    InvalidCredentialData,
    MetadataUnavailable,
    UnsafeStoreRoot,
    DebugPlaintextConfigured,
    TestKeychainAccessForbidden,
}

impl ClassifiedCredentialStore {
    /// Observes this process's existing credential store through the platform seam.
    pub fn open_read_only_for_process(root: &Path) -> Self {
        match with_platform_keychain_for_production(|keychain| {
            Ok(Self::open_read_only_with_keychain(root, keychain))
        }) {
            Ok(classified) => classified,
            Err(error) => Self::Unusable {
                cause: classify_failure(&error),
            },
        }
    }

    /// Reads only existing files and key material; creates no root, lock or marker.
    pub fn open_read_only_with_keychain(root: &Path, keychain: &dyn KeychainAccess) -> Self {
        match observe_store(root, keychain) {
            Ok(classified) => classified,
            Err(cause) => Self::Unusable { cause },
        }
    }
}

fn observe_store(
    root: &Path,
    keychain: &dyn KeychainAccess,
) -> Result<ClassifiedCredentialStore, UnusableCause> {
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if !metadata.is_dir() => return Err(UnusableCause::UnsafeStoreRoot),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ClassifiedCredentialStore::Absent);
        }
        Err(_) => return Err(UnusableCause::MetadataUnavailable),
    }
    let files = FileSecretStore::open_read_only(root).map_err(|error| classify_failure(&error))?;
    FileSecretStore::reject_debug_plaintext_declaration(root)
        .map_err(|error| classify_failure(&error))?;
    let marker = files
        .has_format_v2_marker()
        .map_err(|error| classify_failure(&error))?;
    let encrypted = files
        .encrypted_keys_for_validation()
        .map_err(|error| classify_failure(&error))?;
    let legacy = files
        .list_pooled_credential_files("secret")
        .map_err(|error| classify_failure(&error))?;
    let temporary = files
        .list_pooled_credential_temporary_files()
        .map_err(|error| classify_failure(&error))?;
    let data_key = match read_existing_pooled_credential_data_key(&files, keychain)
        .map_err(|error| classify_failure(&error))?
    {
        ExistingCredentialDataKey::BackendAbsent => return Ok(ClassifiedCredentialStore::Absent),
        ExistingCredentialDataKey::MissingStore
            if !marker && encrypted.is_empty() && legacy.is_empty() && temporary.is_empty() =>
        {
            return Ok(ClassifiedCredentialStore::Absent);
        }
        ExistingCredentialDataKey::MissingStore => return Err(UnusableCause::InvalidStoreMetadata),
        ExistingCredentialDataKey::Ready(key) => key,
    };
    if !marker || !legacy.is_empty() || !temporary.is_empty() {
        return Err(UnusableCause::MigrationIncomplete);
    }
    let store = EncryptedCredentialStore::new(files, data_key);
    for key in encrypted {
        // Authentication failures are store damage, even when the damaged item
        // belongs to another credential family. The plaintext is never retained.
        store
            .read_secret(&key)
            .map_err(|error| classify_failure(&error))?;
    }
    Ok(ClassifiedCredentialStore::Usable(store))
}

fn classify_failure(error: &SecretStoreError) -> UnusableCause {
    match error {
        SecretStoreError::KeyUnavailable => UnusableCause::KeychainUnavailable,
        SecretStoreError::KeyMissing => UnusableCause::KeyMissing,
        SecretStoreError::InvalidDataKey => UnusableCause::InvalidDataKey,
        SecretStoreError::KeychainServiceRejected => UnusableCause::KeychainServiceRejected,
        SecretStoreError::TestKeychainAccessForbidden => UnusableCause::TestKeychainAccessForbidden,
        SecretStoreError::InvalidCredentialStoreMarker { .. } => {
            UnusableCause::InvalidStoreMetadata
        }
        SecretStoreError::StoreUnavailable(_) => UnusableCause::MigrationIncomplete,
        SecretStoreError::CodexHomePath { .. } | SecretStoreError::SymlinkPath { .. } => {
            UnusableCause::UnsafeStoreRoot
        }
        SecretStoreError::DebugPlaintextUnavailable
        | SecretStoreError::DebugPlaintextRootRejected
        | SecretStoreError::InvalidDebugPlaintextDeclaration => {
            UnusableCause::DebugPlaintextConfigured
        }
        SecretStoreError::Filesystem { .. } | SecretStoreError::CredentialStoreLock { .. } => {
            UnusableCause::MetadataUnavailable
        }
        SecretStoreError::InvalidCentralSecretKey
        | SecretStoreError::InvalidFleetSecretKey
        | SecretStoreError::FleetSecretRequiresEncryption
        | SecretStoreError::InvalidSecretKey { .. }
        | SecretStoreError::PooledCredentialRequiresEncryption { .. }
        | SecretStoreError::InvalidCredentialKey { .. }
        | SecretStoreError::UnexpectedCredentialEntry { .. }
        | SecretStoreError::UnknownCredentialProvider { .. }
        | SecretStoreError::RandomnessUnavailable(_)
        | SecretStoreError::TestCredentialKeyOnProductionRoot
        | SecretStoreError::TestCredentialKeyRootUnverifiable
        | SecretStoreError::InvalidCredentialEnvelope
        | SecretStoreError::CipherInitializationFailed
        | SecretStoreError::CredentialAuthenticationFailed
        | SecretStoreError::InvalidCredentialEncoding
        | SecretStoreError::InvalidSecretPayload { .. } => UnusableCause::InvalidCredentialData,
    }
}
