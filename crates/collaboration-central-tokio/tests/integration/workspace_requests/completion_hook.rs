//! Every executing request reaches the borrowed hook before the caller sees its answer.
use super::{engine_v2_support::*, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, ReadOutcome, SqlValue, Statement, StatementBatch, StoreFailure,
};
use fleet_runtime::PausePoint;
use router_clock::RuntimeClock;
use workspace_model as m;

#[tokio::test]
async fn runtime_completion_rearms_before_read_refusal_error_and_write_return() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let area = fixture.area(14800).await;
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    set_wake(&fixture, 14801, due, "completion alarm").await;
    let read = m::WorkspaceOperation::Show {
        item: m::ItemRef::Area(area.clone()),
    };
    let write = m::WorkspaceOperation::CreateArea {
        name: name("borrowed completion"),
        description: "Test description".to_owned().try_into().unwrap(),
    };
    let store = runtime.runtime().statement_host();

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
        assert!(matches!(
            store
                .run_batch(StatementBatch {
                    statements: vec![],
                    alarm: AlarmUpdate::SetFromQuery(Statement {
                        sql: "SELECT NULL",
                        args: vec![]
                    }),
                })
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
        assert_eq!(alarm_ms(&runtime), None);
        let execution = runtime.runtime();
        let counts = || {
            (
                execution.completion_attempts(),
                execution.engine_completion_entries(),
            )
        };
        let before = counts();
        let pauses = execution.pause_points();
        let mut executed = pauses.hold_central(PausePoint::AfterWorkspaceExecute);
        let applied = request_id
            .is_some()
            .then(|| pauses.hold(&fixture.workspace, PausePoint::AfterStoreApplied));
        let pending = runtime.request(
            &fixture.caller,
            m::WorkspaceRequest {
                request_id,
                workspace_id: Some(fixture.workspace.clone()),
                operation,
            },
        );
        tokio::pin!(pending);
        let retained = if let Some(mut applied) = applied {
            tokio::select! {
                biased;
                reply = &mut pending => panic!("write returned before its applied pause: {reply:?}"),
                () = applied.until_held() => {}
            }
            assert_eq!(
                counts(),
                before,
                "completion started before execute finished"
            );
            assert!(
                execution
                    .workspace_turns()
                    .try_take(&fixture.workspace)
                    .is_none()
            );
            let rows = execution
                .inspect_workspace(&fixture.workspace, applied_create_query())
                .await
                .unwrap();
            applied.release();
            Some(applied_create(rows))
        } else {
            None
        };
        tokio::select! {
            biased;
            reply = &mut pending => panic!("{outcome} returned before its execute pause: {reply:?}"),
            () = executed.until_held() => {}
        }
        assert_eq!(
            counts(),
            before,
            "{outcome}: completion started before execute returned"
        );
        assert!(execution.try_central_turn().is_some());
        assert!(
            execution
                .workspace_turns()
                .try_take(&fixture.workspace)
                .is_some()
        );

        // A successful write may already publish its drain preset during execute.
        let alarm_after_execute = alarm_ms(&runtime);
        if outcome != "write" {
            assert_eq!(alarm_after_execute, None);
        }

        let mut completing = pauses.hold_central(PausePoint::EngineCompletionEntered);
        executed.release();
        tokio::select! {
            biased;
            reply = &mut pending => panic!("{outcome} returned before the engine paused: {reply:?}"),
            () = completing.until_held() => {}
        }
        assert_eq!(counts(), (before.0 + 1, before.1 + 1), "{outcome}");
        assert!(execution.try_central_turn().is_some());
        assert!(
            execution
                .workspace_turns()
                .try_take(&fixture.workspace)
                .is_some()
        );
        assert_eq!(
            alarm_ms(&runtime),
            alarm_after_execute,
            "completion has not published while held"
        );
        assert!(
            futures_util::FutureExt::now_or_never(pending.as_mut()).is_none(),
            "{outcome} replied while completion was held"
        );
        completing.release();
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
        assert_eq!(counts(), (before.0 + 1, before.1 + 1), "{outcome}");
        assert_eq!(
            alarm_ms(&runtime),
            Some(due.timestamp_millis()),
            "{outcome}"
        );
    }
    fixture.host.close().await;
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
