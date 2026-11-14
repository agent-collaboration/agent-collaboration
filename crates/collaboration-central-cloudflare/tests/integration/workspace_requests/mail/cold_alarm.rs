use super::super::{
    Statement,
    support::*,
    workerd_host::{AlarmReceipt, AlarmReceiptResult},
};
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result,
};
use std::sync::atomic::Ordering;

async fn set_due_wake(fixture: &Fixture) {
    let now = fixture.clock.0.load(Ordering::SeqCst);
    let due = chrono::DateTime::from_timestamp_millis(now + 60_000).unwrap();
    let result = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(1300),
                operation: Operation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: "cold".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(result, Result::Done(Answer::WakeSet { .. })));
    fixture
        .clock
        .0
        .store(due.timestamp_millis(), Ordering::SeqCst);
}

#[tokio::test]
async fn cold_platform_alarm_recovers_organization_and_fires() {
    let fixture = Fixture::new().await;
    set_due_wake(&fixture).await;
    let runtime = fixture.host.cold_runtime(&fixture.caller.organization_id);
    assert_eq!(runtime.cold_alarm().await.unwrap().wakes_fired, 1);
    let receipt: Option<AlarmReceipt> = runtime.control("raw-alarm-status", &()).await.unwrap();
    let receipt = receipt.unwrap();
    assert!(receipt.initialized_cold);
    assert_eq!(
        receipt.organization,
        Some(fixture.caller.organization_id.clone())
    );
    assert!(matches!(
        receipt.result,
        AlarmReceiptResult::Applied { wakes_fired: 1, .. }
    ));
    let rows = runtime.read(Statement { sql: "SELECT (SELECT COUNT(*) FROM wakes WHERE state='done'), (SELECT COUNT(*) FROM messages WHERE message_kind='wake')", args: vec![] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            central_storage::SqlValue::Integer(1.try_into().unwrap()),
            central_storage::SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn stamp_failures_and_unprovisioned_alarm_leave_business_rows_unchanged() {
    for case in 0..5 {
        let fixture = Fixture::new().await;
        set_due_wake(&fixture).await;
        let runtime = fixture.host.cold_runtime(&fixture.caller.organization_id);
        let own = fixture.caller.organization_id.as_str().to_owned();
        let other = id::<workspace_model::WorkspaceId>(1301).to_string();
        let stamps = match case {
            0 => vec![],
            1 => vec![(other.clone(), "verified fixture".to_owned())],
            2 => vec![
                (own.clone(), "verified fixture".to_owned()),
                (other, "verified fixture".to_owned()),
            ],
            3 => vec![("bad id".to_owned(), "verified fixture".to_owned())],
            _ => vec![(own.clone(), String::new())],
        };
        let applied: std::result::Result<
            central_storage::BatchOutcome,
            central_storage::StoreFailure,
        > = runtime.control("raw-set-stamps", &stamps).await.unwrap();
        assert!(matches!(
            applied,
            Ok(central_storage::BatchOutcome::Applied { .. })
        ));
        let result = runtime.cold_alarm().await;
        match case {
            0 => assert_eq!(
                result.unwrap(),
                collaboration_central_core::AlarmResult::default()
            ),
            1 => assert_eq!(
                result.unwrap_err(),
                central_storage::StoreFailure::Unavailable
            ),
            _ => {
                let column = if case == 4 {
                    "organization_identity.organization_statement"
                } else {
                    "organization_identity.id"
                };
                assert_eq!(
                    result.unwrap_err(),
                    central_storage::StoreFailure::Corrupt {
                        reason: central_storage::CorruptReason::BadValue {
                            column: column.to_owned()
                        }
                    }
                );
            }
        }
        let receipt: Option<AlarmReceipt> = runtime.control("raw-alarm-status", &()).await.unwrap();
        let receipt = receipt.unwrap();
        assert!(receipt.initialized_cold);
        assert!(receipt.organization.is_none());
        if case == 0 {
            assert!(matches!(receipt.result, AlarmReceiptResult::Unprovisioned));
            assert_eq!(
                runtime
                    .control::<Option<i64>>("raw-alarm-time", &())
                    .await
                    .unwrap(),
                None
            );
        } else {
            assert!(matches!(receipt.result, AlarmReceiptResult::Error { .. }));
        }
        let rows: std::result::Result<central_storage::ReadOutcome, central_storage::StoreFailure> = runtime.control("raw-read", &Statement { sql: "SELECT (SELECT COUNT(*) FROM wakes WHERE state='active'), (SELECT COUNT(*) FROM wakes WHERE state='done'), (SELECT COUNT(*) FROM messages WHERE message_kind='wake')", args: vec![] }).await.unwrap();
        let rows = rows.unwrap();
        assert_eq!(
            rows.rows,
            vec![vec![
                central_storage::SqlValue::Integer(1.try_into().unwrap()),
                central_storage::SqlValue::Integer(0.try_into().unwrap()),
                central_storage::SqlValue::Integer(0.try_into().unwrap())
            ]]
        );
        let restored: std::result::Result<
            central_storage::BatchOutcome,
            central_storage::StoreFailure,
        > = runtime
            .control("raw-set-stamps", &vec![(own, "verified fixture")])
            .await
            .unwrap();
        assert!(matches!(
            restored,
            Ok(central_storage::BatchOutcome::Applied { .. })
        ));
        fixture.host.close().await;
    }
}

#[tokio::test]
async fn timer_commit_crash_restarts_without_loss_or_duplicate() {
    for (point, expected_recovered) in [("beforeTimerCommit", 1), ("afterTimerCommit", 0)] {
        let fixture = Fixture::new().await;
        set_due_wake(&fixture).await;
        let runtime = fixture.host.central(&fixture.caller.organization_id).await;
        let _: bool = runtime.control("hold-central", &point).await.unwrap();
        let driver = runtime.clone();
        let running = tokio::spawn(async move { driver.on_alarm().await });
        let _: bool = runtime.control("until-held-central", &point).await.unwrap();
        let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
        assert!(reset.is_err());
        running.abort();
        let _ = running.await;
        let cold = fixture.host.cold_runtime(&fixture.caller.organization_id);
        assert_eq!(
            cold.cold_alarm().await.unwrap().wakes_fired,
            expected_recovered
        );
        let rows = cold.read(Statement { sql: "SELECT (SELECT COUNT(*) FROM wakes WHERE state='done'), (SELECT COUNT(*) FROM messages WHERE message_kind='wake')", args: vec![] }).await.unwrap();
        assert_eq!(
            rows.rows,
            vec![vec![
                central_storage::SqlValue::Integer(1.try_into().unwrap()),
                central_storage::SqlValue::Integer(1.try_into().unwrap())
            ]]
        );
        fixture.host.close().await;
    }
}
