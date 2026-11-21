//! A close's local retirement and remote evidence are independent.

use collaboration_protocol::{
    ProviderOperationEffect, ProviderOperationKind, ProviderOperationStage,
    ProviderReconciliationState,
};
use collaboration_runtime::StoredUncertainOperation;
use provider_session_machines::session_owner::{CloseOutcome, RemoteEffect};

pub const INCOMPLETE_CLOSE_NOTE: &str =
    "an earlier close may have reached the harness; restoring anyway";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloseSettlementEvidence {
    pub locally_retired: bool,
    pub effect: ProviderOperationEffect,
    pub reconciliation: ProviderReconciliationState,
    pub summary: &'static str,
}

#[must_use]
pub const fn classify_close(outcome: CloseOutcome) -> CloseSettlementEvidence {
    let effect = match outcome.remote_effect() {
        RemoteEffect::None => ProviderOperationEffect::None,
        RemoteEffect::Applied => ProviderOperationEffect::Applied,
        RemoteEffect::Unknown => ProviderOperationEffect::Unknown,
    };
    let reconciliation = match effect {
        ProviderOperationEffect::None | ProviderOperationEffect::Applied => {
            ProviderReconciliationState::Confirmed
        }
        ProviderOperationEffect::Unknown => ProviderReconciliationState::Unresolved,
    };
    CloseSettlementEvidence {
        locally_retired: outcome.locally_retired(),
        effect,
        reconciliation,
        summary: outcome.human_phrase(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreCloseEvidence {
    RunStillPresent,
    ExactRunRetired,
    ReopenedStore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreUncertainty {
    BlocksRestore,
    EarlierRestoreRetired,
    EarlierCloseIncomplete,
}

/// Live callers supply evidence of the exact retired run. A reopened store
/// has no surviving local incarnation from before the restart.
#[must_use]
pub const fn classify_restore_uncertainty(
    row: &StoredUncertainOperation,
    local: RestoreCloseEvidence,
) -> RestoreUncertainty {
    match (row.kind, row.stage, local) {
        (
            ProviderOperationKind::ConversationClose,
            ProviderOperationStage::MayHaveDispatched | ProviderOperationStage::Terminal,
            RestoreCloseEvidence::ExactRunRetired | RestoreCloseEvidence::ReopenedStore,
        ) => RestoreUncertainty::EarlierCloseIncomplete,
        (kind, ProviderOperationStage::Terminal, _) if kind.is_restore() => {
            RestoreUncertainty::EarlierRestoreRetired
        }
        _ => RestoreUncertainty::BlocksRestore,
    }
}

#[cfg(test)]
#[path = "session_close_evidence_test.rs"]
mod tests;
