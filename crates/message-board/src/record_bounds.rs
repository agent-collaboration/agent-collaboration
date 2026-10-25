//! Shared caller-field limits for admission and authoritative record decoding.
use crate::*;

pub const MAX_TRAIL_TAGS: usize = 32;
pub const MAX_TRAIL_LINKS: usize = 16;
pub const MAX_EVIDENCE_PATH_BYTES: usize = 1024;
pub const MAX_EVIDENCE_URL_BYTES: usize = 2048;

pub(crate) fn text_bound(
    value: &str,
    limit: usize,
    field: &'static str,
) -> Result<(), TextValidationError> {
    if value.len() > limit {
        return Err(TextValidationError {
            field,
            requirement: "exceeds its UTF-8 byte limit",
        });
    }
    Ok(())
}
pub(crate) fn count_bound<T>(
    values: &[T],
    limit: usize,
    field: &'static str,
) -> Result<(), TextValidationError> {
    if values.len() > limit {
        return Err(TextValidationError {
            field,
            requirement: "exceeds its entry limit",
        });
    }
    Ok(())
}

impl EvidencePointer {
    pub fn validate_bounds(&self) -> Result<(), TextValidationError> {
        match self {
            Self::RepoPath { path, .. } => text_bound(path, MAX_EVIDENCE_PATH_BYTES, "path"),
            Self::Url { url } => text_bound(url.as_str(), MAX_EVIDENCE_URL_BYTES, "url"),
            Self::Commit { .. } | Self::PullRequest { .. } | Self::Record { .. } => Ok(()),
        }
    }
}
impl WorkTrailRow {
    pub fn validate_bounds(&self) -> Result<(), TextValidationError> {
        count_bound(&self.tags, MAX_TRAIL_TAGS, "tags")?;
        count_bound(&self.links, MAX_TRAIL_LINKS, "links")?;
        self.links
            .iter()
            .try_for_each(EvidencePointer::validate_bounds)
    }
}
fn invalid_reasons() -> TextValidationError {
    TextValidationError {
        field: "reasons",
        requirement: "must follow the rejection producer's shape",
    }
}

impl Record {
    pub fn validate_bounds(&self) -> Result<(), TextValidationError> {
        match self {
            Self::Workstream(row) => {
                text_bound(&row.goal, MAX_MESSAGE_TEXT_BYTES, "goal")?;
                if let WorkstreamState::Closed { note, .. } = &row.state {
                    text_bound(note, MAX_MESSAGE_TEXT_BYTES, "note")?;
                }
            }
            Self::Task(row) => text_bound(&row.body, MAX_MESSAGE_TEXT_BYTES, "body")?,
            Self::WorkTrailRow(row) => row.validate_bounds()?,
            Self::RoleAssignment(row) => {
                if let Some(RoleEnd::StoodDown { reason }) = &row.ended {
                    text_bound(reason, MAX_MESSAGE_TEXT_BYTES, "reason")?;
                }
            }
            Self::OwnerGrant(row) => {
                let GrantKind::ReplaceLead { reason, .. } = &row.grant;
                text_bound(reason, MAX_MESSAGE_TEXT_BYTES, "reason")?;
            }
            Self::Workspace(row) => count_bound(&row.repositories, 16, "repositories")?,
            Self::Board(row) => {
                if let BoardKind::Initiative { goal, done_when } = &row.kind {
                    text_bound(goal, 32 * 1024, "goal")?;
                    text_bound(done_when, 32 * 1024, "doneWhen")?;
                }
            }
            Self::Adr(row) => {
                count_bound(&row.repository_files, 16, "repositoryFiles")?;
                for artifact in &row.repository_files {
                    artifact.validate_bounds()?;
                }
            }
            Self::PlanRevision(row) => row.document.validate_bounds()?,
            Self::Fact(row) => {
                count_bound(&row.links, 64, "links")?;
                row.kind.validate_bounds()?;
            }
            Self::NotifyIntent(row) => {
                if let IntentState::Refused {
                    reason: IntentRefusal::Delivery { detail },
                } = row.state()
                {
                    text_bound(detail, 16 * 1024, "detail")?;
                }
            }
            Self::Request(row) => {
                if let StoredDecision::Rejected { reasons } = &row.decision {
                    if reasons.len() != 1 {
                        return Err(invalid_reasons());
                    }
                    for reason in reasons {
                        reason.validate_bounds()?;
                    }
                }
            }
            Self::TaskLinkEvent(_)
            | Self::Subtask(_)
            | Self::TrailRedaction(_)
            | Self::StandDownAcknowledgement(_)
            | Self::Project(_)
            | Self::ReadMark(_)
            | Self::SubscriptionMembership(_)
            | Self::Post(_)
            | Self::Thread(_)
            | Self::ThreadRefiled(_)
            | Self::DirectoryEntry(_) => {}
        }
        Ok(())
    }
}

impl Refusal {
    pub fn validate_bounds(&self) -> Result<(), TextValidationError> {
        let reason = match self {
            Self::ProjectRejected { reason } => {
                if matches!(reason.as_ref(), Self::ProjectRejected { .. }) {
                    return Err(TextValidationError {
                        field: "reason",
                        requirement: "permits at most one project rejection wrapper",
                    });
                }
                reason.as_ref()
            }
            reason => reason,
        };
        match reason {
            Self::InvalidShape { field } | Self::Trail(TrailRefusal::InvalidShape { field }) => {
                text_bound(field, 256, "field")
            }
            _ => Ok(()),
        }
    }
}
