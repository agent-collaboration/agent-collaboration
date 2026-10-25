use crate::ProjectStorageError;
use message_board::{ActorOrigin, RecordId, RecordedActor};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportRowKind {
    Actor,
    EpochLine,
    TaskLink,
    PostReference,
    ThreadParticipation,
    WorkTrailTag,
    WorkTrailLink,
    ReadMark,
    ThreadWatch,
    ProjectWatch,
    SubscriptionMembership,
    FeedEntry,
    HandoffFrontier,
}

pub trait ImportRowKeys: Send + Sync {
    fn row_key(&self, kind: ImportRowKind, stable_parts: &[&str]) -> String;
}

pub(crate) enum RecordAdmission {
    Live {
        origin: ActorOrigin,
    },
    Import {
        provenance: fleet_admission::ImportProvenance,
        row_keys: Arc<dyn ImportRowKeys>,
    },
}

impl RecordAdmission {
    pub(crate) fn row_key(
        &self,
        kind: ImportRowKind,
        stable_parts: &[&str],
    ) -> Result<RecordId, ProjectStorageError> {
        match self {
            Self::Live { .. } => Ok(RecordId::generate()),
            Self::Import { row_keys, .. } => {
                RecordId::try_from(row_keys.row_key(kind, stable_parts))
                    .map_err(|_| ProjectStorageError::InvalidImportRowKey { kind })
            }
        }
    }

    pub(crate) fn origin(&self) -> ActorOrigin {
        match self {
            Self::Live { origin } => origin.clone(),
            Self::Import { provenance, .. } => ActorOrigin {
                installation: provenance.installation.clone(),
                machine: provenance.machine.clone(),
            },
        }
    }

    pub(crate) fn admits_actor(&self, actor: &RecordedActor) -> bool {
        match self {
            Self::Live { origin } => match actor {
                RecordedActor::Router { installation, .. } => installation == &origin.installation,
                RecordedActor::Session { .. } | RecordedActor::Human { .. } => true,
            },
            Self::Import { .. } => true,
        }
    }

    pub(crate) fn admits_time(&self, time: message_board::RecordTime) -> bool {
        matches!(
            (self, time),
            (Self::Live { .. }, message_board::RecordTime::Router(_)) | (Self::Import { .. }, _)
        )
    }
}
