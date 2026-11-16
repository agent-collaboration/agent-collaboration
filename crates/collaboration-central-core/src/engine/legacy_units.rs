use central_engine_model::ItemFailureClass;
use central_storage::{BatchOutcome, ConstraintKind, StoreFailure};
use workspace_model::WorkspaceId;

/// The typed result of one whole notice unit (P5). Every recorded variant names
/// the workspaces it affected; a failure found before any workspace (a central
/// listing read) names none.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LegacyUnitOutcome {
    Done,
    RemoteUnavailable {
        workspaces: Vec<WorkspaceId>,
    },
    Corrupt {
        column: String,
        workspaces: Vec<WorkspaceId>,
    },
    Rejected {
        workspaces: Vec<WorkspaceId>,
    },
    ConstraintViolated {
        constraint: ConstraintKind,
        workspaces: Vec<WorkspaceId>,
    },
    Superseded,
    Uncertain,
    CentralDown(StoreFailure),
}
impl From<StoreFailure> for LegacyUnitOutcome {
    fn from(failure: StoreFailure) -> Self {
        match failure {
            StoreFailure::Corrupt { reason } => {
                let (central_storage::CorruptReason::MissingColumn { column }
                | central_storage::CorruptReason::UnknownKind { column }
                | central_storage::CorruptReason::BadValue { column }) = reason;
                Self::Corrupt {
                    column,
                    workspaces: vec![],
                }
            }
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
            Ok(BatchOutcome::StatementRejected { .. }) => {
                Err(Self::Rejected { workspaces: vec![] })
            }
            Ok(BatchOutcome::ConstraintViolated { kind, .. }) => Err(Self::ConstraintViolated {
                constraint: kind,
                workspaces: vec![],
            }),
            Err(failure) => Err(failure.into()),
        }
    }
    pub(crate) fn failure_class(&self) -> Option<ItemFailureClass> {
        match self {
            Self::RemoteUnavailable { .. } => Some(ItemFailureClass::OtherStoreUnavailable),
            Self::Corrupt { column, .. } => Some(ItemFailureClass::CorruptRow {
                column: column.clone(),
                raw_value: None,
            }),
            Self::Rejected { .. } => Some(ItemFailureClass::StatementRejected),
            Self::ConstraintViolated { constraint, .. } => {
                Some(ItemFailureClass::ConstraintViolated {
                    constraint: *constraint,
                })
            }
            Self::Done | Self::Superseded | Self::Uncertain | Self::CentralDown(_) => None,
        }
    }
    /// The workspaces a recorded failure affected, for its observation.
    pub(crate) fn affected_workspaces(&self) -> &[WorkspaceId] {
        match self {
            Self::RemoteUnavailable { workspaces }
            | Self::Corrupt { workspaces, .. }
            | Self::Rejected { workspaces }
            | Self::ConstraintViolated { workspaces, .. } => workspaces,
            Self::Done | Self::Superseded | Self::Uncertain | Self::CentralDown(_) => &[],
        }
    }
    /// Names the one workspace a per-workspace drain failure came from.
    pub(crate) fn in_workspace(mut self, workspace: &WorkspaceId) -> Self {
        if let Self::RemoteUnavailable { workspaces }
        | Self::Corrupt { workspaces, .. }
        | Self::Rejected { workspaces }
        | Self::ConstraintViolated { workspaces, .. } = &mut self
            && !workspaces.contains(workspace)
        {
            workspaces.push(workspace.clone());
        }
        self
    }
    /// Preserve the old request entry's result; the engine keeps the richer origin.
    pub(crate) fn into_request_failure(self) -> StoreFailure {
        match self {
            Self::Corrupt { column, .. } => crate::workspace_decode_error::corrupt(&column),
            Self::CentralDown(failure) => failure,
            Self::Uncertain => StoreFailure::AnswerLost,
            Self::Done
            | Self::RemoteUnavailable { .. }
            | Self::Rejected { .. }
            | Self::ConstraintViolated { .. }
            | Self::Superseded => StoreFailure::Unavailable,
        }
    }
    /// Central outage wins. Then the first recorded failure keeps its class and
    /// gains every other failing workspace; uncharged outcomes are not failures.
    pub(crate) fn combine(self, next: Self) -> Self {
        match (self, next) {
            (down @ Self::CentralDown(_), _) | (_, down @ Self::CentralDown(_)) => down,
            (mut first, second) if first.failure_class().is_some() => {
                let more = second.affected_workspaces().to_vec();
                for workspace in &more {
                    first = first.in_workspace(workspace);
                }
                first
            }
            (_, failure) if failure.failure_class().is_some() => failure,
            (Self::Uncertain, _) | (_, Self::Uncertain) => Self::Uncertain,
            (Self::Superseded, _) | (_, Self::Superseded) => Self::Superseded,
            (Self::Done, Self::Done) => Self::Done,
            // Every remaining pair has a recorded side and was matched above.
            (first, _) => first,
        }
    }
}
