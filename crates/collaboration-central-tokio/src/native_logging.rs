//! Native logs retain the core's closed classifications without request data.
use collaboration_central_core::CentralObservation;

pub(crate) fn observe(observation: CentralObservation) {
    let CentralObservation::AccessCheck {
        route_class,
        outcome,
        key_source,
    } = observation;
    tracing::info!(target: "collaboration_central::access", ?route_class, ?outcome, ?key_source, "central Access check");
}
