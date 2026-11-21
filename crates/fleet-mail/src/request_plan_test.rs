use super::*;
use central_storage::{RowExpectation, SqlValue};
use workspace_model::{ChangeEvent, ItemRef};

fn sourced_notice_input() -> (SessionRef, NoticeSource, Timestamp) {
    let target = SessionRef {
        machine_id: "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap(),
        session_id: "recipient".to_owned().try_into().unwrap(),
    };
    let source = NoticeSource {
        change_id: "01890f2e-7b4c-7cc0-98c4-000000000003".parse().unwrap(),
        recipient_actor: "01890f2e-7b4c-7cc0-98c4-000000000004".parse().unwrap(),
        workspace_id: "01890f2e-7b4c-7cc0-98c4-000000000005".parse().unwrap(),
        thread_id: Some("01890f2e-7b4c-7cc0-98c4-000000000006".parse().unwrap()),
    };
    (
        target,
        source,
        "2026-11-20T06:00:00.123999Z".parse().unwrap(),
    )
}

#[test]
fn sourced_notice_plans_the_exact_insert_and_matching_readiness() {
    let (target, source, now) = sourced_notice_input();
    let item = NoticeItem {
        workspace_id: source.workspace_id.clone(),
        thread_id: source.thread_id.clone(),
        change_id: source.change_id.clone(),
        position: 7_u64.try_into().unwrap(),
        item: ItemRef::Thread(source.thread_id.clone().unwrap()),
        actor: "01890f2e-7b4c-7cc0-98c4-000000000007".parse().unwrap(),
        event: ChangeEvent::Resolved,
    };
    let message_id: MessageId = "01890f2e-7b4c-7cc0-98c4-000000000002".parse().unwrap();
    let expected_hint = MailDeliveryReady {
        machine_id: target.machine_id.clone(),
        message_id: message_id.clone(),
    };
    let (insert, hint) =
        plan_sourced_notice(target, vec![item], source, now, || Ok(message_id)).unwrap();

    assert_eq!(hint, expected_hint);
    assert_eq!(insert.expect, RowExpectation::Any);
    assert_eq!(
        insert.statement.sql,
        concat!(
            "INSERT INTO mail_messages(message_id,sender,target_machine_id,target_session,",
            "message_kind,body,state,failure_reason,source_change_id,recipient_actor,",
            "source_workspace_id,source_thread_id,created_at,updated_at,cancel_due_at,",
            "sender_request_id,delivery_window_ends_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            " ON CONFLICT(source_change_id,recipient_actor) WHERE source_change_id IS NOT NULL DO NOTHING",
        )
    );
    use SqlValue::{Null, Text};
    assert_eq!(
        insert.statement.args,
        vec![
            Text("01890f2e-7b4c-7cc0-98c4-000000000002".into()),
            Text(r#"{"kind":"central"}"#.into()),
            Text("01890f2e-7b4c-7cc0-98c4-000000000001".into()),
            Text(r#"{"machineId":"01890f2e-7b4c-7cc0-98c4-000000000001","sessionId":"recipient"}"#.into()),
            Text("notice".into()),
            Text(concat!(
                r#"{"kind":"notice","items":[{"workspaceId":"01890f2e-7b4c-7cc0-98c4-000000000005","#,
                r#""threadId":"01890f2e-7b4c-7cc0-98c4-000000000006","changeId":"01890f2e-7b4c-7cc0-98c4-000000000003","#,
                r#""position":7,"item":{"kind":"thread","id":"01890f2e-7b4c-7cc0-98c4-000000000006"},"#,
                r#""actor":"01890f2e-7b4c-7cc0-98c4-000000000007","event":{"kind":"resolved"}}]}"#,
            ).into()),
            Text("queued".into()),
            Null,
            Text("01890f2e-7b4c-7cc0-98c4-000000000003".into()),
            Text("01890f2e-7b4c-7cc0-98c4-000000000004".into()),
            Text("01890f2e-7b4c-7cc0-98c4-000000000005".into()),
            Text("01890f2e-7b4c-7cc0-98c4-000000000006".into()),
            Text("2026-11-20T06:00:00.123Z".into()),
            Text("2026-11-20T06:00:00.123Z".into()),
            Null,
            Null,
            Text("2026-11-21T06:05:00.000Z".into()),
        ],
    );
}

#[test]
fn sourced_notice_propagates_a_failed_mint() {
    let (target, source, now) = sourced_notice_input();
    let result = plan_sourced_notice(target, vec![], source, now, || {
        Err(StoreFailure::AnswerLost)
    });
    assert!(matches!(result, Err(StoreFailure::AnswerLost)));
}
