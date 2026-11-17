use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

#[tokio::test]
/// Split (Lead ruling): this is the current half. The "other window still
/// delivers" half is deferred under the original name to the notice deliverable,
/// because windows run as one whole unit (P5).
async fn unavailable_workspace_does_not_block_wakes_and_recovers_without_duplicates() {
    let mut f = Fixture::new().await;
    let second = call(
        &f.host,
        &f.caller,
        None,
        8800,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Outage second workspace"),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
        second.result
    else {
        panic!("second workspace")
    };
    let other = workspace.workspace_id;
    let r = f.host.central(&f.caller.organization_id).await;
    assert!(matches!(
        r.statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE machines SET allowed_workspaces=? WHERE installation_id=?",
                        args: vec![
                            text(
                                serde_json::to_string(&vec![f.workspace.clone(), other.clone()])
                                    .unwrap()
                            ),
                            text(f.caller.caller.machine_id())
                        ]
                    },
                    expect: RowExpectation::Exactly(1),
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let mut recipients = Vec::new();
    for (workspace, base, suffix) in [
        (&f.workspace, 8810, "outage-w1"),
        (&other, 8820, "healthy-w2"),
    ] {
        let area = created(
            call(
                &f.host,
                &f.caller,
                Some(workspace.clone()),
                base,
                m::WorkspaceOperation::CreateArea {
                    name: name(&format!("Area {suffix}")),
                },
            )
            .await,
        );
        let m::ItemRef::Area(area) = area else {
            panic!("area")
        };
        let thread = created(
            call(
                &f.host,
                &f.caller,
                Some(workspace.clone()),
                base + 1,
                m::WorkspaceOperation::CreateThread {
                    area_id: area,
                    title: name(&format!("Thread {suffix}")),
                    body: "First post".to_owned().try_into().unwrap(),
                    addressees: vec![],
                },
            )
            .await,
        );
        let m::ItemRef::Thread(thread) = thread else {
            panic!("thread")
        };
        assert!(matches!(
            call(
                &f.host,
                &f.caller,
                Some(workspace.clone()),
                base + 2,
                m::WorkspaceOperation::SetNotifications {
                    thread_id: thread.clone(),
                    setting: m::NotificationSetting::Off
                }
            )
            .await
            .result,
            m::WorkspaceResult::Done(_)
        ));
        let mut observer = f.caller.clone();
        let mut address = session(&observer);
        address.session_id = suffix.to_owned().try_into().unwrap();
        observer.caller = m::Caller::Session(address);
        assert!(matches!(
            call(
                &f.host,
                &observer,
                Some(workspace.clone()),
                base + 3,
                m::WorkspaceOperation::JoinThread {
                    thread_id: thread.clone(),
                    role: m::MemberRole::Observer
                }
            )
            .await
            .result,
            m::WorkspaceResult::Done(_)
        ));
        created(
            call(
                &f.host,
                &f.caller,
                Some(workspace.clone()),
                base + 4,
                post(&thread, vec![]),
            )
            .await,
        );
        let actors = r
            .statement_host()
            .read(Statement {
                sql: "SELECT actor_identity_id,identity FROM actor_identities",
                args: vec![],
            })
            .await
            .unwrap();
        let actor: m::ActorIdentityId = actors
            .rows
            .iter()
            .find_map(|row| match (&row[0], &row[1]) {
                (SqlValue::Text(id), SqlValue::Text(identity)) if identity.contains(suffix) => {
                    Some(id.parse().unwrap())
                }
                _ => None,
            })
            .unwrap();
        recipients.push(actor);
    }
    assert_ne!(recipients[0], recipients[1]);
    let windows = r
        .statement_host()
        .read(Statement {
            sql: "SELECT recipient_actor,due_at FROM notification_windows ORDER BY recipient_actor",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        windows.rows.len(),
        2,
        "one independent recipient window per workspace"
    );
    let due_text = windows
        .rows
        .iter()
        .find(|row| row[0] == text(&recipients[0]))
        .unwrap()[1]
        .clone();
    let SqlValue::Text(ref due_string) = due_text else {
        panic!("window deadline")
    };
    let due = chrono::DateTime::parse_from_rfc3339(due_string)
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert!(windows.rows.iter().all(|row| row[1] == due_text));
    let set_wake = |number, due_at| CentralMailRequest {
        request_id: id(number),
        operation: CentralMailOperation::SetWake {
            target: session(&f.caller),
            due_at,
            text: "independent wake".to_owned().try_into().unwrap(),
        },
    };
    let first = f
        .host
        .request_central(&f.caller, set_wake(8830, due))
        .await
        .unwrap();
    let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = first else {
        panic!("due wake refused: {first:?}")
    };
    let later_due = due + chrono::Duration::minutes(5);
    let later = f
        .host
        .request_central(&f.caller, set_wake(8831, later_due))
        .await
        .unwrap();
    let CentralMailResult::Done(CentralMailAnswer::WakeSet {
        wake_id: later_wake,
        ..
    }) = later
    else {
        panic!("later wake refused: {later:?}")
    };
    f.workspace_execute(
        "ALTER TABLE changes RENAME TO outage_changes",
        vec![],
        RowExpectation::Any,
    )
    .await;
    let outage = r
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT change_id FROM changes",
                args: vec![],
            },
        )
        .await;
    assert!(
        matches!(
            outage,
            Err(central_storage::StoreFailure::Unavailable
                | central_storage::StoreFailure::AnswerLost)
        ),
        "the owning W1 store must actually fail its changes read"
    );
    assert!(
        r.inspect_workspace(
            &other,
            Statement {
                sql: "SELECT change_id FROM changes",
                args: vec![]
            }
        )
        .await
        .is_ok(),
        "W2 remains readable"
    );
    f.clock
        .0
        .store(due.timestamp_millis(), std::sync::atomic::Ordering::SeqCst);
    let _alarm_result = r.after_event().await;
    let effects = || Statement {
        sql: "SELECT (SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?),(SELECT COUNT(*) FROM messages WHERE message_kind='notice' AND recipient_actor=? AND source_workspace_id=?),(SELECT COUNT(*) FROM messages WHERE message_kind='notice' AND recipient_actor=? AND source_workspace_id=?)",
        args: vec![
            text(&wake_id),
            text(&recipients[1]),
            text(&other),
            text(&recipients[0]),
            text(&f.workspace),
        ],
    };
    let first_effects = r.statement_host().read(effects()).await.unwrap();
    assert_eq!(
        first_effects.rows,
        vec![vec![
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap())
        ]],
        "the wake fires; the whole window unit waits for the outage to clear"
    );
    let record = r.statement_host().read(Statement { sql: "SELECT state,last_failure FROM central_work_attempts WHERE machine_kind='legacy_notice_windows' AND subject_key='all'", args: vec![] }).await.unwrap();
    assert_eq!(
        record.rows,
        vec![vec![text("failing"), text("other_store_unavailable")]]
    );
    let retained = r
        .statement_host()
        .read(Statement {
            sql: "SELECT due_at FROM notification_windows WHERE recipient_actor=?",
            args: vec![text(&recipients[0])],
        })
        .await
        .unwrap();
    assert_eq!(
        retained.rows,
        vec![vec![due_text.clone()]],
        "transient outage retains its original due time"
    );
    let minimum = r.statement_host().read(Statement { sql: "SELECT MIN(unixepoch(due_at)*1000+CAST(substr(strftime('%f',due_at),4,3) AS INTEGER)) AS earliest_due FROM (SELECT due_at FROM wakes WHERE state='active' UNION ALL SELECT next_due_at FROM automations WHERE state='active' AND next_due_at IS NOT NULL UNION ALL SELECT due_at FROM notification_windows UNION ALL SELECT drain_due_at FROM workspace_drains WHERE drain_due_at IS NOT NULL) WHERE unixepoch(due_at) IS NOT NULL", args: vec![] }).await.unwrap();
    assert_eq!(
        minimum.rows,
        vec![vec![SqlValue::Integer(
            due.timestamp_millis().try_into().unwrap()
        )]]
    );
    assert_eq!(
        r.control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(later_due.timestamp_millis())
    );
    f.clock.0.store(
        later_due.timestamp_millis(),
        std::sync::atomic::Ordering::SeqCst,
    );
    f.caller = super::super::support::fresh_auth(&f.host, &f.caller);
    f.workspace_execute(
        "ALTER TABLE outage_changes RENAME TO changes",
        vec![],
        RowExpectation::Any,
    )
    .await;
    let _retry = r.after_event().await;
    assert_eq!(
        r.statement_host().read(effects()).await.unwrap().rows,
        vec![vec![
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap())
        ]],
        "retry must deliver W1 exactly once without duplicating healthy effects"
    );
    let later_count = r.statement_host().read(Statement { sql: "SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?", args: vec![text(later_wake)] }).await.unwrap();
    assert_eq!(
        later_count.rows,
        vec![vec![SqlValue::Integer(1.try_into().unwrap())]]
    );
    assert_eq!(
        r.control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        None
    );
    f.host.close().await;
}
