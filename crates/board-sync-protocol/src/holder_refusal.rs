use fleet_sync_protocol as wire;
use serde::{Deserialize, Deserializer, Serialize};

/// Presence and renewal observations keep their original field and meaning.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum HolderObservation {
    Presence {
        #[serde(deserialize_with = "deserialize_named_presence")]
        last_seen: Option<wire::SyncTime>,
    },
    Renewal {
        last_renewed_at: wire::SyncTime,
    },
}

fn deserialize_named_presence<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<wire::SyncTime>, D::Error> {
    Option::<wire::SyncTime>::deserialize(deserializer)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardHolderChangeRefusal {
    HolderUnreachable(HolderObservation),
    GateRefused {
        reason: wire::GateReason,
    },
    TransferInProgress {
        transfer: wire::TransferId,
    },
    PreviewStale,
    MigrationPending {
        migration: u64,
        state: wire::MigrationState,
    },
    SchemaTooNew {
        store: wire::SchemaIdentity,
        supported: Option<u64>,
    },
    AlreadyHeld {
        holder: wire::PeerId,
    },
    NotFound,
    Unavailable,
    RegistryEpochUnproven,
    Superseded {
        by: wire::TransferId,
    },
    Retired,
}

impl From<wire::HolderChangeRefusal> for BoardHolderChangeRefusal {
    fn from(value: wire::HolderChangeRefusal) -> Self {
        match value {
            wire::HolderChangeRefusal::HolderStale { last_renewed_at } => {
                Self::HolderUnreachable(HolderObservation::Renewal { last_renewed_at })
            }
            wire::HolderChangeRefusal::HolderUnreachable { last_seen } => {
                Self::HolderUnreachable(HolderObservation::Presence { last_seen })
            }
            wire::HolderChangeRefusal::GateRefused { reason } => Self::GateRefused { reason },
            wire::HolderChangeRefusal::TransferInProgress { transfer } => {
                Self::TransferInProgress { transfer }
            }
            wire::HolderChangeRefusal::PreviewStale => Self::PreviewStale,
            wire::HolderChangeRefusal::MigrationPending { migration, state } => {
                Self::MigrationPending { migration, state }
            }
            wire::HolderChangeRefusal::SchemaTooNew { store, supported } => {
                Self::SchemaTooNew { store, supported }
            }
            wire::HolderChangeRefusal::AlreadyHeld { holder } => Self::AlreadyHeld { holder },
            wire::HolderChangeRefusal::NotFound => Self::NotFound,
            wire::HolderChangeRefusal::Unavailable => Self::Unavailable,
            wire::HolderChangeRefusal::EpochUnprovable => Self::RegistryEpochUnproven,
            wire::HolderChangeRefusal::Superseded { by } => Self::Superseded { by },
            wire::HolderChangeRefusal::Retired => Self::Retired,
        }
    }
}

impl From<BoardHolderChangeRefusal> for wire::HolderChangeRefusal {
    fn from(value: BoardHolderChangeRefusal) -> Self {
        match value {
            BoardHolderChangeRefusal::HolderUnreachable(HolderObservation::Renewal {
                last_renewed_at,
            }) => Self::HolderStale { last_renewed_at },
            BoardHolderChangeRefusal::HolderUnreachable(HolderObservation::Presence {
                last_seen,
            }) => Self::HolderUnreachable { last_seen },
            BoardHolderChangeRefusal::GateRefused { reason } => Self::GateRefused { reason },
            BoardHolderChangeRefusal::TransferInProgress { transfer } => {
                Self::TransferInProgress { transfer }
            }
            BoardHolderChangeRefusal::PreviewStale => Self::PreviewStale,
            BoardHolderChangeRefusal::MigrationPending { migration, state } => {
                Self::MigrationPending { migration, state }
            }
            BoardHolderChangeRefusal::SchemaTooNew { store, supported } => {
                Self::SchemaTooNew { store, supported }
            }
            BoardHolderChangeRefusal::AlreadyHeld { holder } => Self::AlreadyHeld { holder },
            BoardHolderChangeRefusal::NotFound => Self::NotFound,
            BoardHolderChangeRefusal::Unavailable => Self::Unavailable,
            BoardHolderChangeRefusal::RegistryEpochUnproven => Self::EpochUnprovable,
            BoardHolderChangeRefusal::Superseded { by } => Self::Superseded { by },
            BoardHolderChangeRefusal::Retired => Self::Retired,
        }
    }
}
