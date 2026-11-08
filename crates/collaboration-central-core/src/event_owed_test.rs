use super::{owed_notice, owed_request, scoped, take};
use fleet_protocol::PeerId;
use std::collections::BTreeSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each event's appended set while id errors propagate."
)]
#[tokio::test]
async fn an_event_takes_only_its_own_appends_and_a_nested_event_keeps_its_own() -> TestResult {
    let outer = PeerId::new(durable_steps::Uuid::now_v7())?;
    let inner = PeerId::new(durable_steps::Uuid::now_v7())?;
    let (outer_seen, inner_seen) = scoped(async move {
        owed_notice(outer);
        let inner_seen = scoped(async move {
            owed_notice(inner);
            take().notices
        })
        .await;
        // The nested event's scope ended; this event's own set is back.
        (take().notices, inner_seen)
    })
    .await;
    assert_eq!(outer_seen, BTreeSet::from([outer]));
    assert_eq!(inner_seen, BTreeSet::from([inner]));
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertion checks the outside-event case while id errors propagate."
)]
#[test]
fn an_append_outside_any_event_names_nothing() -> TestResult {
    owed_notice(PeerId::new(durable_steps::Uuid::now_v7())?);
    assert_eq!(take(), super::EventOwed::default());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertion keeps notice and request machines apart while id errors propagate."
)]
#[tokio::test]
async fn notices_and_requests_are_owed_separately() -> TestResult {
    let (notice, request) = (
        PeerId::new(durable_steps::Uuid::now_v7())?,
        PeerId::new(durable_steps::Uuid::now_v7())?,
    );
    let owed = scoped(async move {
        owed_notice(notice);
        owed_request(request);
        take()
    })
    .await;
    assert_eq!(owed.notices, BTreeSet::from([notice]));
    assert_eq!(owed.requests, BTreeSet::from([request]));
    Ok(())
}
