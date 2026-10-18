//! A lost activity scope must recover another project's pending work through native probes.
use super::super::{SubscriptionClock, subscription_service::OwnerObservation};
use super::owner_fixture::*;
use message_board::*;
use serde_json::json;
use std::time::Duration;

async fn write_project(
    fixture: &OwnerFixture,
    project: &ProjectId,
    actor: Identity,
    operation: BoardOperation,
) {
    let reply = fixture
        .admission
        .submit_at(
            &ProjectRequest {
                request_id: RequestId::generate(),
                project_id: project.clone(),
                actor,
                acting_for: None,
                observed_epoch: Some(EpochNumber::try_from(1).unwrap()),
                resubmits: None,
                protocol_version: ProtocolVersion::CURRENT,
                operation,
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    assert!(matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
}

#[tokio::test]
async fn lagged_activity_rescans_the_reader_and_recovers_a_different_project() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let original = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = original.snapshot().await.unwrap();
    let project = read
        .record(&fixture.project.clone().into())
        .await
        .unwrap()
        .unwrap();
    let board = project.envelope().scope.board.clone().unwrap();
    read.finish().await.unwrap();
    let second = ProjectId::generate();
    let create = serde_json::from_value(json!({"requestId":RequestId::generate(),"protocolVersion":1,"resubmits":null,"actingFor":null,"actor":human("board-owner"),"boardId":board,"projectId":second,"title":"second project"})).unwrap();
    assert!(matches!(
        fixture
            .admission
            .create_project(&create)
            .await
            .unwrap()
            .result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let post_id = PostId::generate();
    let root = ThreadId::from(post_id.clone());
    write_project(
        &fixture,
        &second,
        human("board-owner"),
        BoardOperation::ThreadCreate {
            post_id,
            about: crate::test_subject_workstream::create(&fixture.registry, &second)
                .await
                .unwrap(),
            text: PostText::try_from("Other root".to_owned()).unwrap(),
            references: Vec::new(),
            participation: Some(ParticipantRole::Orchestrator),
            watch: false,
            addressees: Vec::new(),
        },
    )
    .await;
    write_project(
        &fixture,
        &second,
        fixture.reader.clone(),
        BoardOperation::ThreadJoin {
            thread_id: root.clone(),
            role: ParticipantRole::Participant,
            watch: true,
            mode: Some(SubscriptionMode::Poll),
            when_idle: Some(WhenIdle::Hold),
            replace: None,
            note: None,
        },
    )
    .await;
    let runtime = fixture.runtime().await;
    runtime.owner_barrier(&fixture.reader).await;
    let store = fixture
        .registry
        .existing_project(&second)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let Some(Record::Post(mut post)) = read
        .record(&PostId::try_from(root.as_str().to_owned()).unwrap().into())
        .await
        .unwrap()
    else {
        panic!("root post");
    };
    read.finish().await.unwrap();
    post.post_id = PostId::generate();
    post.envelope.id = post.post_id.clone().into();
    post.envelope.request = None;
    post.actor = human("missed-writer");
    post.envelope.actor.actor = ActorKind::Declared(post.actor.clone());
    post.envelope.router_time = fixture.clock.now();
    post.placement = Placement::Thread {
        thread_id: root.clone(),
    };
    post.text = PostText::try_from("activity whose signal was lost".to_owned()).unwrap();
    let mut write = store.begin_admission().await.unwrap();
    // This fixture deliberately loses the second project's signal after a real commit.
    write.append_record(Record::Post(post)).await.unwrap();
    write.commit().await.unwrap();
    // A current-thread burst cannot be drained until this producer yields.
    for _ in 0..300 {
        fixture.registry.notify_activity(&fixture.project);
    }
    tokio::time::timeout(
        Duration::from_secs(5),
        runtime.observe(|event| matches!(event, OwnerObservation::ActivityLagged)),
    )
    .await
    .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    let record = fixture
        .store
        .lock()
        .await
        .get_thread_subscription_record(
            &fixture.reader,
            &SubscriptionScope::thread(root.clone()),
            fixture.clock.now(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record
            .roots()
            .iter()
            .find(|pending| pending.thread_id() == &root)
            .unwrap()
            .pending_count(),
        1
    );
    runtime.close().await;
}
