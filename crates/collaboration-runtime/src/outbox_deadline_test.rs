use super::*;
use central_link_client::OutboxCancellation;

#[test]
fn every_outbox_write_has_15_minutes() {
    assert_eq!(expires_at_ms(1_000), 1_000 + 900_000);
    assert_eq!(expires_at_ms(i64::MAX), i64::MAX);
}

#[test]
fn an_overdue_write_ends_by_whether_it_left_the_mac() {
    assert_eq!(
        end_reason(DepartureAtDeadline::NeverLeft),
        OutboxCancellation::NotSentWithin15Minutes
    );
    assert_eq!(
        end_reason(DepartureAtDeadline::MayHaveArrived),
        OutboxCancellation::NotConfirmedWithin15Minutes
    );
}
