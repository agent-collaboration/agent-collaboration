//! Subscription producer and recipient-observed stored-range proof for the delivery matrix.
use super::{ProofContext, ProofResult, SUBSCRIPTION_NOTICE_LABEL};
use collaboration_client::board::{
    BoardCreateRequest, BoardId, Description, Identity, ParticipantRole, PostId, PostReferences,
    ProjectCreateRequest, ProjectId, ResourceName, SubscriptionMode, SubscriptionPolicyPatch,
    SubscriptionScope, SubscriptionTimingPatch, ThreadCreateRequest, ThreadJoinRequest,
    ThreadPostRequest, WorkspaceCreateRequest, WorkspaceId,
};
use collaboration_client::protocol::{SessionRef, ThreadSubscribeRequest};
use serde_json::json;

#[path = "subscription_notice_observer.rs"]
mod notice_observer;
pub(super) use notice_observer::verify_subscription_notice;

pub(super) async fn board_subscription_push(
    proof: &mut ProofContext,
    sender: &SessionRef,
    codex: &SessionRef,
    peer: &SessionRef,
    codex_marker: &str,
    peer_marker: &str,
) -> ProofResult<()> {
    board_subscription_push_targets(proof, sender, &[(codex, codex_marker), (peer, peer_marker)])
        .await
}

pub(super) async fn board_subscription_push_targets(
    proof: &mut ProofContext,
    sender: &SessionRef,
    targets: &[(&SessionRef, &str)],
) -> ProofResult<()> {
    let actor: Identity = serde_json::from_value(json!({"kind":"session","session":sender}))?;
    let workspace_id = WorkspaceId::generate();
    let board_id = BoardId::generate();
    let project_id = ProjectId::generate();
    proof
        .client
        .workspace_create(WorkspaceCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: message_board::ProtocolVersion::CURRENT,
            workspace_id: workspace_id.clone(),
            name: ResourceName::try_from(format!("Delivery matrix {}", workspace_id.as_str()))?,
            description: Description::try_from("Disposable recipient delivery proof".to_owned())?,
            actor: actor.clone(),
            acting_for: None,
        })
        .await?;
    proof
        .client
        .board_create(BoardCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: message_board::ProtocolVersion::CURRENT,
            kind: message_board::BoardKind::Area {},
            board_id: board_id.clone(),
            workspace_id,
            name: ResourceName::try_from(format!("Delivery board {}", board_id.as_str()))?,
            description: Description::try_from("Thread subscription recipient proof".to_owned())?,
            actor: actor.clone(),
            acting_for: None,
        })
        .await?;
    proof
        .client
        .project_create(ProjectCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: message_board::ProtocolVersion::CURRENT,
            project_id: project_id.clone(),
            board_id,
            title: ResourceName::try_from("Delivery".to_owned())?,
            description: None,
            actor: actor.clone(),
            acting_for: None,
        })
        .await?;
    proof.client.role_grant(serde_json::from_value(json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1,"actor":actor,"actingFor":null,"projectId":project_id,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project_id},"role":"lead","holder":actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}))?).await?;
    let root_id = PostId::generate();
    proof
        .client
        .thread_create(ThreadCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: message_board::ProtocolVersion::CURRENT,
            addressees: Vec::new(),
            about: message_board::ThreadSubject::Project {
                line: "Delivery matrix discussion".to_owned().try_into()?,
            },
            post_id: root_id.clone(),
            project_id,
            actor: actor.clone(),
            acting_for: None,
            text: collaboration_client::board::PostText::try_from(
                "Delivery matrix root".to_owned(),
            )?,
            references: PostReferences::try_from(Vec::new())?,
            role: Some(ParticipantRole::Orchestrator),
            watch: true,
        })
        .await?;
    for (target, _) in targets {
        let reader: Identity = serde_json::from_value(json!({"kind":"session","session":target}))?;
        proof
            .client
            .thread_join(ThreadJoinRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                mode: None,
                when_idle: None,
                thread: message_board::ThreadId::from(root_id.clone()).into(),
                actor: reader.clone(),
                role: ParticipantRole::Participant,
                watch: true,
                replace: None,
                note: None,
            })
            .await?;
        proof
            .client
            .thread_subscribe(ThreadSubscribeRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                acting_for: None,
                actor: reader,
                scope: SubscriptionScope::thread(root_id.clone().into()).into(),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Deliver),
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(1),
                        cap_seconds: Some(10),
                    },
                    ..SubscriptionPolicyPatch::default()
                },
            })
            .await?;
    }
    for (_, marker) in targets {
        proof
            .client
            .thread_post(ThreadPostRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                addressees: Vec::new(),
                post_id: PostId::generate(),
                placement: message_board::PostTarget::Thread {
                    thread: message_board::ThreadId::from(root_id.clone()).into(),
                },
                actor: actor.clone(),
                acting_for: None,
                text: collaboration_client::board::PostText::try_from((*marker).to_owned())?,
                references: PostReferences::try_from(Vec::new())?,
            })
            .await?;
    }
    Ok(())
}
