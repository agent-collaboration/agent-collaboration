//! Real Router APIs, two installation stores, authenticated central and mail custody.
//! Only the final external session carrier is a local Unix peer fixture; no model runs.
use board_central_test_support::{EnrolledTestMachine, TestCentral};
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    OperationId, SessionRef, WakeFailureReason, WakeMutationRequest, WakeRefusal, WakeSendRequest,
    WakeShowRequest, WakeState,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

#[path = "support/registered_route_project.rs"]
#[allow(clippy::expect_used)] // Existing route fixture asserts fixed inputs and bounded settlement.
mod registered_route_project;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(20);

fn origin(root: &Path) -> TestResult<message_board::ActorOrigin> {
    Ok(message_board::ActorOrigin {
        installation: String::from(collaboration_runtime::load_service_identity(root)?)
            .try_into()?,
        machine: "remote wake fixture".to_owned().try_into()?,
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

fn request(creator: &SessionRef, recipient: &SessionRef) -> TestResult<WakeSendRequest> {
    Ok(serde_json::from_value(json!({
        "operationId":OperationId::generate(),"requestedBy":{"kind":"session", "endpoint":creator.endpoint,"sessionId":creator.session_id},
        "replace":false,"message":{"target":recipient,
        "content":{"kind":"agent","sender":creator,"text":"A remotely owned wake"},
        "delivery":"auto"},"timing":{"kind":"after","seconds":60},
        "expiry":{"kind":"after","seconds":120}
    }))?)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Behavior assertions stay distinct from propagated fixture failures.
async fn remote_wake_is_owned_and_fires_on_the_recipient_installation() -> TestResult {
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
    let offline_creator = target(&source, "creator")?;
    let mut known_remote_wake = None;
    let result = async {
        let sender = CollaborationClient::connect(&a,"remote-wake-test","1").await?;
        let receiving = CollaborationClient::connect(&b,"remote-wake-test","1").await?;
        let creator = target(&source,"creator")?;
        let to = target(&recipient,"recipient")?;
        eprintln!("REMOTE_WAKE_STAGE create");
        let first_request=request(&creator,&to)?;
        let sent = tokio::time::timeout(BOUND,sender.send_wakeup(first_request.clone())).await??;
        eprintln!("REMOTE_WAKE_STAGE recipient-read");
        let owned = receiving.read_wakeup(WakeShowRequest {
            wakeup_id: sent.definition.wakeup_id.clone(),
        }).await.map_err(|error|format!("remote wake was not created on target installation: {error}"))?;
        assert_eq!(owned.definition.message.target,to);
        assert_eq!(serde_json::to_value(owned.definition.created_by)?,json!({"kind":"session","endpoint":creator.endpoint,"sessionId":creator.session_id}));
        let id=sent.definition.wakeup_id.clone();
        known_remote_wake=Some(id.clone());
        eprintln!("REMOTE_WAKE_STAGE connected-message");
        enrolled.duplicate_next_mail_notice();
        let message=collaboration_client::MessageSendRequest {
            item_id:Some(collaboration_protocol::MailItemId::generate()),target:to.clone(),
            message:collaboration_client::PublicMessageContent::HumanUser{text:"mail after initial fetch".to_owned().try_into()?},
            delivery:collaboration_protocol::MessageDelivery::Auto,generation_guard:None,
        };
        sender.send_message(message.clone()).await?;
        let (_,direct)=tokio::time::timeout(BOUND,peer_received.recv()).await.map_err(|_|"connected recipient did not receive a new direct message after its initial fetch")?.ok_or("peer stopped")?;
        assert!(direct.ok_or("missing direct message")?.contains("mail after initial fetch"));
        sender.send_message(message.clone()).await?;
        eprintln!("REMOTE_WAKE_STAGE connected-control");
        let cancellation=collaboration_protocol::MessageCancelRequest{
            cancel_id:Some(collaboration_protocol::MailItemId::generate()),
            target:message.item_id.ok_or("missing direct item identity")?,
            sender:collaboration_protocol::MessageCancelSender::HumanUser,
        };
        let cancelled=tokio::time::timeout(BOUND,async{
            loop {
                let outcome=sender.message_cancel(cancellation.clone()).await?;
                if !matches!(outcome.outcome,collaboration_protocol::RequestedCancelOutcome::Pending){
                    return Ok::<_,collaboration_client::ClientError>(outcome);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await??;
        assert!(matches!(cancelled.outcome,collaboration_protocol::RequestedCancelOutcome::TooLate{..}));
        eprintln!("REMOTE_WAKE_STAGE connected-notification");
        let project=registered_route_project::route_project_requests();
        registered_route_project::assert_board_accepted(&registered_route_project::settled_creation(||sender.workspace_create(project.workspace.clone())).await);
        registered_route_project::assert_board_accepted(&registered_route_project::settled_creation(||sender.board_create(project.board.clone())).await);
        registered_route_project::assert_board_accepted(&registered_route_project::settled_creation(||sender.project_create(project.project.clone())).await);
        registered_route_project::assert_board_accepted(&sender.role_grant(serde_json::from_value(json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1,"actor":project.project.actor,"actingFor":null,"projectId":project.project.project_id,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project.project.project_id},"role":"lead","holder":project.project.actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}))?).await?);
        let addressed:message_board::SessionRef=to.clone().try_into()?;
        let thread=message_board::ThreadCreateRequest{
            request_id:message_board::RequestId::generate(),protocol_version:message_board::ProtocolVersion::CURRENT,
            post_id:message_board::PostId::generate(),project_id:project.project.project_id,
            about:message_board::ThreadSubject::Project{line:"connected notification".to_owned().try_into()?},role:None,watch:false,
            actor:project.project.actor,acting_for:None,text:"connected notification".to_owned().try_into()?,
            references:Vec::new().try_into()?,addressees:vec![message_board::NotifyTarget::Actor(message_board::Identity::Session{session:addressed})],
        };
        registered_route_project::assert_board_accepted(&sender.thread_create(thread).await?);
        let (_,notification)=tokio::time::timeout(BOUND,peer_received.recv()).await.map_err(|_|"connected recipient did not receive a project notification")?.ok_or("peer stopped")?;
        let notification=peer_record(&receiving,&to,&notification.ok_or("missing notification")?).await?;
        assert_eq!(notification.kind,collaboration_protocol::PushKind::BoardNotification);
        assert_eq!(enrolled.duplicated_mail_notices(),1,"actual incoming hint was not duplicated");
        eprintln!("REMOTE_WAKE_STAGE remote-show");
        let remote_shown=sender.read_wakeup(WakeShowRequest{wakeup_id:id.clone()}).await?;
        assert_eq!(remote_shown.definition.message.target,to);
        eprintln!("REMOTE_WAKE_STAGE remote-list");
        let listed=sender.list_wakeups(collaboration_protocol::AutomationPageRequest{cursor:None,limit:50.try_into()?}).await?;
        assert_eq!(listed.records.len(),1,"sender created a second local timer");
        assert!(listed.records.iter().any(|row|row.definition.wakeup_id==id && row.definition.message.target==to));
        eprintln!("REMOTE_WAKE_STAGE creation-replay");
        let replay=sender.send_wakeup(first_request).await?;
        assert_eq!(replay.definition.wakeup_id,id,"same operation must replay owning creation");
        eprintln!("REMOTE_WAKE_STAGE occupied-refusal");
        let refused=sender.send_wakeup(request(&creator,&to)?).await.err().ok_or("second wake admitted")?;
        let collaboration_client::WakeClientError::Rejected(refused)=refused else{return Err("missing typed refusal".into());};
        let expected_creator=json!({"kind":"session","endpoint":creator.endpoint,"sessionId":creator.session_id});
        let WakeFailureReason::Refused(WakeRefusal::WakeExists{wakeup,creator:actual_creator})=refused.reason else{return Err(format!("wrong refusal: {refused:?}").into());};
        assert_eq!(wakeup,id);assert_eq!(serde_json::to_value(actual_creator)?,expected_creator);
        eprintln!("REMOTE_WAKE_STAGE foreign-replacement");
        let mut outsider=request(&target(&source,"other-creator")?,&to)?;outsider.replace=true;
        let refused=sender.send_wakeup(outsider).await.err().ok_or("other creator replaced wake")?;
        assert!(matches!(refused,collaboration_client::WakeClientError::Rejected(error) if matches!(error.reason,WakeFailureReason::Refused(WakeRefusal::WakeExists{..}))));
        eprintln!("REMOTE_WAKE_STAGE creator-replacement");
        let mut replacement=request(&creator,&to)?;replacement.replace=true;
        let replacement=sender.send_wakeup(replacement).await?;
        assert_ne!(replacement.definition.wakeup_id,id);
        assert_eq!(sender.read_wakeup(WakeShowRequest{wakeup_id:id}).await?.state,WakeState::Cancelled);
        let id=replacement.definition.wakeup_id;
        let mutation=||WakeMutationRequest{operation_id:OperationId::generate(),wakeup_id:id.clone()};
        eprintln!("REMOTE_WAKE_STAGE pause");
        assert_eq!(sender.pause_wakeup(mutation()).await?.wakeup.state,WakeState::Paused);
        eprintln!("REMOTE_WAKE_STAGE paused-wait");
        let waiting=CollaborationClient::connect(&a,"remote-paused-wait","1").await?.subscribe_wakeup(WakeShowRequest{wakeup_id:id.clone()}).await?;
        assert!(matches!(waiting.wait_until_first_fire().await,Err(collaboration_client::WakeWaitError::Paused{..})));
        eprintln!("REMOTE_WAKE_STAGE resume");
        assert_eq!(sender.resume_wakeup(mutation()).await?.wakeup.state,WakeState::Active);
        eprintln!("REMOTE_WAKE_STAGE cancel");
        assert_eq!(sender.cancel_wakeup(mutation()).await?.wakeup.state,WakeState::Cancelled);
        assert_eq!(receiving.read_wakeup(WakeShowRequest{wakeup_id:id}).await?.state,WakeState::Cancelled);
        eprintln!("REMOTE_WAKE_STAGE fire-create");
        let mut firing=request(&creator,&to)?;
        firing.timing=serde_json::from_value(json!({"kind":"after","seconds":2}))?;
        let firing=sender.send_wakeup(firing).await?;
        let fire_id=firing.definition.wakeup_id;
        eprintln!("REMOTE_WAKE_STAGE fire-wait");
        let waiting=CollaborationClient::connect(&a,"remote-fire-wait","1").await?.subscribe_wakeup(WakeShowRequest{wakeup_id:fire_id.clone()}).await?;
        let fire=tokio::time::timeout(BOUND,waiting.wait_until_first_fire()).await??;
        assert_eq!(fire.wakeup_id,fire_id);
        eprintln!("REMOTE_WAKE_STAGE peer-push");
        let (auth,user)=tokio::time::timeout(BOUND,peer_received.recv()).await?.ok_or("recipient peer stopped")?;
        let auth:serde_json::Value=serde_json::from_str(&auth.ok_or("missing peer auth")?)?;
        assert_eq!(auth,json!({"type":"auth","token":"0123456789abcdef0123456789abcdef"}));
        let user:serde_json::Value=serde_json::from_str(&user.ok_or("missing peer push")?)?;
        let line=user.pointer("/message/content").and_then(serde_json::Value::as_str).ok_or("push content missing")?;
        assert!(line.contains("A remotely owned wake"),"{line}");
        assert!(line.contains(&format!("router://{}/push/",String::from(recipient.service_id().clone()))),"push was not recorded at target: {line}");
        let current=sender.read_wakeup(WakeShowRequest{wakeup_id:fire_id.clone()}).await?;
        assert!(current.first_fire.is_some());
        let record=tokio::time::timeout(BOUND,async {
            loop {
                let record=peer_record(&receiving,&to,&user.to_string()).await?;
                if record.delivery_state==collaboration_protocol::PushDeliveryState::Delivered {return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(record);}
                tokio::task::yield_now().await;
            }
        }).await??;
        assert_eq!(record.kind,collaboration_protocol::PushKind::Wake);
        let retained=sender.cancel_wakeup(WakeMutationRequest{operation_id:OperationId::generate(),wakeup_id:fire_id.clone()}).await?;
        assert_eq!(retained.dispatched_deliveries.len(),1,"remote mutation lost native delivery evidence");
        let delivery=retained.dispatched_deliveries.first().ok_or("retained delivery missing")?;
        assert_eq!(delivery.target,to);
        assert!(matches!(&delivery.evidence,collaboration_protocol::DeliveryEvidence::Accepted{receipt,..} if receipt.outcome==collaboration_protocol::DeliveryOutcome::PeerMessageWritten));

        assert!(peer_received.try_recv().is_err(),"wake pushed twice");
        assert_eq!(central.session_connections(),1,"sender unexpectedly reconnected");
        assert_eq!(enrolled.session_connections(),1,"recipient unexpectedly reconnected");
        let mut expired=request(&creator,&to)?;
        expired.expiry=serde_json::from_value(json!({"kind":"at","at":(chrono::Utc::now()-chrono::Duration::seconds(1)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}))?;
        assert!(matches!(sender.send_wakeup(expired).await,Err(collaboration_client::WakeClientError::Rejected(error)) if matches!(error.reason,WakeFailureReason::DeadlineExpired)));

        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    recipient.shutdown().await?;
    let recovered = if result.is_ok() {
        async {
        let source_client=CollaborationClient::connect(&a,"offline-mail","1").await?;
        eprintln!("REMOTE_WAKE_STAGE offline-typed-management");
        let id=known_remote_wake.clone().ok_or("missing remote route")?;
        let mut operations=tokio::task::JoinSet::new();
        for operation in 0..4 {
            let client=CollaborationClient::connect(&a,"offline-wake-control","1").await?;
            let id=id.clone();
            operations.spawn(async move {
                let request=WakeMutationRequest{operation_id:OperationId::generate(),wakeup_id:id.clone()};
                let error=match operation {
                    0=>client.read_wakeup(WakeShowRequest{wakeup_id:id}).await.err(),
                    1=>client.pause_wakeup(request).await.err(),
                    2=>client.resume_wakeup(request).await.err(),
                    _=>client.cancel_wakeup(request).await.err(),
                };
                matches!(error,Some(collaboration_client::WakeClientError::Rejected(error)) if matches!(error.reason,WakeFailureReason::RemoteUnavailable))
            });
        }
        while let Some(result)=tokio::time::timeout(Duration::from_secs(25),operations.join_next()).await? {
            assert!(result?,"offline management did not produce a typed owning-machine-unavailable refusal");
        }
        let mut expired_target=offline_target.clone();expired_target.session_id="expired-before-remote-admission".to_owned().try_into()?;
        let mut in_transit=request(&offline_creator,&expired_target)?;
        in_transit.expiry=serde_json::from_value(json!({"kind":"after","seconds":1}))?;
        assert!(matches!(source_client.send_wakeup(in_transit).await,Err(collaboration_client::WakeClientError::Rejected(error)) if matches!(error.reason,WakeFailureReason::DeadlineExpired)));
        let service:message_board::ServiceId=String::from(offline_target.endpoint.service_id.clone()).try_into()?;
        let mailbox=board_central_test_support::ObjectAddress::Mailbox(service.clone());
        let connection=board_central_test_support::ObjectAddress::Connection(service);
        central.runtime_control(mailbox.clone(),board_central_test_support::RuntimeControl::DropNextObjectReply{destination:connection}).await?;
        let item=collaboration_protocol::MailItemId::generate();
        let queued=collaboration_client::MessageSendRequest {
            item_id:Some(item.clone()),target:offline_target.clone(),message:collaboration_client::PublicMessageContent::HumanUser{text:"mail retained while recipient offline".to_owned().try_into()?},
            delivery:collaboration_protocol::MessageDelivery::Auto,generation_guard:None,
        };
        source_client.send_message(queued.clone()).await?;
        source_client.send_message(queued).await?;
        // The real Connection received the mailbox's hint while no Router was attached.
        central.runtime_control(mailbox,board_central_test_support::RuntimeControl::AwaitDroppedObjectReply).await?;
        let restarted=start(&b,enrolled.session_inputs()?).await?;
        let proof=async {
            let (_,frame)=tokio::time::timeout(BOUND,peer_received.recv()).await?.ok_or("offline peer stopped")?;
            let frame=frame.ok_or("offline push missing")?;
            assert!(frame.contains("mail retained while recipient offline"));
            let reader=CollaborationClient::connect(&b,"offline-readback","1").await?;
            let record=peer_record(&reader,&offline_target,&frame).await?;
            assert_eq!(record.push_id.as_str(),item.as_str());
            assert_eq!(record.kind,collaboration_protocol::PushKind::DirectMessage);
            let fresh=source_client.send_wakeup(request(&offline_creator,&offline_target)?).await?;
            source_client.cancel_wakeup(WakeMutationRequest{operation_id:OperationId::generate(),wakeup_id:fresh.definition.wakeup_id}).await?;
            let listed=reader.list_wakeups(collaboration_protocol::AutomationPageRequest{cursor:None,limit:50.try_into()?}).await?;
            assert!(!listed.records.iter().any(|wake|wake.definition.message.target==expired_target),"expired mail created a timer after recipient reconnect");
            assert_eq!(enrolled.session_connections(),2,"offline recovery did not reopen exactly once");
            assert!(peer_received.try_recv().is_err(),"same-ID replay or duplicate hint reinjected a native push");
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        restarted.shutdown().await?;
        proof
    }.await
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
        _ => Err("peer push was not owned locally".into()),
    }
}
