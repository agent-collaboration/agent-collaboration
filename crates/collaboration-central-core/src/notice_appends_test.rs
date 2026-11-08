use super::{noted, scoped, take};
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
        noted(outer);
        let inner_seen = scoped(async move {
            noted(inner);
            take()
        })
        .await;
        // The nested event's scope ended; this event's own set is back.
        (take(), inner_seen)
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
    noted(PeerId::new(durable_steps::Uuid::now_v7())?);
    assert!(take().is_empty());
    Ok(())
}
