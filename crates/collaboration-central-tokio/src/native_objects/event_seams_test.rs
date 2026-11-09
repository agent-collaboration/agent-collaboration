#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail these tests while fixture errors propagate through Result."
)]
use super::*;
use collaboration_central_core::event_trace::{AlarmRecord, EventKindRecord, ReplyRecord};
use futures_util::FutureExt;

fn record(sequence: u64) -> EventRecord {
    EventRecord {
        object: ObjectAddress::Fleet,
        construction: Uuid::nil(),
        sequence,
        event_id: Uuid::now_v7(),
        event: EventKindRecord::Alarm {
            redelivery: Some(0),
            delivery: collaboration_central_core::event_trace::AlarmDelivery::Platform,
        },
        entered_at: 0,
        exited_at: 0,
        units: Vec::new(),
        sends: Vec::new(),
        notices: Vec::new(),
        calls: Vec::new(),
        alarm: AlarmRecord::default(),
        batch: None,
        reply: ReplyRecord::AlarmDone,
    }
}

#[test]
fn the_ring_reports_its_oldest_cursor_once_records_fall_out() {
    // Arrange
    let seams = ObjectSeams::new(ObjectAddress::Fleet);
    let empty = seams.read_events(0);

    // Act: one more record than the ring holds.
    for sequence in 0..=u64::try_from(RING_CAPACITY).unwrap_or(u64::MAX) {
        seams.push(record(sequence));
    }
    let full = seams.read_events(0);

    // Assert: before any record the next cursor is 1; afterwards cursor 1 has
    // fallen out, so a reader after 0 learns it missed one.
    assert_eq!(
        empty,
        RuntimeControlAnswer::Events {
            records: Vec::new(),
            first_retained: 1,
            through: 0
        }
    );
    let RuntimeControlAnswer::Events {
        records,
        first_retained,
        through,
    } = full
    else {
        panic!("not an events answer");
    };
    assert_eq!(first_retained, 2);
    assert_eq!(through, u64::try_from(RING_CAPACITY).unwrap_or(0) + 1);
    assert_eq!(records.len(), RING_CAPACITY);
    assert_eq!(records[0].record.sequence, 1);
}

#[tokio::test]
async fn two_latches_at_one_point_capture_two_distinct_events() -> Result<(), RuntimeControlFailure>
{
    // Arrange
    let seams = Arc::new(ObjectSeams::new(ObjectAddress::Fleet));
    let first = seams.arm_barrier(BarrierPoint::BeforeClaim, None);
    let second = seams.arm_barrier(BarrierPoint::BeforeClaim, None);
    let (one, two, three) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());

    // Act: three events arrive; only two latches exist.
    let held_one = seams.hold(BarrierPoint::BeforeClaim, None, one);
    let held_two = seams.hold(BarrierPoint::BeforeClaim, None, two);
    let passed = seams.hold(BarrierPoint::BeforeClaim, None, three);

    // Assert: each latch names its own event, and the third event isn't held.
    assert_eq!(seams.await_barrier(first).await?, one);
    assert_eq!(seams.await_barrier(second).await?, two);
    assert_eq!(passed.now_or_never(), Some(()));
    seams.release_barrier(first)?;
    assert_eq!(held_one.now_or_never(), Some(()));
    assert_eq!(
        held_two.now_or_never(),
        None,
        "releasing one latch released the other"
    );
    Ok(())
}

#[tokio::test]
async fn a_spent_latch_never_captures_again() -> Result<(), RuntimeControlFailure> {
    // Arrange
    let seams = Arc::new(ObjectSeams::new(ObjectAddress::Fleet));
    let latch = seams.arm_barrier(BarrierPoint::AfterUnitCommit, Some("handler".into()));
    let ignored = seams.hold(
        BarrierPoint::AfterUnitCommit,
        Some("recompute"),
        Uuid::now_v7(),
    );
    assert_eq!(ignored.now_or_never(), Some(()));
    let first = Uuid::now_v7();
    let held = seams.hold(BarrierPoint::AfterUnitCommit, Some("handler"), first);
    seams.release_barrier(latch)?;
    held.await;

    // Act
    let later = seams.hold(
        BarrierPoint::AfterUnitCommit,
        Some("handler"),
        Uuid::now_v7(),
    );

    // Assert: a unit kind that doesn't match passes, and a spent latch holds nothing.
    assert_eq!(later.now_or_never(), Some(()));
    assert_eq!(seams.await_barrier(latch).await?, first);
    Ok(())
}

#[tokio::test]
async fn a_latch_released_before_it_captured_leaves_the_next_event_to_a_fresh_latch()
-> Result<(), RuntimeControlFailure> {
    // Arrange: a driver cleans up a latch nothing reached, then arms a new one.
    let seams = Arc::new(ObjectSeams::new(ObjectAddress::Fleet));
    let stale = seams.arm_barrier(BarrierPoint::BeforeClaim, None);
    seams.release_barrier(stale)?;
    let fresh = seams.arm_barrier(BarrierPoint::BeforeClaim, None);
    let event = Uuid::now_v7();

    // Act
    let held = seams.hold(BarrierPoint::BeforeClaim, None, event);

    // Assert: the fresh latch holds the event; the spent one never captured it.
    assert_eq!(held.now_or_never(), None);
    assert_eq!(seams.await_barrier(fresh).await?, event);
    let stale_capture = seams.latch(stale)?.captured.borrow().is_some();
    assert!(!stale_capture, "a spent latch captured an event");
    Ok(())
}
