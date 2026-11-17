use super::*;
use crate::operation_record_machine::{Reconciliation, SettledEffect};
use collaboration_protocol::{EndpointId, SessionId};

fn endpoint() -> EndpointRef {
    EndpointRef {
        service_id: "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89"
            .to_owned()
            .try_into()
            .expect("service ID"),
        endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("endpoint ID"),
    }
}

fn session(name: &str) -> SessionRef {
    SessionRef {
        endpoint: endpoint(),
        session_id: SessionId::try_from(name.to_owned()).expect("session ID"),
    }
}

fn index_with(
    operation: &OperationId,
    live: LiveOperation,
) -> (LiveOperations, Arc<LiveOperation>) {
    let live = Arc::new(live);
    let mut index = LiveOperations::default();
    index
        .operations
        .insert(operation.clone(), Arc::clone(&live));
    (index, live)
}

/// Oracle: design note §11.5 and §14.1: a send counts as in flight from its
/// dispatch marker until its record settles; an unknown outcome stays live
/// (SettlingUnknown) until the uncertainty cell holds it; an undone marker
/// stops it counting.
#[test]
fn a_send_counts_in_flight_from_its_marker_until_it_settles() {
    // Arrange
    let operation = OperationId::generate();
    let (index, live) = index_with(
        &operation,
        LiveOperation::reserving(endpoint(), Some(session("a"))),
    );
    live.admitted();
    let before_marker = index.in_flight_on(&endpoint(), Some(&session("a")));

    // Act
    live.dispatch_marked();
    let marked = index.in_flight_on(&endpoint(), Some(&session("a")));
    let other_session = index.in_flight_on(&endpoint(), Some(&session("b")));
    live.completed(AttemptEnding::Unknown);
    let settling_unknown = index.in_flight_on(&endpoint(), Some(&session("a")));
    live.uncertainty_recorded();
    let recorded = index.in_flight_on(&endpoint(), Some(&session("a")));

    // Assert
    assert!(before_marker.is_empty(), "admitted, not yet marked");
    assert_eq!(marked, vec![operation.clone()]);
    assert!(
        other_session.is_empty(),
        "another session doesn't wait on it"
    );
    assert_eq!(
        settling_unknown,
        vec![operation],
        "still live until recorded"
    );
    assert!(recorded.is_empty());
    assert_eq!(
        live.record(),
        OperationRecord::Settled {
            effect: SettledEffect::Unknown,
            reconciliation: Reconciliation::Unresolved,
        }
    );
}

/// Oracle: §11.5's rollback (a send refused before anything left) and
/// §15.3's reservation endings.
#[test]
fn an_undone_marker_and_a_dropped_reservation_leave_nothing_in_flight() {
    // Arrange
    let undone = OperationId::generate();
    let (index, live) = index_with(&undone, LiveOperation::reserving(endpoint(), None));
    live.admitted();
    live.dispatch_marked();
    let binding_wide = index.in_flight_on(&endpoint(), Some(&session("any")));
    let dropped = LiveOperation::reserving(endpoint(), None);

    // Act
    live.undispatched();
    dropped.dropped(true);

    // Assert
    assert_eq!(
        binding_wide,
        vec![undone],
        "a targetless send blocks every session"
    );
    assert!(
        index
            .in_flight_on(&endpoint(), Some(&session("any")))
            .is_empty()
    );
    assert_eq!(live.record(), OperationRecord::Admitted { queued: false });
    assert_eq!(dropped.record(), OperationRecord::Dropped);
    assert!(!dropped.is_reserving());
}

/// Oracle: §11.5: an operation never dispatched ends not sent; a refused
/// attempt settles with no effect.
#[test]
fn an_undispatched_operation_settles_not_sent() {
    // Arrange
    let live = LiveOperation::reserving(endpoint(), None);
    live.admitted();

    // Act
    live.completed(AttemptEnding::Refused);

    // Assert
    assert_eq!(
        live.record(),
        OperationRecord::Settled {
            effect: SettledEffect::None,
            reconciliation: Reconciliation::Confirmed,
        }
    );
}
