use super::super::{Statement, support::*};
use std::sync::atomic::Ordering;
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

#[tokio::test]
async fn earlier_wake_queued_behind_timer_commit_owns_next_physical_alarm() {
    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let now = f.clock.0.load(Ordering::SeqCst);
    // The engine reaches the timer-commit points only for a unit with work, and
    // only the window unit holds the central turn a request queues behind. A
    // window due at the event's instant keeps the held commit; the wakes keep
    // their original offsets from that instant.
    let event = now + 900_000;
    let later = chrono::DateTime::from_timestamp_millis(event + 600_000).unwrap();
    let earlier = chrono::DateTime::from_timestamp_millis(event + 300_000).unwrap();
    let wake = |due_at| CentralMailOperation::SetWake {
        target: session(&f.caller),
        due_at,
        text: "alarm order".to_owned().try_into().unwrap(),
    };
    let old = runtime
        .request_central(
            &f.caller,
            CentralMailRequest {
                request_id: id(1400),
                operation: wake(later),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        old,
        CentralMailResult::Done(CentralMailAnswer::WakeSet { .. })
    ));
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(later.timestamp_millis())
    );
    // An observer's join opens the thread creator's notice window, due at `event`.
    let area = f.area(1402).await;
    let thread = f.thread(1403, area).await;
    let mut observer = f.caller.clone();
    let mut address = session(&observer);
    address.session_id = "alarm-order-observer".to_owned().try_into().unwrap();
    observer.caller = m::Caller::Session(address);
    let joined = call(
        &f.host,
        &observer,
        Some(f.workspace.clone()),
        1404,
        m::WorkspaceOperation::JoinThread {
            thread_id: thread,
            role: m::MemberRole::Observer,
        },
    )
    .await;
    assert!(matches!(
        joined.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(event)
    );
    f.clock.0.store(event, Ordering::SeqCst);
    // The clock moved past the fixture token; this request carries one for now.
    let fresh_caller = super::super::support::fresh_auth(&f.host, &f.caller);
    let _: bool = runtime
        .control("hold-central", &"beforeTimerCommit")
        .await
        .unwrap();
    let pending_alarm = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    let _: bool = runtime
        .control("until-held-central", &"beforeTimerCommit")
        .await
        .unwrap();
    let new_request = CentralMailRequest {
        request_id: id(1401),
        operation: wake(earlier),
    };
    let pending_request = tokio::spawn({
        let runtime = runtime.clone();
        let caller = fresh_caller.clone();
        async move { runtime.request_central(&caller, new_request).await }
    });
    let _: bool = runtime
        // The request first waits on the held central turn at its admission.
        .control("until-admission-waiting", &1_usize)
        .await
        .unwrap();
    assert!(!pending_request.is_finished());
    let _: bool = runtime
        .control("release-central", &"beforeTimerCommit")
        .await
        .unwrap();
    assert_eq!(pending_alarm.await.unwrap().unwrap().wakes_fired, 0);
    assert!(matches!(
        pending_request.await.unwrap().unwrap(),
        CentralMailResult::Done(CentralMailAnswer::WakeSet { .. })
    ));
    // Embedded (Lead ruling): due units commit with the alarm unchanged and the
    // re-arm is the event's final step, so this is read after both completed.
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(earlier.timestamp_millis())
    );
    f.clock
        .0
        .store(earlier.timestamp_millis(), Ordering::SeqCst);
    assert_eq!(runtime.after_event().await.unwrap().wakes_fired, 1);
    let rows = runtime
        .read(Statement {
            sql: "SELECT state,COUNT(*) FROM automation_wakes GROUP BY state ORDER BY state",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![
            vec![
                text("active"),
                central_storage::SqlValue::Integer(1.try_into().unwrap())
            ],
            vec![
                text("done"),
                central_storage::SqlValue::Integer(1.try_into().unwrap())
            ]
        ]
    );
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(later.timestamp_millis())
    );
    f.host.close().await;
}
