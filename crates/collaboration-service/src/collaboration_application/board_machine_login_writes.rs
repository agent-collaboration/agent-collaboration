//! Real local admission and durable Fleet outbox remain behind the public login gate.
use board_sync_protocol::{BoardClientAnswer, BoardPayload};
use message_board::{
    ActorOrigin, AuthorityRole, BoardOperation, EpochNumber, FeedCursor, FeedPosition,
    ProjectRequest, ProjectState, ProtocolVersion, Record, RecordEnvelope, RecordScope, RecordTime,
    RecordedActor, RequestDecision, RequestResult, RoleAssignment, RoleAssignmentProps,
    RoleSubject, SubjectRevision, WorkspaceCreateRequest,
};

use super::{Fixture, PROJECT, REQUEST, SERVICE, WORKSPACE, actor, assert_not_logged_in};

fn origin() -> ActorOrigin {
    ActorOrigin {
        installation: SERVICE.to_owned().try_into().unwrap(),
        machine: "Fixture machine".to_owned().try_into().unwrap(),
    }
}
async fn held_project(
    fixture: &Fixture,
) -> std::sync::Arc<collaboration_runtime::NativeProjectStore> {
    let registry = fixture.board.registry.as_ref().unwrap();
    let project = PROJECT.to_owned().try_into().unwrap();
    let store = registry.open_project(&project).await.unwrap();
    let now = runtime_primitives::SyncTime::from_unix_millis(1_000);
    registry
        .database(&project)
        .await
        .unwrap()
        .observe_registration(
            fleet_sync_protocol::LeaseStatus {
                database: fleet_sync_protocol::DatabaseId::new(
                    uuid::Uuid::parse_str(PROJECT).unwrap(),
                )
                .unwrap(),
                epoch: fleet_sync_protocol::Epoch::new(1).unwrap(),
                holder: fleet_sync_protocol::PeerId::new(
                    uuid::Uuid::parse_str("019f0000-0000-7000-8000-000000000003").unwrap(),
                )
                .unwrap(),
                state: fleet_sync_protocol::LeaseState::Held,
                expires_at: runtime_primitives::SyncTime::from_unix_millis(600_000),
                remaining_ms: 599_000,
            },
            now,
        )
        .await
        .unwrap();
    let mut write = store.begin_admission(origin()).await.unwrap();
    let cursor = FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: FeedPosition::ZERO,
    };
    let envelope = RecordEnvelope {
        id: project.clone().into(),
        scope: RecordScope {
            workspace: WORKSPACE.to_owned().try_into().unwrap(),
            board: Some(
                "019f0000-0000-7000-8000-000000000018"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            ),
            project: Some(project.clone()),
        },
        actor: RecordedActor::declared(actor()),
        request: None,
        position: write.record_placement(cursor),
        time: RecordTime::Router(chrono::DateTime::from_timestamp_millis(1_000).unwrap()),
    };
    let project_envelope = envelope.clone();
    write
        .append_record(Record::Project(ProjectState {
            envelope,
            project_id: project,
            description: "Fixture project".to_owned().try_into().unwrap(),
            revision: SubjectRevision::INITIAL,
            closed_at: None,
        }))
        .await
        .unwrap();
    write.commit().await.unwrap();
    let mut write = store.begin_admission(origin()).await.unwrap();
    let assignment_id = "019f0000-0000-7000-8000-000000000017"
        .to_owned()
        .try_into()
        .unwrap();
    let role = RoleAssignment::new(RoleAssignmentProps {
        envelope: RecordEnvelope {
            id: assignment_id.clone().into(),
            ..project_envelope
        },
        assignment_id,
        subject: RoleSubject::Project(PROJECT.to_owned().try_into().unwrap()),
        role: AuthorityRole::Advisor,
        holder: RecordedActor::declared(actor()),
        granted_by: RecordedActor::declared(actor()),
        model: None,
        lineage: None,
        ended: None,
        owner_grant: None,
    })
    .unwrap();
    write
        .append_record(Record::RoleAssignment(role))
        .await
        .unwrap();
    write.commit().await.unwrap();
    store
}

#[tokio::test]
async fn machine_login_gate_prevents_a_local_write_and_allows_its_real_commit_after_login() {
    let fixture = Fixture::new().await;
    let store = held_project(&fixture).await;
    let request = ProjectRequest {
        request_id: REQUEST.to_owned().try_into().unwrap(),
        project_id: PROJECT.to_owned().try_into().unwrap(),
        actor: actor(),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::ProjectWatch {},
    };

    assert_not_logged_in(
        fixture
            .board
            .project_write(request.clone())
            .await
            .unwrap_err(),
    );
    let mut before = store.snapshot().await.unwrap();
    assert!(before.project_watch(&actor()).await.unwrap().is_none());
    assert!(
        before
            .record(&request.request_id.clone().into())
            .await
            .unwrap()
            .is_none()
    );
    before.finish().await.unwrap();
    fixture
        .state
        .send_replace(collaboration_contracts::MachineLoginState::LoggedIn);

    let reply = fixture.board.project_write(request.clone()).await.unwrap();

    assert!(matches!(
        reply.result,
        message_board::public_replies::RequestResult::Decided(
            message_board::public_replies::RequestDecision::Accepted { .. }
        )
    ));
    let mut after = store.snapshot().await.unwrap();
    assert!(after.project_watch(&actor()).await.unwrap().is_some());
    assert!(matches!(
        after.record(&request.request_id.into()).await.unwrap(),
        Some(Record::Request(_))
    ));
    after.finish().await.unwrap();
}

#[tokio::test]
async fn machine_login_gate_prevents_outgoing_fleet_admission_and_allows_durable_queueing_after_login()
 {
    let fixture = Fixture::new().await;
    let request = WorkspaceCreateRequest {
        request_id: REQUEST.to_owned().try_into().unwrap(),
        protocol_version: ProtocolVersion::CURRENT,
        workspace_id: "019f0000-0000-7000-8000-000000000019"
            .to_owned()
            .try_into()
            .unwrap(),
        name: "New workspace".to_owned().try_into().unwrap(),
        description: "Fixture".to_owned().try_into().unwrap(),
        actor: actor(),
        acting_for: None,
    };
    let admission = fixture.board.admission().unwrap();

    assert_not_logged_in(
        fixture
            .board
            .workspace_create(request.clone())
            .await
            .unwrap_err(),
    );
    assert!(admission.fleet.pending_requests().await.unwrap().is_empty());
    fixture
        .state
        .send_replace(collaboration_contracts::MachineLoginState::LoggedIn);

    let reply = fixture
        .board
        .workspace_create(request.clone())
        .await
        .unwrap();

    assert!(matches!(
        reply.result,
        message_board::public_replies::RequestResult::PendingUnreachable
    ));
    let pending = admission.fleet.pending_requests().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending.first().unwrap().request.request_id,
        request.request_id
    );
}

#[tokio::test]
async fn machine_login_gate_is_local_to_board_families_and_never_enters_typed_forwarded_replies() {
    let fixture = Fixture::new().await;
    let _store = held_project(&fixture).await;
    let request: ProjectRequest = serde_json::from_value(serde_json::json!({
        "requestId":REQUEST,"projectId":PROJECT,"actor":{"kind":"human","humanId":"fixture-owner"},"actingFor":null,"protocolVersion":1,
        "operation":{"kind":"threadCreate","postId":"019f0000-0000-7000-8000-000000000020","about":{"kind":"project","subject":{"line":"Forwarded fixture"}},"text":"Forwarded body","references":[],"participation":null,"watch":false,"addressees":[]}
    })).unwrap();
    assert_not_logged_in(
        fixture
            .board
            .project_write(request.clone())
            .await
            .unwrap_err(),
    );

    // server_calls::answer_server_call uses this admission capability directly,
    // after decoding the central-verified post, rather than BoardOperations.
    let reply = fixture
        .board
        .admission()
        .unwrap()
        .submit_forwarded_post(&request, origin())
        .await
        .unwrap();

    assert!(matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let answer = BoardClientAnswer::ForwardedPost(BoardPayload::encode(&reply).unwrap());
    let wire = serde_json::to_string(&answer).unwrap();
    assert!(!wire.contains("notLoggedIn"));
    assert!(!wire.contains("runFleetLogin"));
    let decoded: BoardClientAnswer = serde_json::from_str(&wire).unwrap();
    let BoardClientAnswer::ForwardedPost(payload) = decoded else {
        panic!("forwarded-post answer");
    };
    let restored: message_board::WriteReply = payload.decode().unwrap();
    assert_eq!(restored, reply);
}
