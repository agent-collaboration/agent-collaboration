use super::*;

fn session() -> OutboxWriteTarget {
    OutboxWriteTarget::Session {
        to: workspace_model::SessionAddress {
            machine_id: "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap(),
            endpoint: "codex-local".to_owned().try_into().unwrap(),
            session_id: "recipient".to_owned().try_into().unwrap(),
        },
    }
}

fn thread() -> OutboxWriteTarget {
    OutboxWriteTarget::Thread {
        workspace_id: None,
        thread_id: "01890f2e-7b4c-7cc0-98c4-000000000002".parse().unwrap(),
    }
}

#[test]
fn a_direct_message_has_15_minutes_and_every_other_write_24_hours() {
    assert_eq!(expires_at_ms(&session(), 1_000), 1_000 + 900_000);
    assert_eq!(expires_at_ms(&thread(), 1_000), 1_000 + 86_400_000);
    assert_eq!(expires_at_ms(&thread(), i64::MAX), i64::MAX);
}

#[test]
fn an_overdue_write_ends_by_target_and_whether_it_left_the_mac() {
    use DepartureAtDeadline::{MayHaveArrived, NeverLeft};
    assert_eq!(
        end_reason(&session(), NeverLeft),
        OutboxCancellation::NotDeliveredWithin15Minutes
    );
    assert_eq!(
        end_reason(&session(), MayHaveArrived),
        OutboxCancellation::NotConfirmedWithin15Minutes
    );
    assert_eq!(
        end_reason(&thread(), NeverLeft),
        OutboxCancellation::NotSentWithin24Hours
    );
    assert_eq!(
        end_reason(&thread(), MayHaveArrived),
        OutboxCancellation::NotConfirmedWithin24Hours
    );
}
