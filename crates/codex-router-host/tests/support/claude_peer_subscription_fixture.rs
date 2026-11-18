//! Real Board/automation storage and authenticated Claude peer composition for resume proof.
use crate::board_fixture::native_registry;
#[path = "claude_peer_subscription_clock.rs"]
mod observed_clock;
#[path = "claude_peer_subscription_transport.rs"]
mod peer_transport;
use collaboration_protocol::{EndpointId as PeerEndpointId, EndpointRef, SessionRef, UuidIdentity};
use collaboration_runtime::FleetClient;
use collaboration_runtime::ProjectStoreRegistry;
use collaboration_service::CollaborationApplication;

use message_board::public_replies::{RequestDecision, RequestResult, WriteReply};
use message_board::{SessionId as BoardSessionId, SessionRef as BoardSessionRef, *};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{error::Error, os::unix::fs::PermissionsExt as _, path::Path, time::Duration};

pub type ProofResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

pub const SESSION_ID: &str = "claude-subscription-reader";
pub const ROOT_THREAD_TEXT: &str = "Claude peer resume proof";
pub const POSTED_MESSAGE_TEXT: &str = "A reply arrives while Claude is closed";
pub const PEER_TOKEN: &str = "0123456789abcdef0123456789abcdef";
pub const PRESENCE_RECHECK: Duration = Duration::from_secs(30);
pub const OWNER_EVENT_TIMEOUT: Duration = Duration::from_secs(5);

const SERVICE_ID: &str = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
const ENDPOINT_ID: &str = "claude-local";

pub struct PeerMessageFrames {
    pub authentication: Value,
    pub user: Value,
    pub extra_frame: Option<String>,
}

fn peer_target() -> ProofResult<SessionRef> {
    Ok(SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from(SERVICE_ID.to_owned())?,
            endpoint_id: PeerEndpointId::try_from(ENDPOINT_ID.to_owned())?,
        },
        session_id: collaboration_protocol::SessionId::try_from(SESSION_ID.to_owned())?,
    })
}

fn board_reader_identity() -> ProofResult<Identity> {
    Ok(Identity::Session {
        session: BoardSessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from(SERVICE_ID.to_owned())?,
                endpoint_id: message_board::EndpointId::try_from(ENDPOINT_ID.to_owned())?,
            },
            session_id: BoardSessionId::try_from(SESSION_ID.to_owned())?,
        },
    })
}

fn human_identity(name: &str) -> ProofResult<Identity> {
    Ok(Identity::Human {
        human_id: name.to_owned().try_into()?,
    })
}

async fn create_project_thread_and_subscription(
    application: &CollaborationApplication,
    reader: &Identity,
    thread: &ThreadId,
    fleet: &FleetClient,
    registry: &ProjectStoreRegistry,
) -> ProofResult<ProjectId> {
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let writer = human_identity("peer-resume-test-writer")?;
    let request = |fields: Value| -> ProofResult<Value> {
        let mut request = json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":writer,"actingFor":null});
        request
            .as_object_mut()
            .ok_or("request")?
            .extend(fields.as_object().ok_or("fields")?.clone());
        Ok(request)
    };
    require_accepted(&application.board().workspace_create(serde_json::from_value(request(json!({"workspaceId":workspace,"name":"Peer resume proof workspace","description":""}))?)?).await?)?;
    require_accepted(&application.board().board_create(serde_json::from_value(request(json!({"workspaceId":workspace,"boardId":board,"name":"Peer resume proof board","description":"","kind":{"kind":"area"}}))?)?).await?)?;
    let creation = serde_json::from_value(request(
        json!({"projectId":project,"boardId":board,"title":"Peer resume proof project"}),
    )?)?;
    // Directory acceptance and the persisted grant are explicit component inputs.
    // Peer delivery below is real; this fixture does not observe central enrollment.
    native_registry::prepare_registered_creation(
        fleet,
        registry,
        &creation,
        &ActorOrigin {
            installation: ServiceId::try_from(SERVICE_ID.to_owned())?,
            machine: MachineLabel::try_from("claude-peer-resume-fixture".to_owned())?,
        },
    )
    .await?;
    require_accepted(&application.board().project_create(creation).await?)?;
    require_accepted(&application.board().role_grant(serde_json::from_value(request(json!({"projectId":project,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":writer,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}))?)?).await?)?;
    require_accepted(&application.board().thread_create(serde_json::from_value(request(json!({"projectId":project,"postId":thread,"about":{"kind":"project","subject":{"line":"Peer resume proof"}},"text":ROOT_THREAD_TEXT,"references":[],"role":null,"watch":false,"addressees":[]}))?)?).await?)?;
    require_accepted(
        &application
            .board()
            .thread_join(ThreadJoinRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                thread: (thread.clone()).into(),
                actor: reader.clone(),
                role: ParticipantRole::Participant,
                watch: true,
                mode: Some(SubscriptionMode::Deliver),
                when_idle: Some(WhenIdle::Hold),
                replace: None,
                note: None,
            })
            .await?,
    )?;
    require_accepted(
        &application
            .board()
            .project_write(ProjectRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                project_id: project.clone(),
                actor: reader.clone(),
                acting_for: None,
                operation: BoardOperation::ThreadSubscribe {
                    scope: SubscriptionScope::thread(thread.clone()),
                    policy: SubscriptionPolicyPatch {
                        mode: Some(SubscriptionMode::Deliver),
                        when_idle: Some(WhenIdle::Hold),
                        timing: SubscriptionTimingPatch {
                            quiet_seconds: Some(0),
                            cap_seconds: Some(900),
                        },
                        ..SubscriptionPolicyPatch::default()
                    },
                },
            })
            .await?,
    )?;
    Ok(project)
}

fn require_accepted(reply: &WriteReply) -> ProofResult {
    if matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ) {
        Ok(())
    } else {
        Err(format!("native fixture admission refused: {reply:?}").into())
    }
}
