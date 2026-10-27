#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Holder-side admission consumes a fixture verified origin; central routing remains G3.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::json;

#[tokio::test]
async fn forwarded_posts_keep_the_declared_actor_and_original_per_observation_origin_on_replay()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000002".to_owned())?,
        machine: MachineLabel::try_from("fixture enrolled sender".to_owned())?,
    };
    let remote_actor = json!({"kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000003","endpointId":"codex-local"},"sessionId":"declared-session"}});
    accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"advisor","holder":remote_actor,"model":"fixture-model","lineage":"fixture-lineage","expectedRevision":1,"ownerGrant":null}))).await?);
    let actors = [fixture.actor.clone(), remote_actor];
    for actor in actors {
        let post = PostId::generate();
        let participation = actor
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .filter(|kind| *kind == "session")
            .map(|_| "participant");
        let mut value=fixture.request(&project,json!({"kind":"threadCreate","postId":post,"about":{"kind":"project","subject":{"line":"forwarded discussion"}},"text":"from another installation","references":[],"participation":participation,"watch":false,"addressees":[]}));
        value
            .as_object_mut()
            .ok_or("request")?
            .insert("actor".to_owned(), actor);
        let request: ProjectRequest = serde_json::from_value(value)?;
        let reply = fixture
            ._admission
            .submit_forwarded_post(&request, origin.clone())
            .await?;
        assert!(
            matches!(
                reply.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "{reply:?}"
        );
        let response = PostId::generate();
        let mut response_value=fixture.request(&project,json!({"kind":"threadPost","postId":response,"threadId":post,"text":"forwarded reply","references":[],"addressees":[]}));
        response_value
            .as_object_mut()
            .ok_or("reply request")?
            .insert("actor".to_owned(), serde_json::to_value(&request.actor)?);
        let response_request: ProjectRequest = serde_json::from_value(response_value)?;
        let response_reply = fixture
            ._admission
            .submit_forwarded_post(&response_request, origin.clone())
            .await?;
        assert!(
            matches!(
                response_reply.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "{response_reply:?}"
        );
        let store = fixture
            ._registry
            .existing_project(&project)
            .await?
            .ok_or("project")?;
        let mut read = store.snapshot().await?;
        let before = read.high_water();
        for reference in [
            post.clone().into(),
            request.request_id.clone().into(),
            response.into(),
            response_request.request_id.clone().into(),
        ] {
            let record = read.record(&reference).await?.ok_or("forwarded record")?;
            assert_eq!(record.envelope().position.origin(), Some(&origin));
            match (&record.envelope().actor, &request.actor) {
                (
                    RecordedActor::Human { human_id: actual },
                    Identity::Human { human_id: declared },
                ) => assert_eq!(actual, declared),
                (
                    RecordedActor::Session { session: actual },
                    Identity::Session { session: declared },
                ) => assert_eq!(actual, declared),
                actors => panic!("forwarded actor was restamped: {actors:?}"),
            }
        }
        read.finish().await?;
        let replay = fixture
            ._admission
            .submit_forwarded_post(&request, fixture.record_origin()?)
            .await?;
        assert_eq!(replay.result, reply.result);
        let response_replay = fixture
            ._admission
            .submit_forwarded_post(&response_request, fixture.record_origin()?)
            .await?;
        assert_eq!(response_replay.result, response_reply.result);
        let read = store.snapshot().await?;
        assert_eq!(read.high_water(), before);
        read.finish().await?;
        drop(store);
    }
    fixture.stop().await
}

#[tokio::test]
async fn forwarded_admission_refuses_operations_outside_the_post_family_without_effects()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let request:ProjectRequest=serde_json::from_value(fixture.request(&project,json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":workstream,"title":"not a forwarded post","body":"","blockedBy":[],"relatedTo":[]})))?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    assert!(matches!(
        fixture
            ._admission
            .submit_forwarded_post(&request, fixture.record_origin()?)
            .await,
        Err(collaboration_service::ProjectAdmissionError::InvalidRecord(
            _
        ))
    ));
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), before);
    assert!(
        read.record(&request.request_id.clone().into())
            .await?
            .is_none()
    );
    read.finish().await?;
    drop(store);
    fixture.stop().await
}
