use super::super::provider_admission_policy::AdmissionMode;
use super::super::tests::binding;
use super::*;
use crate::operation_machines::operation_record_machine::{Reconciliation, SettledEffect};
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

fn in_flight_on(index: &LiveOperations, target: &SessionRef) -> Vec<OperationId> {
    index
        .lane_mark_blocker(
            &binding(),
            AdmissionRequest {
                operation: &OperationId::generate(),
                kind: ProviderOperationKind::ConversationLoad,
                mode: AdmissionMode::HANDOFF,
                target: Some(target),
            },
        )
        .into_iter()
        .collect()
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

/// The marked send stays in flight through recording uncertainty, until its
/// writer settles. An undone marker stops it counting immediately.
#[test]
fn a_send_counts_in_flight_from_its_marker_until_it_settles() {
    // Arrange
    let operation = OperationId::generate();
    let (index, live) = index_with(
        &operation,
        LiveOperation::reserving(
            &binding(),
            Some(session("a")),
            ProviderOperationKind::ConversationPrompt,
            AdmissionMode::deferred(AdmissionClass::OwnerPlaced),
        ),
    );
    live.admitted();
    let before_marker = in_flight_on(&index, &session("a"));

    // Act
    live.dispatch_marked();
    let marked = in_flight_on(&index, &session("a"));
    let other_session = in_flight_on(&index, &session("b"));
    live.completed(AttemptEnding::Unknown);
    let settling_unknown = in_flight_on(&index, &session("a"));
    live.uncertainty_recorded();
    let recorded = in_flight_on(&index, &session("a"));
    live.finish_write(CustodyEnd::Written);
    let persisted = in_flight_on(&index, &session("a"));

    // Assert
    assert!(before_marker.is_empty(), "admitted, not yet marked");
    assert_eq!(marked, vec![operation.clone()]);
    assert!(
        other_session.is_empty(),
        "another session doesn't wait on it"
    );
    assert_eq!(
        settling_unknown,
        vec![operation.clone()],
        "still live until recorded"
    );
    assert_eq!(
        recorded,
        vec![operation],
        "the terminal write is still held"
    );
    assert!(persisted.is_empty());
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
    let (index, live) = index_with(
        &undone,
        LiveOperation::reserving(
            &binding(),
            None,
            ProviderOperationKind::ConversationPrompt,
            AdmissionMode::deferred(AdmissionClass::OwnerPlaced),
        ),
    );
    live.admitted();
    live.dispatch_marked();
    let binding_wide = in_flight_on(&index, &session("any"));
    let dropped = LiveOperation::reserving(
        &binding(),
        None,
        ProviderOperationKind::ConversationPrompt,
        AdmissionMode::deferred(AdmissionClass::OwnerPlaced),
    );

    // Act
    live.undispatched();
    dropped.dropped(true, CustodyEnd::Dropped);

    // Assert
    assert_eq!(
        binding_wide,
        vec![undone],
        "a targetless send blocks every session"
    );
    assert!(in_flight_on(&index, &session("any")).is_empty());
    assert_eq!(live.record(), OperationRecord::Admitted { queued: false });
    assert_eq!(dropped.record(), OperationRecord::Dropped);
    assert!(!dropped.is_reserving());
}

/// Oracle: §11.5: an operation never dispatched ends not sent; a refused
/// attempt settles with no effect.
#[test]
fn an_undispatched_operation_settles_not_sent() {
    // Arrange
    let live = LiveOperation::reserving(
        &binding(),
        None,
        ProviderOperationKind::ConversationPrompt,
        AdmissionMode::deferred(AdmissionClass::OwnerPlaced),
    );
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

#[test]
fn lane_receipts_are_bound_to_the_reserved_operation_generation_and_target() {
    let binding = binding();
    let target = session("reserved");
    let operation = OperationId::generate();
    let (mut index, old) = index_with(
        &operation,
        LiveOperation::reserving(
            &binding,
            Some(target.clone()),
            ProviderOperationKind::ConversationPrompt,
            AdmissionMode::deferred(AdmissionClass::OwnerPlaced),
        ),
    );
    let lookup = |index: &LiveOperations,
                  operation: &OperationId,
                  binding: &ProviderBindingIdentity,
                  target: &SessionRef| {
        index.begin_lane_write(operation, &binding.generation, target, Arc::new(|| {}))
    };
    assert!(lookup(&index, &OperationId::generate(), &binding, &target).is_none());
    assert!(lookup(&index, &operation, &binding, &session("other")).is_none());
    let old_ticket = lookup(&index, &operation, &binding, &target).expect("the old writer");
    let mut replacement_binding = binding.clone();
    replacement_binding.generation.generation = 2.try_into().unwrap();
    let replacement = Arc::new(LiveOperation::reserving(
        &replacement_binding,
        Some(target.clone()),
        ProviderOperationKind::ConversationPrompt,
        AdmissionMode::deferred(AdmissionClass::OwnerPlaced),
    ));
    index
        .operations
        .insert(operation.clone(), Arc::clone(&replacement));
    assert!(lookup(&index, &operation, &binding, &target).is_none());
    let ticket =
        lookup(&index, &operation, &replacement_binding, &target).expect("the current writer");
    old.finish_write(CustodyEnd::NoWrite);
    replacement.finish_write(CustodyEnd::NoWrite);

    old_ticket.settle(CustodyEnd::Written);
    assert!(!old.holds_custody());
    assert!(replacement.holds_custody());
    ticket.settle(CustodyEnd::Written);
    assert!(!replacement.holds_custody());
}

#[test]
fn a_published_writer_survives_result_window_eviction_until_its_receipt() {
    let operation = OperationId::generate();
    let (mut index, live) = index_with(&operation, LiveOperation::pending());
    index.retain_result(operation.clone(), Retention::PersistenceFailed);

    for _ in 0..=MAX_RETAINED_SETTLEMENTS {
        let operation = OperationId::generate();
        let later = LiveOperation::pending();
        later.finish_write(CustodyEnd::Failed);
        index.operations.insert(operation.clone(), Arc::new(later));
        index.retain_result(operation, Retention::PersistenceFailed);
    }
    assert!(index.operations.contains_key(&operation));
    assert_eq!(index.operations.len(), MAX_RETAINED_SETTLEMENTS + 1);

    live.finish_write(CustodyEnd::Written);
    index.trim_results();
    assert!(!index.operations.contains_key(&operation));
    assert_eq!(index.operations.len(), MAX_RETAINED_SETTLEMENTS);
}
