//! A dispatch settles a drive started at an earlier clock position before it
//! moves the clock, so that drive cannot finish after the move and pass for the
//! dispatched alarm.
use super::*;
use fleet_runtime::PausePoint;

/// Resolves once the manual clock has reached `at`; yields, never sleeps.
async fn clock_reaches(f: &Fixture, at: m::Timestamp) {
    while now(f) < at {
        tokio::task::yield_now().await;
    }
}

pub(super) async fn exercise_dispatch_settles_an_in_flight_drive(f: &Fixture) {
    // Arrange: two wakes five minutes apart.
    let target = session(&f.caller);
    let first = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(300) + 1) * 300, 0)
        .unwrap();
    let second = first + chrono::Duration::minutes(5);
    let mut wakes = Vec::new();
    for (number, due_at) in [(47000, first), (47001, second)] {
        let set = request(
            f,
            &f.caller,
            number,
            Op::SetWake {
                target: target.clone(),
                due_at,
                text: body(),
            },
        )
        .await;
        let Result::Done(Answer::WakeSet { wake_id, .. }) = set else {
            panic!("wake: {set:?}")
        };
        wakes.push(wake_id);
    }
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    // Every due batch reaches the drain unit's control clear; hold the drive
    // the jump past the first wake starts there.
    let mut held = r
        .pause_points()
        .hold_engine(PausePoint::BeforeControlClear, "legacy_workspace_drains");
    let overdue_at = first + chrono::Duration::seconds(20);
    advance_to(f, overdue_at).await;
    held.until_held().await;

    // Act: dispatch the second wake while the overdue drive is held.
    let mut dispatch = std::pin::pin!(dispatch_to(f, second));
    tokio::select! {
        () = &mut dispatch => panic!("the dispatch finished while a drive was held"),
        () = clock_reaches(f, second) => {
            panic!("the dispatch moved the clock while a drive was in flight")
        }
        () = f.clock.until_move_waiting() => {}
    }
    assert_eq!(now(f), overdue_at);
    held.release();
    dispatch.await;

    // Assert: the overdue drive fired the first wake at its own clock position,
    // and the dispatch fired the second at its due time.
    let queue = r.queued_for_session(&target).await.unwrap();
    let fired = |index: usize| {
        let wake = &wakes[index];
        queue
            .messages
            .iter()
            .filter(|message| {
                matches!(&message.body, MessageBody::Wake { wake_id, .. } if wake_id == wake)
            })
            .map(|message| message.created_at)
            .collect::<Vec<_>>()
    };
    assert_eq!(fired(0), vec![overdue_at]);
    assert_eq!(fired(1), vec![second]);
}

/// Every fixture clock move, not only a dispatch, settles a drive started at
/// an earlier clock position before it moves the clock.
pub(super) async fn exercise_a_clock_move_settles_an_in_flight_drive(f: &Fixture) {
    // Arrange: one wake, and the drive the jump past it starts held.
    let target = session(&f.caller);
    let due = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(300) + 1) * 300, 0)
        .unwrap();
    let set = request(
        f,
        &f.caller,
        47010,
        Op::SetWake {
            target: target.clone(),
            due_at: due,
            text: body(),
        },
    )
    .await;
    let Result::Done(Answer::WakeSet { wake_id, .. }) = set else {
        panic!("wake: {set:?}")
    };
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let mut held = r
        .pause_points()
        .hold_engine(PausePoint::BeforeControlClear, "legacy_workspace_drains");
    let overdue_at = due + chrono::Duration::seconds(20);
    advance_to(f, overdue_at).await;
    held.until_held().await;

    // Act: move the clock again while that drive is held.
    let later = overdue_at + chrono::Duration::seconds(10);
    let mut moved = std::pin::pin!(f.advance(std::time::Duration::from_secs(10)));
    tokio::select! {
        () = &mut moved => panic!("the clock moved while a drive was held"),
        () = clock_reaches(f, later) => {
            panic!("the clock moved while a drive was in flight")
        }
        () = f.clock.until_move_waiting() => {}
    }
    assert_eq!(now(f), overdue_at);
    held.release();
    moved.await;

    // Assert: the held drive fired the wake at its own clock position.
    assert_eq!(now(f), later);
    let queue = r.queued_for_session(&target).await.unwrap();
    let fired: Vec<_> = queue
        .messages
        .iter()
        .filter(|message| matches!(&message.body, MessageBody::Wake { wake_id: id, .. } if id == &wake_id))
        .map(|message| message.created_at)
        .collect();
    assert_eq!(fired, vec![overdue_at]);
}
