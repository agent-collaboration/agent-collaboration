//! Preserve the old forbidden-edge oracle and a permitted lower-ranked control.
use collaboration_central_core::{CallEdgeRefused, CallerKind, validate_call_edge};

#[test]
fn copy_to_registry_preserves_the_exact_typed_forbidden_edge() {
    assert_eq!(
        validate_call_edge(CallerKind::Copy, CallerKind::Registry),
        Err(CallEdgeRefused {
            from: CallerKind::Copy,
            to: CallerKind::Registry,
        })
    );
    assert_eq!(
        validate_call_edge(CallerKind::Copy, CallerKind::Fleet),
        Ok(())
    );
    println!("CALL_EDGE_CASE exact_Copy_Registry_refusal_and_allowed_Copy_Fleet passed");
}
