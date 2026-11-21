use super::*;

#[hegel::test(seed = Some(2911801), test_cases = 100)]
fn a_same_operation_undo_cannot_clear_the_replacement_mark(case: hegel::TestCase) {
    use hegel::generators as gs;

    let state = Arc::new(OperationLifecycle::new(
        OperationRecord::Admitted { queued: false },
        Arc::new(Notify::new()),
    ));
    let old = Uuid::now_v7();
    let first = state.begin_lane_write(Arc::new(|| {})).unwrap();
    first.prepare_lane_projection(old);
    first.project_lane_mark(old);
    assert_eq!(state.state().lane_marked(), Some(true));
    first.settle(CustodyEnd::Written);

    let undo_attempt = Uuid::now_v7();
    let undo = state.begin_lane_write(Arc::new(|| {})).unwrap();
    undo.prepare_lane_projection(undo_attempt);
    if case.draw(gs::booleans()) {
        undo.project_lane_undo(undo_attempt, Uuid::now_v7());
        assert_eq!(state.state().lane_marked(), Some(true));
    }
    undo.project_lane_undo(undo_attempt, old);
    assert_eq!(state.state().lane_marked(), Some(false));
    undo.settle(CustodyEnd::Written);

    let replacement = Uuid::now_v7();
    let ticket = state.begin_lane_write(Arc::new(|| {})).unwrap();
    ticket.prepare_lane_projection(replacement);
    for _ in 0..case.draw(gs::integers::<u8>().max_value(12)) {
        ticket.project_lane_mark(old);
        ticket.project_lane_undo(undo_attempt, old);
        assert_eq!(state.state().lane_marked(), Some(false));
    }
    ticket.project_lane_mark(replacement);
    ticket.project_lane_undo(replacement, old);
    assert_eq!(
        state.state().record,
        OperationRecord::MayHaveDispatched {
            attempt: DispatchAttempt(replacement)
        }
    );
    ticket.settle(CustodyEnd::Written);
}

#[test]
fn only_the_last_matching_lane_receipt_can_release_a_finished_writer() {
    let mut custody = OperationCustody::held();
    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    assert!(custody.begin_lane_write(first));
    assert!(custody.begin_lane_write(second));

    assert!(!custody.finish_host(CustodyEnd::NoWrite));
    assert!(!custody.finish_lane_write(Uuid::now_v7(), CustodyEnd::Written));
    assert!(!custody.finish_lane_write(first, CustodyEnd::Written));
    assert!(!custody.finish_lane_write(first, CustodyEnd::Written));
    assert!(custody.holds_custody());
    assert!(custody.finish_lane_write(second, CustodyEnd::Failed));

    assert_eq!(custody, OperationCustody::Ended(CustodyEnd::Failed));
    assert!(!custody.begin_lane_write(Uuid::now_v7()));
}
