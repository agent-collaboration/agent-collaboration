use chrono::{DateTime, Utc};
use collaboration_replication_protocol::{
    Epoch, InstallationId, ProjectId, ReplicationPosition, RequestId, SchemaIdentity, SessionAnswer,
};
use serde::{Deserialize, Serialize};

use crate::StoreError;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectCallId(RequestId);

impl ObjectCallId {
    pub fn new(
        value: impl Into<String>,
    ) -> Result<Self, collaboration_replication_protocol::IdentityError> {
        RequestId::new(value).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegistryTime(pub DateTime<Utc>);

#[derive(Clone, Debug, Default)]
pub struct IdBatch {
    pub call_ids: Vec<ObjectCallId>,
    /// Stable for one physical registry actor; newly minted after restart.
    pub registry_run: Option<crate::RegistryRunId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ObjectAddress {
    Copy(ProjectId),
    Registry,
    Fleet,
    Connection(InstallationId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FenceExpectation {
    pub epoch: Epoch,
    pub acked: ReplicationPosition,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyStatus {
    pub imported: Option<crate::ImportedCopy>,
    pub project: ProjectId,
    pub fence: Epoch,
    pub holder: Option<InstallationId>,
    pub start: ReplicationPosition,
    pub acked: ReplicationPosition,
    pub schema: SchemaIdentity,
    pub archived: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum FenceResult {
    Fenced {
        old_epoch: Epoch,
        new_epoch: Epoch,
        start: ReplicationPosition,
    },
    PreviewStale {
        acked: ReplicationPosition,
    },
    SchemaTooNew {
        store: SchemaIdentity,
        supported: u64,
    },
    FenceNotAdvanced {
        current_epoch: Epoch,
    },
    NotInitialized,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum CentralError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("invalid core input: {message}")]
    InvalidInput { message: String },
    #[error("encoding failure: {message}")]
    Encoding { message: String },
    #[error("incoming call must produce exactly one matching typed Return")]
    ReplyContract,
    #[error("object call edge refused: {from:?} -> {to:?}")]
    CallEdgeRefused {
        from: crate::CallerKind,
        to: crate::CallerKind,
    },
    #[error("storage capacity exhausted; rollback required")]
    CopyFull { call_id: ObjectCallId },
    #[error("import refused; rollback required")]
    ImportRejected {
        call_id: ObjectCallId,
        reason: collaboration_replication_protocol::ProjectImportRefusal,
    },
    #[error("copy bookkeeping unavailable: {failure}")]
    CopyMigrationPending {
        reply: crate::CopyMigrationReply,
        #[source]
        failure: crate::CopyMigrationFailure,
    },
}

impl CentralError {
    #[must_use]
    pub fn rollback_trace(&self) -> Vec<crate::WireEffect> {
        self.copy_effects_after_rollback()
            .into_iter()
            .map(Into::into)
            .collect()
    }
    #[must_use]
    pub fn copy_effects_after_rollback(&self) -> Vec<crate::CopyEffect> {
        use crate::{CopyEffect, CopyResult};
        match self {
            Self::CopyMigrationPending {
                reply: crate::CopyMigrationReply::Migrate(call_id),
                failure,
            } => vec![CopyEffect::Return {
                call_id: call_id.clone(),
                result: CopyResult::Migrated(crate::CopyMigrationResult::Failed {
                    version: failure.version,
                    reason: failure.reason(),
                }),
            }],
            Self::CopyMigrationPending {
                reply: crate::CopyMigrationReply::Call(call_id),
                failure,
            } => vec![CopyEffect::Return {
                call_id: call_id.clone(),
                result: CopyResult::Unavailable(if failure.is_full() {
                    collaboration_replication_protocol::CopyUnavailableReason::StorageFull
                } else {
                    collaboration_replication_protocol::CopyUnavailableReason::MigrationPending {
                        kind: collaboration_replication_protocol::BookkeepingKind::Copy,
                        version: failure.version,
                    }
                }),
            }],
            Self::ImportRejected { call_id, reason } => vec![CopyEffect::Return {
                call_id: call_id.clone(),
                result: CopyResult::ImportSeed(crate::ImportSeedResult {
                    answer: collaboration_replication_protocol::ProjectImportAnswer::Refused(
                        reason.clone(),
                    ),
                    unheld: None,
                }),
            }],
            Self::CopyFull { call_id } => vec![CopyEffect::Return {
                call_id: call_id.clone(),
                result: CopyResult::ApplyPiece(SessionAnswer::Refused(
                    collaboration_replication_protocol::SessionRefusal::CentralCopyFull,
                )),
            }],
            _ => Vec::new(),
        }
    }
}
pub fn validate_call_returns(
    input: &crate::WireInput,
    effects: &[crate::WireEffect],
) -> Result<(), CentralError> {
    if let crate::WireInput::Call { call_id, call, .. } = input {
        let mut returns = effects.iter().filter_map(|effect| match effect {
            crate::WireEffect::Return { call_id, result } => Some((call_id, result)),
            _ => None,
        });
        let Some((returned, result)) = returns.next() else {
            return Err(CentralError::ReplyContract);
        };
        if returned != call_id || returns.next().is_some() || !call.matches_result(result) {
            return Err(CentralError::ReplyContract);
        }
    }
    Ok(())
}
