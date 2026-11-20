//! Read the physical alarm from real workerd after the borrowed hook settles.
use super::{engine_v2_support::*, support::*};
use central_storage::{ReadOutcome, SqlValue, Statement, StoreFailure};
use workspace_model as m;

#[tokio::test]
async fn runtime_completion_rearms_before_read_refusal_error_and_write_return() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let area = fixture.area(14800).await;
    let due = now(&fixture) + chrono::Duration::minutes(5);
    set_wake(&fixture, 14801, due, "completion alarm").await;
    let read = m::WorkspaceOperation::Show {
        item: m::ItemRef::Area(area.clone()),
    };
    let write = m::WorkspaceOperation::CreateArea {
        name: name("borrowed completion"),
        description: "Test description".to_owned().try_into().unwrap(),
    };

    for (outcome, request_id, operation) in [
        ("read", None, read.clone()),
        ("refusal", None, write.clone()),
        ("write", Some(id(14802)), write),
        ("error", None, read),
    ] {
        if outcome == "error" {
            fixture
                .workspace_batch(
                    "UPDATE workspace_areas SET state='invalid' WHERE area_id=?",
                    vec![text(&area)],
                )
                .await;
        }
        assert!(
            runtime
                .control::<bool>("raw-drop-alarm", &())
                .await
                .unwrap()
        );
        assert_eq!(alarm_ms(&runtime).await, None);
        let before = completion_counts(&runtime).await;
        let executed = "afterWorkspaceExecute";
        let engine = "engineCompletionEntered";
        assert!(
            runtime
                .control::<bool>("hold-central", &executed)
                .await
                .unwrap()
        );
        let applied =
            serde_json::json!({"workspace":fixture.workspace,"point":"afterStoreApplied"});
        let is_write = request_id.is_some();
        if is_write {
            assert!(runtime.control::<bool>("hold", &applied).await.unwrap());
        }
        let pending = runtime.request(
            &fixture.caller,
            m::WorkspaceRequest {
                request_id,
                workspace_id: Some(fixture.workspace.clone()),
                operation,
            },
        );
        tokio::pin!(pending);
        let retained = if is_write {
            tokio::select! {
                biased;
                reply = &mut pending => panic!("write returned before its applied pause: {reply:?}"),
                held = runtime.control::<bool>("until-held", &applied) => assert!(held.unwrap()),
            }
            assert_eq!(completion_counts(&runtime).await, before);
            let turns: [bool; 2] = runtime
                .control("try-completion-turns", &fixture.workspace)
                .await
                .unwrap();
            assert!(!turns[1], "the write still holds its workspace turn");
            let rows = runtime
                .inspect_workspace(&fixture.workspace, applied_create_query())
                .await
                .unwrap();
            assert!(runtime.control::<bool>("release", &applied).await.unwrap());
            Some(applied_create(rows))
        } else {
            None
        };
        tokio::select! {
            biased;
            reply = &mut pending => panic!("{outcome} returned before its execute pause: {reply:?}"),
            held = runtime.control::<bool>("until-held-central", &executed) => assert!(held.unwrap()),
        }
        assert_eq!(
            completion_counts(&runtime).await,
            before,
            "{outcome}: completion started before execute returned"
        );
        let turns: [bool; 2] = runtime
            .control("try-completion-turns", &fixture.workspace)
            .await
            .unwrap();
        assert_eq!(
            turns,
            [true, true],
            "{outcome}: execute released both turns"
        );

        // A successful write may already publish its drain preset during execute.
        let alarm_after_execute = alarm_ms(&runtime).await;
        if outcome != "write" {
            assert_eq!(alarm_after_execute, None);
        }

        assert!(
            runtime
                .control::<bool>("hold-central", &engine)
                .await
                .unwrap()
        );
        assert!(
            runtime
                .control::<bool>("release-central", &executed)
                .await
                .unwrap()
        );
        tokio::select! {
            biased;
            reply = &mut pending => panic!("{outcome} returned before the engine paused: {reply:?}"),
            held = runtime.control::<bool>("until-held-central", &engine) => assert!(held.unwrap()),
        }
        assert_eq!(
            completion_counts(&runtime).await,
            (before.0 + 1, before.1 + 1),
            "{outcome}"
        );
        let turns: [bool; 2] = runtime
            .control("try-completion-turns", &fixture.workspace)
            .await
            .unwrap();
        assert_eq!(
            turns,
            [true, true],
            "{outcome}: both turns are free at engine entry"
        );
        assert_eq!(
            alarm_ms(&runtime).await,
            alarm_after_execute,
            "completion has not published while held"
        );
        assert!(
            futures_util::FutureExt::now_or_never(pending.as_mut()).is_none(),
            "{outcome} replied while completion was held"
        );
        assert!(
            runtime
                .control::<bool>("release-central", &engine)
                .await
                .unwrap()
        );
        let reply = pending.await;
        if let Some(expected) = retained {
            assert_eq!(reply.as_ref().unwrap().result, expected);
        }

        match (outcome, reply.map(|reply| reply.result)) {
            ("read", Ok(m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { .. })))
            | ("refusal", Ok(m::WorkspaceResult::Refused(m::RefusalReason::InvalidInput { .. })))
            | ("write", Ok(m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })))
            | ("error", Err(StoreFailure::Corrupt { .. })) => {}
            (_, reply) => panic!("unexpected {outcome} result: {reply:?}"),
        }
        assert_eq!(
            completion_counts(&runtime).await,
            (before.0 + 1, before.1 + 1),
            "{outcome}"
        );
        assert_eq!(
            alarm_ms(&runtime).await,
            Some(due.timestamp_millis()),
            "{outcome}"
        );
    }
    fixture.host.close().await;
}

async fn completion_counts(runtime: &Runtime) -> (u64, u64) {
    (
        runtime.control("completion-attempts", &()).await.unwrap(),
        runtime
            .control("engine-completion-entries", &())
            .await
            .unwrap(),
    )
}

fn applied_create(rows: ReadOutcome) -> m::WorkspaceResult {
    let [row] = rows.rows.as_slice() else {
        panic!("the write and its answer must exist once")
    };
    let [SqlValue::Text(ident), SqlValue::Text(answer)] = row.as_slice() else {
        panic!("the durable row and answer must be text")
    };
    let answer = serde_json::from_str(answer).unwrap();
    assert!(matches!(
        &answer,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { item: m::ItemRef::Area(area), .. }) if area.to_string() == *ident
    ));
    answer
}

fn applied_create_query() -> Statement {
    Statement {
        sql: "SELECT area_id,answer FROM workspace_areas,workspace_request_log WHERE name=? AND request_id=?",
        args: vec![text("borrowed completion"), text(id::<m::RequestId>(14802))],
    }
}
