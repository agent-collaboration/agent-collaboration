//! Two-root producer for the held-subscription acceptance cell.
use super::ProofResult;
use collaboration_client::CollaborationClient;
use collaboration_client::board::{
    BoardCreateRequest, BoardId, Description, Identity, ParticipantRole, PostId, PostReferences,
    ProjectCreateRequest, ProjectId, ResourceName, SubscriptionMode, SubscriptionPolicyPatch,
    SubscriptionScope, SubscriptionTimingPatch, ThreadCreateRequest, ThreadJoinRequest,
    ThreadPostRequest, WhenIdle, WorkspaceCreateRequest, WorkspaceId,
};
use collaboration_client::protocol::{SessionRef, ThreadSubscribeRequest};
use serde_json::json;

pub(super) async fn board_two_thread_subscription_push(
    client: &CollaborationClient,
    sender: &SessionRef,
    target: &SessionRef,
    markers: [&str; 2],
    resources: &mut super::owned_resources::OwnedMatrixResources,
) -> ProofResult<Vec<message_board::ThreadId>> {
    let actor: Identity = serde_json::from_value(json!({"kind":"session","session":sender}))?;
    let workspace_id = WorkspaceId::generate();
    client
        .workspace_create(WorkspaceCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: message_board::ProtocolVersion::CURRENT,
            resubmits: None,
            workspace_id: workspace_id.clone(),
            name: ResourceName::try_from(format!("Held matrix {}", workspace_id.as_str()))?,
            description: Description::try_from("Two-root held subscription proof".to_owned())?,
            actor: actor.clone(),
            acting_for: None,
        })
        .await?;
    let reader: Identity = serde_json::from_value(json!({"kind":"session","session":target}))?;
    let mut roots: Vec<message_board::ThreadId> = Vec::with_capacity(markers.len());
    for (index, _) in markers.iter().enumerate() {
        let board_id = BoardId::generate();
        client
            .board_create(BoardCreateRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                resubmits: None,
                kind: message_board::BoardKind::Area {},
                board_id: board_id.clone(),
                workspace_id: workspace_id.clone(),
                name: ResourceName::try_from(format!("Held board {}", board_id.as_str()))?,
                description: Description::try_from("Held batch recipient proof".to_owned())?,
                actor: actor.clone(),
                acting_for: None,
            })
            .await?;

        let project_id = ProjectId::generate();
        client
            .project_create(ProjectCreateRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                resubmits: None,
                project_id: project_id.clone(),
                board_id: board_id.clone(),
                title: ResourceName::try_from(format!("Held batch {index}"))?,
                actor: actor.clone(),
                acting_for: None,
            })
            .await?;

        client.role_grant(serde_json::from_value(json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1,"resubmits":null,"observedEpoch":null,"actor":actor,"actingFor":null,"projectId":project_id,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project_id},"role":"lead","holder":actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}))?).await?;
        let root_id = PostId::generate();
        resources.register_actor_root(actor.clone(), root_id.clone().into())?;
        client
            .thread_create(ThreadCreateRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                resubmits: None,
                observed_epoch: None,
                addressees: Vec::new(),
                about: message_board::ThreadSubject::Project {
                    line: "Delivery matrix discussion".to_owned().try_into()?,
                },
                post_id: root_id.clone(),
                project_id: project_id.clone(),
                actor: actor.clone(),
                acting_for: None,
                text: collaboration_client::board::PostText::try_from(format!(
                    "Held matrix root {index}"
                ))?,
                references: PostReferences::try_from(Vec::new())?,
                role: Some(ParticipantRole::Orchestrator),
                watch: true,
            })
            .await?;
        resources.register_actor_root(reader.clone(), root_id.clone().into())?;
        client
            .thread_join(ThreadJoinRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                resubmits: None,
                observed_epoch: None,
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
        client
            .thread_subscribe(ThreadSubscribeRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                resubmits: None,
                observed_epoch: None,
                acting_for: None,
                actor: reader.clone(),
                scope: SubscriptionScope::thread(root_id.clone().into()).into(),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Deliver),
                    when_idle: Some(WhenIdle::Hold),
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(1),
                        cap_seconds: Some(10),
                    },
                    ..SubscriptionPolicyPatch::default()
                },
            })
            .await?;
        roots.push(root_id.into());
    }
    for (thread_id, marker) in roots.iter().cloned().zip(markers) {
        client
            .thread_post(ThreadPostRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                resubmits: None,
                observed_epoch: None,
                addressees: Vec::new(),
                post_id: PostId::generate(),
                placement: message_board::PostTarget::Thread {
                    thread: thread_id.into(),
                },
                actor: actor.clone(),
                acting_for: None,
                text: collaboration_client::board::PostText::try_from(marker.to_owned())?,
                references: PostReferences::try_from(Vec::new())?,
            })
            .await?;
    }
    Ok(roots)
}
