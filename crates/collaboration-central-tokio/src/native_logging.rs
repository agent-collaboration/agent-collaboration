//! Native logs retain closed classifications without request data.
use fleet_contracts::CentralObservation;
pub(crate) fn observe(observation: CentralObservation) {
    match observation {
        CentralObservation::EngineAlarmNotPublished { failure } => {
            tracing::warn!(target:"collaboration_central::engine",?failure,"central engine alarm not published")
        }
        CentralObservation::EngineStoreDown { kind, key, failure } => {
            tracing::warn!(target:"collaboration_central::engine",kind=kind.0,key=%key.0,?failure,"central engine store down")
        }
        CentralObservation::EngineItemFailed {
            kind,
            key,
            failure,
            count,
            retired,
            workspaces,
        } => {
            tracing::warn!(target:"collaboration_central::engine",kind=kind.0,key=%key.0,class=failure.storage_tag(),count,retired,?workspaces,"central engine item failed")
        }
        CentralObservation::OrganizationStatementRefused {
            organization,
            refusal,
        } => {
            tracing::info!(target:"collaboration_central::organization",organization=%organization.as_str(),?refusal,"organization statement refused")
        }
    }
}
