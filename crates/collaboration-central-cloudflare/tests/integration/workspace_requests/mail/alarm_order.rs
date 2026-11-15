use super::super::{Statement, support::*};
use collaboration_protocol::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn earlier_wake_queued_behind_timer_commit_owns_next_physical_alarm() {
    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let now = f.clock.0.load(Ordering::SeqCst);
    let later = chrono::DateTime::from_timestamp_millis(now + 600_000).unwrap();
    let earlier = chrono::DateTime::from_timestamp_millis(now + 300_000).unwrap();
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
    let _: bool = runtime
        .control("hold-central", &"beforeTimerCommit")
        .await
        .unwrap();
    let pending_alarm = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.on_alarm().await }
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
        let caller = f.caller.clone();
        async move { runtime.request_central(&caller, new_request).await }
    });
    let _: bool = runtime
        .control("until-central-waiting", &1_usize)
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
    assert_eq!(runtime.on_alarm().await.unwrap().wakes_fired, 1);
    let rows = runtime
        .read(Statement {
            sql: "SELECT state,COUNT(*) FROM wakes GROUP BY state ORDER BY state",
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
