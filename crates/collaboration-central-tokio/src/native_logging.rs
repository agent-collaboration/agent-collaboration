//! Native logs retain the core's closed classifications without request data.
use collaboration_central_core::CentralObservation;

pub(crate) fn observe(observation: CentralObservation) {
    match observation {
        CentralObservation::AccessCheck {
            route_class,
            outcome,
            key_source,
        } => {
            tracing::info!(target: "collaboration_central::access", ?route_class, ?outcome, ?key_source, "central Access check");
        }
        CentralObservation::OrganizationStatementRefused {
            organization,
            refusal,
        } => {
            tracing::info!(target: "collaboration_central::organization", organization = organization.as_str(), ?refusal, "central organization statement refused");
        }
    }
}
