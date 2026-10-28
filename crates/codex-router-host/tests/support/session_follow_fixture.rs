//! Board requests used to observe the real Host follow attachment.
#![cfg(test)]
use super::registered_route_project::{
    RegisteredRouteProject, assert_board_accepted, settled_creation,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{SessionRef, ThreadSubscribeRequest};
use message_board::{
    Identity, ParticipantRole, PostId, PostReferences, PostText, ProtocolVersion, RequestId,
    SubscriptionMode, SubscriptionPolicyPatch, SubscriptionScope, SubscriptionTimingPatch,
    ThreadId, ThreadJoinRequest, ThreadPostRequest,
};
use serde_json::json;

pub(super) async fn post_thread_activity_for_sessions(
    client: &CollaborationClient,
    readers: [SessionRef; 2],
    inputs: RegisteredRouteProject,
) {
    let owner = inputs.project.actor.clone();
    let project = inputs.project.project_id.clone();
    assert_board_accepted(
        &settled_creation(|| client.workspace_create(inputs.workspace.clone())).await,
    );
    assert_board_accepted(&settled_creation(|| client.board_create(inputs.board.clone())).await);
    assert_board_accepted(
        &settled_creation(|| client.project_create(inputs.project.clone())).await,
    );
    assert_board_accepted(&client.role_grant(serde_json::from_value(json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":owner,"actingFor":null,"projectId":project,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":owner,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}})).expect("role request")).await.expect("route lead"));
    let root = PostId::generate();
    let thread = ThreadId::from(root.clone());
    assert_board_accepted(
        &client
            .thread_create(message_board::ThreadCreateRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                post_id: root,
                project_id: project,
                about: message_board::ThreadSubject::Project {
                    line: "Route discussion".to_owned().try_into().expect("subject"),
                },
                role: None,
                watch: false,
                actor: owner.clone(),
                acting_for: None,
                text: PostText::try_from("Route root".to_owned()).expect("root text"),
                references: PostReferences::try_from(Vec::new()).expect("references"),
                addressees: Vec::new(),
            })
            .await
            .expect("root post"),
    );
    for target in readers {
        let reader: Identity = serde_json::from_value(json!({"kind":"session", "session": target}))
            .expect("session reader");
        assert_board_accepted(
            &client
                .thread_join(ThreadJoinRequest {
                    request_id: RequestId::generate(),
                    protocol_version: ProtocolVersion::CURRENT,
                    thread: (thread.clone()).into(),
                    actor: reader.clone(),
                    role: ParticipantRole::Participant,
                    watch: true,
                    mode: None,
                    when_idle: None,
                    replace: None,
                    note: None,
                })
                .await
                .expect("reader joined"),
        );
        assert_board_accepted(
            &client
                .thread_subscribe(ThreadSubscribeRequest {
                    request_id: RequestId::generate(),
                    protocol_version: ProtocolVersion::CURRENT,
                    actor: reader,
                    acting_for: None,
                    scope: SubscriptionScope::thread(thread.clone()).into(),
                    policy: SubscriptionPolicyPatch {
                        mode: Some(SubscriptionMode::Deliver),
                        timing: SubscriptionTimingPatch {
                            quiet_seconds: Some(0),
                            cap_seconds: Some(0),
                        },
                        ..SubscriptionPolicyPatch::default()
                    },
                })
                .await
                .expect("session subscription"),
        );
    }
    assert_board_accepted(
        &client
            .thread_post(ThreadPostRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                post_id: PostId::generate(),
                placement: message_board::PostTarget::Thread {
                    thread: thread.into(),
                },
                actor: owner,
                acting_for: None,
                text: PostText::try_from("Route reply".to_owned()).expect("reply text"),
                references: PostReferences::try_from(Vec::new()).expect("references"),
                addressees: Vec::new(),
            })
            .await
            .expect("reply post"),
    );
}
