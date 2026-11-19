//! V8 and V12.4 on workerd with the held clock: without traffic the object wakes
//! only when work is due, each time once; with a request every minute it never
//! needs an alarm callback. Times are named from the fixture's 10:00:00 start.
use super::super::support::*;
use super::engine_event_support::*;
use super::engine_probe_support::*;
use central_storage::SqlValue;
use workspace_model::central_mail::CentralMailResult;

/// The wake's message time: when the due batch that fired it ran.
async fn wake_message_times(runtime: &Runtime) -> Vec<Vec<SqlValue>> {
    runtime
        .statement_host()
        .read(super::super::Statement {
            sql: "SELECT created_at FROM mail_messages WHERE message_kind='wake' ORDER BY created_at",
            args: vec![],
        })
        .await
        .unwrap()
        .rows
}

#[tokio::test]
async fn an_hour_without_traffic_wakes_only_when_work_is_due_and_each_time_once() {
    let mut fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    open_notice_window(&fixture, 33000).await;
    assert_eq!(
        window_due(&runtime).await,
        vec![vec![stamp(at(start, 15, 0, 0))]]
    );
    let windows = count(
        &runtime,
        "SELECT COUNT(*) FROM mail_notification_windows",
        vec![],
    )
    .await;
    let early = set_wake(&fixture, 33010, at(start, 5, 0, 0)).await;
    let late = set_wake(&fixture, 33011, at(start, 20, 0, 0)).await;
    let (failing, failing_group) = (probe_id(33020), probe_id(33021));
    seed_probe(
        &runtime,
        failing,
        failing_group,
        Some(at(start, 5, 0, 0)),
        "failing",
    )
    .await;
    raw(
        &runtime,
        "CREATE TRIGGER probe_reject BEFORE UPDATE OF effect_count ON engine_probe_groups BEGIN SELECT RAISE(ABORT,'probe rejected'); END",
        vec![],
    )
    .await;
    // The last setup due batch starts at 10:01:45, so the 10:02:15 backstop
    // set before the final setup write sits exactly on its 30 s floor.
    set_now(&fixture, at(start, 1, 45, 0));
    fixture.caller = super::super::support::fresh_auth(&fixture.host, &fixture.caller);
    assert!(matches!(
        send_message(&fixture, 33030).await.unwrap(),
        CentralMailResult::Done(_)
    ));
    preset_drain(&fixture, &runtime, at(start, 2, 15, 0)).await;
    set_now(&fixture, at(start, 2, 10, 0));
    fixture.caller = super::super::support::fresh_auth(&fixture.host, &fixture.caller);
    assert!(matches!(
        send_message(&fixture, 33031).await.unwrap(),
        CentralMailResult::Done(_)
    ));
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 2, 15, 0)));
    let before = callbacks(&runtime).await;
    let expected = [
        at(start, 2, 15, 0),
        at(start, 5, 0, 0),
        at(start, 10, 0, 0),
        at(start, 15, 0, 0),
        at(start, 20, 0, 0),
    ];
    let mut probes: Vec<Time> = (0..=240)
        .map(|step| at(start, 2, 10 + 15 * step, 0))
        .chain(
            expected
                .iter()
                .flat_map(|time| [*time - chrono::Duration::milliseconds(1), *time]),
        )
        .collect();
    probes.sort();
    probes.dedup();
    let mut fired_at = Vec::new();

    for time in probes {
        set_now(&fixture, time);
        fixture.caller = super::super::support::fresh_auth(&fixture.host, &fixture.caller);
        let dispatched = runtime.dispatch_due_alarm().await.unwrap();
        if expected.contains(&time) {
            assert!(dispatched.is_some(), "a callback is due at {time}");
            fired_at.push(time);
            let next = alarm_ms(&runtime).await;
            assert!(
                next.is_none_or(|next| next > time.timestamp_millis()),
                "the re-arm at {time} is strictly later: {next:?}"
            );
            assert!(
                runtime.dispatch_due_alarm().await.unwrap().is_none(),
                "no second callback at {time}"
            );
        } else {
            assert!(dispatched.is_none(), "no callback is due at {time}");
        }
    }

    assert_eq!(fired_at, expected);
    assert_eq!(
        callbacks(&runtime).await,
        before + 5,
        "exactly five callbacks"
    );
    assert_eq!(wake_state(&runtime, &early).await, text("done"));
    assert_eq!(wake_state(&runtime, &late).await, text("done"));
    assert_eq!(drain_due(&fixture, &runtime).await, SqlValue::Null);
    assert_eq!(notices(&runtime).await, windows, "each window sent once");
    assert_eq!(
        attempt(&runtime, failing).await,
        vec![vec![
            text("retired"),
            integer(2),
            text("statement_rejected")
        ]],
        "the failing probe was tried twice, then retired"
    );
    assert_eq!(
        alarm_ms(&runtime).await,
        None,
        "the alarm reads back NULL once nothing is pending"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_request_every_minute_needs_no_alarm_callback_and_fires_wakes_early() {
    let mut fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let early = set_wake(&fixture, 34000, at(start, 5, 0, 0)).await;
    let late = set_wake(&fixture, 34001, at(start, 20, 0, 0)).await;
    let before = callbacks(&runtime).await;

    for minute in 0..30 {
        set_now(&fixture, at(start, minute, 20, 0));
        fixture.caller = super::super::support::fresh_auth(&fixture.host, &fixture.caller);
        assert!(matches!(
            send_message(&fixture, 34010 + u32::try_from(minute).unwrap())
                .await
                .unwrap(),
            CentralMailResult::Done(_)
        ));
    }

    assert_eq!(callbacks(&runtime).await, before, "zero alarm callbacks");
    assert_eq!(wake_state(&runtime, &early).await, text("done"));
    assert_eq!(wake_state(&runtime, &late).await, text("done"));
    assert_eq!(
        wake_message_times(&runtime).await,
        vec![
            vec![stamp(at(start, 4, 20, 0))],
            vec![stamp(at(start, 19, 20, 0))]
        ],
        "each wake fired 40 s before its mark, inside the 50 s look-ahead"
    );
    fixture.host.close().await;
}
