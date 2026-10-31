//! Read-only classification for the machine login's lifetime storage choice.
use std::{fmt, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    encrypted_credential_store::{EncryptedCredentialStore, EncryptedCredentialStoreStatus},
    keychain_data_key::KeychainAccess,
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
    pub fn open_read_only_with_keychain(root: &Path, keychain: &dyn KeychainAccess) -> Self {
        // Baseline path for the tests-first read-only contract: the existing
        // opener's mutation and loss of causes must be observed before replacement.
        match EncryptedCredentialStore::open_for_process_with_keychain(root, keychain) {
            Ok(store) => match store.status() {
                EncryptedCredentialStoreStatus::Ready => Self::Usable(store),
                EncryptedCredentialStoreStatus::KeyUnavailable => Self::Unusable {
                    cause: UnusableCause::KeychainUnavailable,
                },
                EncryptedCredentialStoreStatus::MigrationIncomplete { .. } => Self::Unusable {
                    cause: UnusableCause::MigrationIncomplete,
                },
            },
            Err(_) => Self::Unusable {
                cause: UnusableCause::MetadataUnavailable,
            },
        }
    }
}
