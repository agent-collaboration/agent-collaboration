use super::*;

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
