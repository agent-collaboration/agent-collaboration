//! Project-to-Fleet returns use durable follow-ups, never a synchronous call edge.
use collaboration_central_core::{CallEdgeRefused, CallerKind, validate_call_edge};

#[test]
fn project_sync_calls_refuse_registry_and_fleet_with_exact_typed_edges() {
    for to in [CallerKind::Registry, CallerKind::Fleet] {
        assert_eq!(
            validate_call_edge(CallerKind::Copy, to),
            Err(CallEdgeRefused {
                from: CallerKind::Copy,
                to
            })
        );
    }
    assert_eq!(
        validate_call_edge(CallerKind::Fleet, CallerKind::Copy),
        Ok(())
    );
    println!(
        "CALL_EDGE_CASE exact_Copy_Registry_and_Copy_Fleet_refusals_and_allowed_Fleet_Copy passed"
    );
}
