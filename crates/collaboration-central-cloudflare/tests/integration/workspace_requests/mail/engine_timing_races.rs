//! V12 races on workerd: a request held across a newer due batch publishes the
//! newer floor and backlog, two simultaneous requests run one due batch, an
//! eviction repeats nothing, notice units get no look-ahead, and a changed
//! organization stamp refuses every event. Times are named from 10:00:00.
use super::super::{
    support::*,
    workerd_host::{AlarmReceipt, AlarmReceiptResult},
};
use super::engine_event_support::*;
use super::engine_probe_support::*;
use central_storage::StoreFailure;
use collaboration_central_core::{
    PausePoint,
    engine_probe::{EngineRequestOutcome, ProbeEvent},
};
use workspace_model::central_mail::CentralMailResult;

/// Moves every open window to `due`, keeping its items.
async fn window_due_at(runtime: &Runtime, due: Time) {
    raw(
        runtime,
        "UPDATE mail_notification_windows SET due_at=?",
        vec![stamp(due)],
    )
    .await;
}

/// Request A commits at 10:04:20 and is held before its event's publication;
/// request B at 10:04:58 claims a due batch; A publishes after B at 10:04:59.
async fn held_request_publishes_after_a_newer_batch(
    fixture: &Fixture,
    runtime: &Runtime,
    start: Time,
    traffic: uuid::Uuid,
) {
    let mut held = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineCommit, &traffic.to_string())
        .await;
    set_now(fixture, at(start, 4, 20, 0));
    let request_a = tokio::spawn({
        let runtime = runtime.clone();
        let caller = fixture.caller.clone();
        async move {
            runtime
                .request_probe(&caller, probe_request(35001, traffic, ProbeEvent::Run))
                .await
        }
    });
    held.until_held().await;
    set_now(fixture, at(start, 4, 58, 0));
    assert!(matches!(
        send_message(fixture, 35002).await.unwrap(),
        CentralMailResult::Done(_)
    ));
    assert_eq!(
        alarm_ms(runtime).await,
        ms(at(start, 5, 28, 0)),
        "B published its floor"
    );
    set_now(fixture, at(start, 4, 59, 0));
    held.release().await;
    assert!(matches!(
        request_a.await.unwrap().unwrap(),
        EngineRequestOutcome::Answer { .. }
    ));
}

#[tokio::test]
async fn a_request_held_across_a_newer_due_batch_publishes_the_newer_floor() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    open_notice_window(&fixture, 35100).await;
    due_batch_at(&fixture, &runtime, at(start, 4, 0, 0)).await;
    window_due_at(&runtime, at(start, 5, 0, 0)).await;
    let (traffic, traffic_group) = (probe_id(35000), probe_id(35003));
    seed_probe(&runtime, traffic, traffic_group, None, "traffic").await;

    held_request_publishes_after_a_newer_batch(&fixture, &runtime, start, traffic).await;

    assert_eq!(
        alarm_ms(&runtime).await,
        ms(at(start, 5, 28, 0)),
        "A, published last, used B's floor and never 10:05:00"
    );
    assert_eq!(
        window_due(&runtime).await,
        vec![vec![stamp(at(start, 5, 0, 0))]]
    );
    assert_eq!(notices(&runtime).await, 0, "the window is not run early");
    fixture.host.close().await;
}

#[tokio::test]
async fn a_request_held_across_a_capped_batch_publishes_the_continuation() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    due_batch_at(&fixture, &runtime, at(start, 4, 0, 0)).await;
    let group = probe_id(35200);
    for index in 0..251 {
        seed_probe(
            &runtime,
            probe_id(35300 + index),
            group,
            Some(at(start, 5, 0, 0)),
            &format!("capped{index}"),
        )
        .await;
    }
    let (traffic, traffic_group) = (probe_id(35000), probe_id(35003));
    seed_probe(&runtime, traffic, traffic_group, None, "traffic").await;

    held_request_publishes_after_a_newer_batch(&fixture, &runtime, start, traffic).await;

    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM engine_probe_items WHERE group_id=? AND state='fired'",
            vec![text(group)]
        )
        .await,
        250,
        "B's batch left one item beyond the cap"
    );
    assert_eq!(
        alarm_ms(&runtime).await,
        ms(at(start, 5, 28, 0)),
        "A kept the continuation, not the 10:05:00 due"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn two_simultaneous_requests_run_exactly_one_due_batch() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    due_batch_at(&fixture, &runtime, at(start, 4, 0, 0)).await;
    let before = runtime.legacy_unit_invocations().await;
    set_now(&fixture, at(start, 4, 31, 0));

    let (first, second) =
        tokio::join!(send_message(&fixture, 35400), send_message(&fixture, 35401));

    assert!(matches!(first.unwrap(), CentralMailResult::Done(_)));
    assert!(matches!(second.unwrap(), CentralMailResult::Done(_)));
    let after = runtime.legacy_unit_invocations().await;
    assert_eq!(
        (after.windows - before.windows, after.drains - before.drains),
        (1, 1),
        "exactly one due batch ran its notice units"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn an_eviction_after_a_due_batch_fires_nothing_twice() {
    let mut fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (item, group) = (probe_id(35500), probe_id(35501));
    seed_probe(&runtime, item, group, Some(at(start, 5, 0, 0)), "early").await;
    let wake = set_wake(&fixture, 35502, at(start, 5, 0, 0)).await;
    due_batch_at(&fixture, &runtime, at(start, 4, 58, 0)).await;
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("fired"), integer(1), integer(0)]
    );
    assert_eq!(wake_state(&runtime, &wake).await, text("done"));

    fixture.reopen().await;
    set_now(&fixture, at(start, 5, 5, 0));
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    assert!(matches!(
        send_message(&fixture, 35503).await.unwrap(),
        CentralMailResult::Done(_)
    ));

    let invocations = runtime.legacy_unit_invocations().await;
    println!(
        "V12.10 workerd: after eviction the 10:05:05 request ran {} due batch(es)",
        invocations.windows
    );
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("fired"), integer(1), integer(0)],
        "the probe's effect ran once"
    );
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake'",
            vec![]
        )
        .await,
        1,
        "the wake's message went out once"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_wake_due_with_a_window_fires_early_and_the_window_waits_for_its_slot() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    open_notice_window(&fixture, 35600).await;
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
    assert!(windows > 0);
    let wake = set_wake(&fixture, 35610, at(start, 15, 0, 0)).await;

    due_batch_at(&fixture, &runtime, at(start, 14, 40, 0)).await;

    assert_eq!(
        wake_state(&runtime, &wake).await,
        text("done"),
        "20 s early"
    );
    assert_eq!(notices(&runtime).await, 0, "the window is not run early");
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 15, 10, 0)));
    set_now(&fixture, at(start, 15, 9, 999));
    assert!(runtime.dispatch_due_alarm().await.unwrap().is_none());
    set_now(&fixture, at(start, 15, 10, 0));
    assert!(runtime.dispatch_due_alarm().await.unwrap().is_some());
    assert_eq!(
        notices(&runtime).await,
        windows,
        "each window sent its notice"
    );
    due_batch_at(&fixture, &runtime, at(start, 15, 41, 0)).await;
    assert_eq!(
        notices(&runtime).await,
        windows,
        "each notice went out once"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn another_valid_stamp_refuses_every_event_and_touches_nothing() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (due, due_group) = (probe_id(35700), probe_id(35701));
    let (traffic, traffic_group) = (probe_id(35702), probe_id(35703));
    seed_probe(&runtime, due, due_group, Some(at(start, 5, 0, 0)), "due").await;
    seed_probe(&runtime, traffic, traffic_group, None, "traffic").await;
    due_batch_at(&fixture, &runtime, at(start, 1, 0, 0)).await;
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 5, 0, 0)));
    let own = fixture.caller.organization_id.as_str().to_owned();
    let other = id::<workspace_model::WorkspaceId>(35704).to_string();
    assert_eq!(other.parse::<uuid::Uuid>().unwrap().get_version_num(), 7);
    let stamped: Result<central_storage::BatchOutcome, central_storage::StoreFailure> = runtime
        .control(
            "raw-set-stamps",
            &vec![(other, "fixture organization statement".to_owned())],
        )
        .await
        .unwrap();
    assert!(matches!(
        stamped,
        Ok(central_storage::BatchOutcome::Applied { .. })
    ));
    set_now(&fixture, at(start, 5, 0, 0));
    // The clock control is itself refused under a foreign stamp, so the fan-out
    // is armed through the raw alarm control and the request is sent bare.
    let armed: bool = runtime
        .control("raw-arm-alarm", &at(start, 5, 0, 0).timestamp_millis())
        .await
        .unwrap();
    assert!(armed);

    let fan_out: AlarmReceipt = runtime.control("raw-until-alarm", &()).await.unwrap();
    let request: Result<Result<ProbeAnswer, StoreFailure>, StoreFailure> = runtime
        .control(
            "engine-probe-request",
            &serde_json::json!({"caller": fixture.caller.caller, "token": fixture.caller.token, "request": probe_request(35705, traffic, ProbeEvent::Run)}),
        )
        .await;

    assert!(
        matches!(fan_out.result, AlarmReceiptResult::Error { .. }),
        "the Due fan-out is refused"
    );
    assert_eq!(
        request,
        Err(StoreFailure::Unavailable),
        "the request is refused"
    );
    let restored: Result<central_storage::BatchOutcome, central_storage::StoreFailure> = runtime
        .control(
            "raw-set-stamps",
            &vec![(own, "fixture organization statement".to_owned())],
        )
        .await
        .unwrap();
    assert!(matches!(
        restored,
        Ok(central_storage::BatchOutcome::Applied { .. })
    ));
    for (item, group) in [(due, due_group), (traffic, traffic_group)] {
        assert_eq!(
            probe_facts(&runtime, item, group).await,
            vec![text("waiting"), integer(0), integer(0)],
            "no instance is touched"
        );
    }
    assert_eq!(receipts(&runtime, 35705).await, 0);
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM engine_work_attempts",
            vec![]
        )
        .await,
        0,
        "no record is written"
    );

    let cleared: Result<central_storage::BatchOutcome, central_storage::StoreFailure> = runtime
        .control("raw-set-stamps", &Vec::<(String, String)>::new())
        .await
        .unwrap();
    assert!(matches!(
        cleared,
        Ok(central_storage::BatchOutcome::Applied { .. })
    ));
    set_now(&fixture, at(start, 1, 10, 0));
    let _ = send_message(&fixture, 35706).await;
    assert_eq!(
        alarm_ms(&runtime).await,
        None,
        "unprovisioned: the request's event cleared the alarm"
    );
    fixture.host.close().await;
}
