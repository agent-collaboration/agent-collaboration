//! The Worker callback and signed requests see the workerd fixture's one clock.
use super::{engine_v2_support::*, support::*};
use central_storage::Statement;
use collaboration_central_conformance::signin_fixtures as signed;
use workspace_model::{RefusalReason, WorkspaceOperation, WorkspaceResult};

#[tokio::test]
async fn injected_clock_drives_uuid_minting_issuer_expiry_and_the_armed_alarm() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let issuer_deadline = start + chrono::Duration::minutes(4);
    let due = start + chrono::Duration::minutes(5);
    let mut expiring_caller = fixture.caller.clone();
    expiring_caller.token = signed::machine_token(
        expiring_caller.organization_id.as_str(),
        &expiring_caller.caller.machine_id().to_string(),
        &expiring_caller.enrollment_id,
        start.timestamp(),
        issuer_deadline.timestamp(),
    )
    .unwrap();

    let first = set_wake(&fixture, 7300, due, "first injected wake").await;
    set_now(&fixture, start + chrono::Duration::milliseconds(1));
    let second = set_wake(&fixture, 7301, due, "second injected wake").await;
    for (wake, milliseconds) in [(&first, 0), (&second, 1)] {
        let minted = uuid::Uuid::parse_str(wake).unwrap();
        assert!(fleet_runtime_ports::uuid_v7(minted));
        let (seconds, nanos) = minted.get_timestamp().unwrap().to_unix();
        assert_eq!(
            seconds * 1_000 + u64::from(nanos / 1_000_000),
            u64::try_from(start.timestamp_millis()).unwrap() + milliseconds
        );
    }
    assert_eq!(alarm_ms(&runtime).await, Some(due.timestamp_millis()));

    set_now(
        &fixture,
        issuer_deadline - chrono::Duration::milliseconds(1),
    );
    let create_area = || WorkspaceOperation::CreateArea {
        name: name("Issuer deadline"),
        description: "Test description".to_owned().try_into().unwrap(),
    };
    let accepted = call(
        &fixture.host,
        &expiring_caller,
        Some(fixture.workspace.clone()),
        7302,
        create_area(),
    )
    .await;
    assert!(matches!(accepted.result, WorkspaceResult::Done(_)));

    set_now(&fixture, issuer_deadline);
    let refused = call(
        &fixture.host,
        &expiring_caller,
        Some(fixture.workspace.clone()),
        7303,
        create_area(),
    )
    .await;
    assert_eq!(
        refused.result,
        WorkspaceResult::Refused(RefusalReason::MachineRevoked)
    );
    let rejected_write = runtime
        .inspect_workspace(
            &fixture.workspace,
            Statement {
                sql: "SELECT request_id FROM workspace_request_log WHERE request_id=?",
                args: vec![text(id::<workspace_model::RequestId>(7303))],
            },
        )
        .await
        .unwrap();
    assert!(rejected_write.rows.is_empty());

    set_now(&fixture, due - chrono::Duration::milliseconds(1));
    assert!(runtime.dispatch_due_alarm().await.unwrap().is_none());
    assert_eq!(wake_messages(&runtime, &first).await, 0);
    assert_eq!(wake_messages(&runtime, &second).await, 0);
    assert_eq!(alarm_ms(&runtime).await, Some(due.timestamp_millis()));

    set_now(&fixture, due);
    let completion = runtime.dispatch_due_alarm().await.unwrap().unwrap();
    assert_eq!(completion.wakes_fired, 2);
    assert_eq!(wake_messages(&runtime, &first).await, 1);
    assert_eq!(wake_messages(&runtime, &second).await, 1);
    assert_eq!(alarm_ms(&runtime).await, None);
    fixture.host.close().await;
}
