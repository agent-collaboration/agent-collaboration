//! Read the physical alarm from real workerd after the borrowed hook settles.
use super::{engine_v2_support::*, support::*};
use central_storage::StoreFailure;
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
        let before: u64 = runtime.control("completion-attempts", &()).await.unwrap();

        let reply = runtime
            .request(
                &fixture.caller,
                m::WorkspaceRequest {
                    request_id,
                    workspace_id: Some(fixture.workspace.clone()),
                    operation,
                },
            )
            .await;

        match (outcome, reply.map(|reply| reply.result)) {
            ("read", Ok(m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { .. })))
            | ("refusal", Ok(m::WorkspaceResult::Refused(m::RefusalReason::InvalidInput { .. })))
            | ("write", Ok(m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })))
            | ("error", Err(StoreFailure::Corrupt { .. })) => {}
            (_, reply) => panic!("unexpected {outcome} result: {reply:?}"),
        }
        let after: u64 = runtime.control("completion-attempts", &()).await.unwrap();
        assert_eq!(after, before + 1, "{outcome}");
        assert_eq!(
            alarm_ms(&runtime).await,
            Some(due.timestamp_millis()),
            "{outcome}"
        );
    }
    fixture.host.close().await;
}
