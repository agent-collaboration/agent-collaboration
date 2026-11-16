use super::store_adapter::corrupt_class;
use central_engine_model::ItemFailureClass;
use central_storage::{BatchOutcome, ConstraintKind, StoreFailure};
use workspace_model::WorkspaceId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LegacyUnitOutcome {
    Done,
    RemoteUnavailable { workspaces: Vec<WorkspaceId> },
    Corrupt { column: String },
    Rejected,
    ConstraintViolated { constraint: ConstraintKind },
    Superseded,
    Uncertain,
    CentralDown(StoreFailure),
}
impl From<StoreFailure> for LegacyUnitOutcome {
    fn from(failure: StoreFailure) -> Self {
        match failure {
            StoreFailure::Corrupt { reason } => match corrupt_class(reason) {
                ItemFailureClass::CorruptRow { column, .. } => Self::Corrupt { column },
                _ => unreachable!("corrupt_class returns CorruptRow"),
            },
            StoreFailure::AnswerLost => Self::Uncertain,
            failure @ (StoreFailure::Unavailable | StoreFailure::AlarmUnpublishable { .. }) => {
                Self::CentralDown(failure)
            }
        }
    }
}
impl LegacyUnitOutcome {
    pub(crate) fn batch(result: Result<BatchOutcome, StoreFailure>) -> Result<(), Self> {
        match result {
            Ok(BatchOutcome::Applied { .. }) => Ok(()),
            Ok(BatchOutcome::GuardFailed { .. }) => Err(Self::Superseded),
            Ok(BatchOutcome::StatementRejected { .. }) => Err(Self::Rejected),
            Ok(BatchOutcome::ConstraintViolated { kind, .. }) => {
                Err(Self::ConstraintViolated { constraint: kind })
            }
            Err(failure) => Err(failure.into()),
        }
    }
    pub(crate) fn failure_class(&self) -> Option<ItemFailureClass> {
        match self {
            Self::RemoteUnavailable { .. } => Some(ItemFailureClass::OtherStoreUnavailable),
            Self::Corrupt { column } => Some(ItemFailureClass::CorruptRow {
                column: column.clone(),
                raw_value: None,
            }),
            Self::Rejected => Some(ItemFailureClass::StatementRejected),
            Self::ConstraintViolated { constraint } => Some(ItemFailureClass::ConstraintViolated {
                constraint: *constraint,
            }),
            Self::Done | Self::Superseded | Self::Uncertain | Self::CentralDown(_) => None,
        }
    }
    /// Preserve the old request entry's result; the engine keeps the richer origin.
    pub(crate) fn into_request_failure(self) -> StoreFailure {
        match self {
            Self::Corrupt { column } => crate::workspace_decode_error::corrupt(&column),
            Self::CentralDown(failure) => failure,
            Self::Uncertain => StoreFailure::AnswerLost,
            Self::Done
            | Self::RemoteUnavailable { .. }
            | Self::Rejected
            | Self::ConstraintViolated { .. }
            | Self::Superseded => StoreFailure::Unavailable,
        }
    }
    /// Central outage wins. Recorded failures win over uncharged outcomes, and
    /// every unavailable remote is retained even if another workspace failed.
    pub(crate) fn combine(self, next: Self) -> Self {
        match (self, next) {
            (down @ Self::CentralDown(_), _) | (_, down @ Self::CentralDown(_)) => down,
            (
                Self::RemoteUnavailable { mut workspaces },
                Self::RemoteUnavailable { workspaces: more },
            ) => {
                workspaces.extend(more);
                workspaces.sort();
                workspaces.dedup();
                Self::RemoteUnavailable { workspaces }
            }
            (
                failure @ (Self::Corrupt { .. }
                | Self::Rejected
                | Self::ConstraintViolated { .. }
                | Self::RemoteUnavailable { .. }),
                _,
            ) => failure,
            (
                _,
                failure @ (Self::Corrupt { .. }
                | Self::Rejected
                | Self::ConstraintViolated { .. }
                | Self::RemoteUnavailable { .. }),
            ) => failure,
            (Self::Uncertain, _) | (_, Self::Uncertain) => Self::Uncertain,
            (Self::Superseded, _) | (_, Self::Superseded) => Self::Superseded,
            (Self::Done, Self::Done) => Self::Done,
        }
    }
}
