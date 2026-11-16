//! V12 (P5 change 7) on the workerd host: a due batch's notice-window unit
//! holding the central turn never makes another instance's engine request wait.
use super::super::engine_evidence::{
    assert_fixture_attempt_ids_are_uuid_v7, witness_fixture_attempt_ids,
};
use super::super::engine_v2_support::{Runtime, integer, runtime, set_now};
use super::*;
use collaboration_central_core::engine_probe::{
    EngineRequestOutcome, InstanceKey, ProbeEvent, ProbeRequest, SeedProbe,
};

async fn count(runtime: &Runtime, sql: &'static str) -> SqlValue {
    runtime
        .statement_host()
        .read(Statement { sql, args: vec![] })
        .await
        .unwrap()
        .rows[0][0]
        .clone()
}
const NOTICES: &str = "SELECT COUNT(*) FROM messages WHERE message_kind='notice'";
const WINDOWS: &str = "SELECT COUNT(*) FROM notification_windows";
const WINDOW_RECORDS: &str =
    "SELECT COUNT(*) FROM central_work_attempts WHERE machine_kind='legacy_notice_windows'";
async fn central_control(runtime: &Runtime, operation: &str, point: &str) {
    let _: bool = runtime.control(operation, &point).await.unwrap();
}

#[tokio::test]
async fn a_held_window_unit_never_delays_another_instances_engine_request() {
    let f = Fixture::new().await;
    witness_fixture_attempt_ids(&f).await;
    let area = f.area(9700).await;
    let thread = f.thread(9701, area).await;
    joined_observer(&f, &thread).await;
    let reply = f.call(9702, post(&thread, vec![])).await;
    assert!(
        matches!(reply.result, m::WorkspaceResult::Done(_)),
        "{reply:?}"
    );
    let runtime = runtime(&f).await;
    let due: String = match &runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT due_at FROM notification_windows",
            args: vec![],
        })
        .await
        .unwrap()
        .rows[0][0]
    {
        SqlValue::Text(due) => due.clone(),
        other => panic!("window due_at {other:?}"),
    };
    let due = chrono::DateTime::parse_from_rfc3339(&due)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let windows = count(&runtime, WINDOWS).await;
    assert_ne!(windows, integer(0));
    let item = uuid::Uuid::parse_str("01890f2e-7b4c-7cc0-98c4-000000009703").unwrap();
    assert!(matches!(
        runtime
            .seed_probe(SeedProbe {
                key: item,
                group: uuid::Uuid::parse_str("01890f2e-7b4c-7cc0-98c4-000000009704").unwrap(),
                due: None,
                claim: "request".into(),
            })
            .await
            .unwrap(),
        central_storage::BatchOutcome::Applied { .. }
    ));

    // Due batch A starts at the window's due instant and is held inside its
    // window unit, before the commit, while it holds the central turn.
    central_control(&runtime, "hold-central", "beforeTimerCommit").await;
    set_now(&f, due);
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    central_control(&runtime, "until-held-central", "beforeTimerCommit").await;

    // 31 s later a probe request on another instance claims a new due batch.
    set_now(&f, due + chrono::Duration::seconds(31));
    let request = runtime.request_probe(
        &f.caller,
        ProbeRequest {
            request_id: id(9705),
            key: InstanceKey(item.to_string()),
            event: ProbeEvent::Run,
        },
    );
    let central_waiting = runtime.control::<bool>("until-central-waiting", &1_usize);
    let answer = tokio::select! {
        answer = request => answer.unwrap(),
        _ = central_waiting => {
            panic!("the request's due batch waited on the held window unit's central turn")
        }
    };
    assert!(
        matches!(answer, EngineRequestOutcome::Answer { .. }),
        "{answer:?}"
    );
    assert_eq!(
        count(&runtime, WINDOWS).await,
        windows,
        "the windows are still due"
    );
    assert_eq!(count(&runtime, NOTICES).await, integer(0));
    assert_eq!(
        count(&runtime, WINDOW_RECORDS).await,
        integer(0),
        "a skipped window unit is not a failure"
    );

    // Released, A delivers the window once; a later batch finds nothing.
    central_control(&runtime, "hold-central", "afterTimerCommit").await;
    central_control(&runtime, "release-central", "beforeTimerCommit").await;
    central_control(&runtime, "until-held-central", "afterTimerCommit").await;
    assert_eq!(count(&runtime, NOTICES).await, windows);
    central_control(&runtime, "release-central", "afterTimerCommit").await;
    batch.await.unwrap().unwrap();
    set_now(&f, due + chrono::Duration::seconds(65));
    runtime.after_event().await.unwrap();
    assert_eq!(count(&runtime, NOTICES).await, windows, "no duplicate");
    assert_eq!(count(&runtime, WINDOWS).await, integer(0));
    assert_eq!(count(&runtime, WINDOW_RECORDS).await, integer(0));
    assert_fixture_attempt_ids_are_uuid_v7(
        &f,
        "a_held_window_unit_never_delays_another_instances_engine_request",
        &[],
    )
    .await;
    f.host.close().await;
}
