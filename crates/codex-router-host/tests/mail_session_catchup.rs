//! Ordinary mail catch-up retains authentication, duplicate-hint and reconnect proof.
//! Only the final external session carrier is a local Unix peer fixture; no model runs.
use board_central_test_support::{EnrolledTestMachine, TestCentral};
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::CollaborationClient;
use collaboration_protocol::SessionRef;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

#[path = "support/registered_route_project.rs"]
mod registered_route_project;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(20);

fn origin(root: &Path) -> TestResult<message_board::ActorOrigin> {
    Ok(message_board::ActorOrigin {
        installation: String::from(collaboration_runtime::load_service_identity(root)?)
            .try_into()?,
        machine: "mail catch-up fixture".to_owned().try_into()?,
    })
}

async fn start(
    root: &Path,
    session: board_fleet_sync::BoardSessionInputs,
) -> TestResult<CollaborationRuntime> {
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    Ok(CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: Some(session),
        directory: root.to_owned(),
        codex_home: root.to_owned(),
        backend_socket: root.join("unused-native.sock"),
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        native_schema: None,
        peer_registry_directory: Some(root.join("peers")),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await?)
}

fn target(runtime: &CollaborationRuntime, session: &str) -> TestResult<SessionRef> {
    Ok(serde_json::from_value(json!({
        "endpoint":{"serviceId":runtime.service_id(),"endpointId":"claude-local"},
        "sessionId":session
    }))?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Behavior assertions stay distinct from propagated fixture failures."
)]
async fn authenticated_mail_catchup_survives_duplicate_hints_and_reconnect() -> TestResult {
    let root = tempfile::Builder::new().prefix("rw-").tempdir_in("/tmp")?;
    let a = root.path().join("a");
    let b = root.path().join("b");
    for directory in [&a, &b] {
        std::fs::create_dir(directory)?;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        std::fs::create_dir(directory.join("peers"))?;
    }
    let central = TestCentral::start_with_peer(
        &root.path().join("central"),
        &origin(&a)?,
        collaboration_runtime::load_installation_peer(&a)?,
    )
    .await?;
    let enrolled: EnrolledTestMachine = central
        .additional_machine(
            &origin(&b)?,
            collaboration_runtime::load_installation_peer(&b)?,
        )
        .await?;
    let socket = b.join("peers/peer.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let pid = std::process::id();
    std::fs::write(b.join(format!("peers/{pid}.json")),json!({"pid":pid,"sessionId":"recipient","status":"busy","peerProtocol":1,"messagingSocketPath":socket}).to_string())?;
    let digest = Sha256::digest(socket.to_str().ok_or("socket path")?.as_bytes());
    let key = b.join(format!("peers/{pid}.{digest:x}.key"));
    std::fs::write(&key, r#"{"peerToken":"0123456789abcdef0123456789abcdef"}"#)?;
    std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600))?;
    let (peer_sender, mut peer_received) = tokio::sync::mpsc::channel(4);
    let peer_task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let mut lines = BufReader::new(stream).lines();
            let auth = lines.next_line().await?;
            let user = lines.next_line().await?;
            if peer_sender.send((auth, user)).await.is_err() {
                break;
            }
        }
        Ok::<_, std::io::Error>(())
    });
    let source = start(&a, central.session_inputs()?).await?;
    let recipient = start(&b, enrolled.session_inputs()?).await?;
    let offline_target = target(&recipient, "recipient")?;
    let result = async {
        let sender = CollaborationClient::connect(&a,"mail-catchup-test","1").await?;
        let receiving = CollaborationClient::connect(&b,"mail-catchup-test","1").await?;
        let to = target(&recipient,"recipient")?;
        eprintln!("MAIL_CATCHUP_STAGE connected-message");
        enrolled.duplicate_next_mail_notice();
        let message=collaboration_client::MessageSendRequest {
            item_id:Some(collaboration_protocol::MailItemId::generate()),target:to.clone(),
            message:collaboration_client::PublicMessageContent::HumanUser{text:"mail after initial fetch".to_owned().try_into()?},
            delivery:collaboration_protocol::MessageDelivery::Auto,generation_guard:None,
        };
        sender.send_message(message.clone()).await?;
        let (auth,direct)=tokio::time::timeout(BOUND,peer_received.recv()).await.map_err(|_|"connected recipient did not receive a new direct message after its initial fetch")?.ok_or("peer stopped")?;
        let auth:serde_json::Value=serde_json::from_str(&auth.ok_or("missing peer auth")?)?;
        assert_eq!(auth,json!({"type":"auth","token":"0123456789abcdef0123456789abcdef"}));
        assert!(direct.ok_or("missing direct message")?.contains("mail after initial fetch"));
        sender.send_message(message.clone()).await?;
        eprintln!("MAIL_CATCHUP_STAGE connected-notification");
        let project=registered_route_project::route_project_requests();
        registered_route_project::assert_board_accepted(&registered_route_project::settled_creation(||sender.workspace_create(project.workspace.clone())).await);
        registered_route_project::assert_board_accepted(&registered_route_project::settled_creation(||sender.board_create(project.board.clone())).await);
        registered_route_project::assert_board_accepted(&registered_route_project::settled_creation(||sender.project_create(project.project.clone())).await);
        registered_route_project::assert_board_accepted(&sender.role_grant(serde_json::from_value(json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1,"actor":project.project.actor,"actingFor":null,"projectId":project.project.project_id,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project.project.project_id},"role":"lead","holder":project.project.actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}))?).await?);
        let addressed:message_board::SessionRef=to.clone().try_into()?;
        let thread=message_board::ThreadCreateRequest{
            request_id:message_board::RequestId::generate(),protocol_version:message_board::ProtocolVersion::CURRENT,
            post_id:message_board::PostId::generate(),project_id:project.project.project_id.clone(),
            about:message_board::ThreadSubject::Project{line:"connected notification".to_owned().try_into()?},role:None,watch:false,
            actor:project.project.actor,acting_for:None,text:"connected notification".to_owned().try_into()?,
            references:Vec::new().try_into()?,addressees:vec![message_board::NotifyTarget::Actor(message_board::Identity::Session{session:addressed})],
        };
        registered_route_project::assert_board_accepted(&sender.thread_create(thread).await?);
        // Pre-existing yield-poll retained; the S8 owner tracks its causal-wait follow-up.
        let (_,notification)=tokio::time::timeout(BOUND,async {
            loop {
                source.runtime_test_replicate_project(&project.project.project_id).await?;
                tokio::select! {
                    received = peer_received.recv() => return received.ok_or_else(||std::io::Error::other("peer stopped")),
                    () = tokio::task::yield_now() => {}
                }
            }
        }).await.map_err(|_|"connected recipient did not receive a project notification")??;
        let notification=peer_record(&receiving,&to,&notification.ok_or("missing notification")?).await?;
        assert_eq!(notification.kind,collaboration_protocol::PushKind::BoardNotification);
        assert_eq!(enrolled.duplicated_mail_notices(),1,"actual incoming hint was not duplicated");
        assert!(peer_received.try_recv().is_err(),"duplicate mail hint reinjected a native push");
        assert_eq!(central.session_connections(),1,"sender unexpectedly reconnected");
        assert_eq!(enrolled.session_connections(),1,"recipient unexpectedly reconnected");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    recipient.shutdown().await?;
    let recovered = if result.is_ok() {
        async {
            let source_client = CollaborationClient::connect(&a, "offline-mail", "1").await?;
            let service: message_board::ServiceId =
                String::from(offline_target.endpoint.service_id.clone()).try_into()?;
            let mailbox = board_central_test_support::ObjectAddress::Mailbox(service.clone());
            let connection = board_central_test_support::ObjectAddress::Connection(service);
            central
                .runtime_control(
                    mailbox.clone(),
                    board_central_test_support::RuntimeControl::DropNextObjectReply {
                        destination: connection,
                    },
                )
                .await?;
            let item = collaboration_protocol::MailItemId::generate();
            let queued = collaboration_client::MessageSendRequest {
                item_id: Some(item.clone()),
                target: offline_target.clone(),
                message: collaboration_client::PublicMessageContent::HumanUser {
                    text: "mail retained while recipient offline"
                        .to_owned()
                        .try_into()?,
                },
                delivery: collaboration_protocol::MessageDelivery::Auto,
                generation_guard: None,
            };
            source_client.send_message(queued.clone()).await?;
            source_client.send_message(queued).await?;
            // The real Connection received the mailbox's hint while no Router was attached.
            central
                .runtime_control(
                    mailbox,
                    board_central_test_support::RuntimeControl::AwaitDroppedObjectReply,
                )
                .await?;
            let restarted = start(&b, enrolled.session_inputs()?).await?;
            let proof = async {
                let (_, frame) = tokio::time::timeout(BOUND, peer_received.recv())
                    .await?
                    .ok_or("offline peer stopped")?;
                let frame = frame.ok_or("offline push missing")?;
                assert!(frame.contains("mail retained while recipient offline"));
                let reader = CollaborationClient::connect(&b, "offline-readback", "1").await?;
                let record = peer_record(&reader, &offline_target, &frame).await?;
                assert_eq!(record.push_id.as_str(), item.as_str());
                assert_eq!(record.kind, collaboration_protocol::PushKind::DirectMessage);
                assert_eq!(
                    enrolled.session_connections(),
                    2,
                    "offline recovery did not reopen exactly once"
                );
                assert!(
                    peer_received.try_recv().is_err(),
                    "same-ID replay or duplicate hint reinjected a native push"
                );
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
            }
            .await;
            restarted.shutdown().await?;
            proof
        }
        .await
    } else {
        Ok(())
    };
    peer_task.abort();
    let _ = peer_task.await;
    source.shutdown().await?;
    enrolled.shutdown().await?;
    central.shutdown().await?;
    result?;
    recovered
}

async fn peer_record(
    client: &CollaborationClient,
    target: &SessionRef,
    frame: &str,
) -> TestResult<collaboration_protocol::PushRecord> {
    let message: serde_json::Value = serde_json::from_str(frame)?;
    let line = message
        .pointer("/message/content")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing peer content")?;
    let reference = line
        .split_whitespace()
        .last()
        .ok_or("missing push link")?
        .to_owned();
    match client
        .router_show(collaboration_protocol::PushRecordShowParams {
            caller: target.clone(),
            reference,
        })
        .await?
    {
        collaboration_protocol::MessageShowResult::Local(shown) => {
            if &shown.record.target != target {
                return Err("peer record belongs to another target".into());
            }
            Ok(shown.record)
        }
        collaboration_protocol::MessageShowResult::Remote { .. } => {
            Err("peer push was not owned locally".into())
        }
    }
}
