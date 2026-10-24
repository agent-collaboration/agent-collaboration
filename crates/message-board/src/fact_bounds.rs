//! Fact payload limits keep producer evidence within one replicated record.
use crate::record_bounds::{count_bound, text_bound};
use crate::*;

impl ArtifactRef {
    pub fn validate_bounds(&self) -> Result<(), TextValidationError> {
        text_bound(&self.path, MAX_EVIDENCE_PATH_BYTES, "path")
    }
}
impl EvidenceRef {
    pub fn validate_bounds(&self) -> Result<(), TextValidationError> {
        match self {
            Self::Artifact { path, .. } => text_bound(path, MAX_EVIDENCE_PATH_BYTES, "path"),
            Self::Post(_) | Self::Fact(_) | Self::PrFact(_) => Ok(()),
        }
    }
}
impl FactKind {
    pub fn validate_bounds(&self) -> Result<(), TextValidationError> {
        match self {
            Self::Receipt(row) => {
                text_bound(&row.obligation, 4 * 1024, "obligation")?;
                text_bound(&row.scenario_run, 256, "scenarioRun")?;
                text_bound(&row.observed, 8 * 1024, "observed")?;
                count_bound(&row.commands, 16, "commands")?;
                for command in &row.commands {
                    text_bound(&command.command, 1024, "command")?;
                }
                count_bound(&row.artifacts, 16, "artifacts")?;
                for artifact in &row.artifacts {
                    artifact.validate_bounds()?;
                }
            }
            Self::EvaluatorResult {
                guard,
                evidence,
                evaluator,
                ..
            } => {
                text_bound(guard, 256, "guard")?;
                if let Some(engine) = &evaluator.engine {
                    text_bound(engine, 256, "engine")?;
                }
                count_bound(evidence, 16, "evidence")?;
                for item in evidence {
                    item.validate_bounds()?;
                }
            }
            Self::HookFact { targets, .. } => {
                count_bound(targets, 64, "targets")?;
                for target in targets {
                    text_bound(target, 1024, "target")?;
                }
            }
            Self::ScopeAcknowledgement { paths, reason, .. } => {
                count_bound(paths, 64, "paths")?;
                for path in paths {
                    text_bound(path, 1024, "path")?;
                }
                text_bound(reason, 16 * 1024, "reason")?;
            }
            Self::Lifecycle(
                LifecycleFact::Closed { reason, .. } | LifecycleFact::Dropped { reason, .. },
            ) => text_bound(reason, MAX_MESSAGE_TEXT_BYTES, "reason")?,
            Self::Lifecycle(LifecycleFact::ThreadResolved { reason, .. }) => {
                text_bound(reason.as_str(), MAX_MESSAGE_TEXT_BYTES, "reason")?
            }
            Self::Lifecycle(LifecycleFact::PhaseCompleted { phase }) => {
                text_bound(phase, 256, "phase")?
            }
            Self::Lifecycle(_) | Self::PrFact { .. } | Self::OwnerWords => {}
        }
        Ok(())
    }
}
