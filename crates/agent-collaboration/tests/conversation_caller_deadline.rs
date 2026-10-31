//! Controlled caller expiry detaches the real ACP conversation without cancellation.
use codex_router_test_support::FixtureResult;
use collaboration_client::{
    AcpConversation, CollaborationAccess, ConversationCreatePromptRequest,
    ConversationCreateRequest, ConversationEnd, ConversationPromptRequest, PublicPromptContent,
};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn controlled_caller_deadline_detaches_without_cancel_and_preserves_target() -> FixtureResult
{
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let service = "00000000-0000-4000-8000-000000000001";
    let endpoint = json!({"serviceId":service,"endpointId":"codex-local"});
    let identity = collaboration_service::ApplicationFixture::new(service, service)?
        .with_endpoints(vec![serde_json::from_value(json!({
            "endpoint":endpoint,"label":"caller deadline fixture",
            "availability":{"state":"available","observedAt":"2026-10-29T00:00:00Z"},
            "channels":[{"kind":"acp","transport":"unixJsonLines","path":"acp.sock",
                "schemaDigest":format!("sha256:{}",collaboration_protocol::ACP_SCHEMA_DIGEST)}]
        }))?])?;
    let served = collaboration_mcp::test_support::ServedCollaborationApi::start(
        root.path(),
        identity.build(),
    )?;
    let listener = tokio::net::UnixListener::bind(root.path().join("acp.sock"))?;
    let (submitted, submission) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        for (method, result) in [
            (
                "initialize",
                json!({"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}),
            ),
            ("session/new", json!({"sessionId":"deadline-thread"})),
        ] {
            let request: Value =
                serde_json::from_str(&lines.next_line().await?.ok_or("setup frame missing")?)?;
            if request.get("method") != Some(&json!(method)) {
                return Err("caller deadline fixture received the wrong setup method".into());
            }
            writer
                .write_all(
                    format!(
                        "{}\n",
                        json!({"jsonrpc":"2.0","id":request.get("id"),"result":result})
                    )
                    .as_bytes(),
                )
                .await?;
        }
        let prompt: Value =
            serde_json::from_str(&lines.next_line().await?.ok_or("prompt frame missing")?)?;
        if prompt.get("method") != Some(&json!("session/prompt"))
            || prompt.pointer("/params/sessionId") != Some(&json!("deadline-thread"))
        {
            return Err("caller deadline fixture did not receive the exact prompt".into());
        }
        submitted
            .send(())
            .map_err(|_| "caller stopped before actual prompt")?;
        // Hold the reply forever. Only the caller's controlled deadline can detach.
        let next = lines.next_line().await?;
        drop(writer);
        if next.is_some() {
            return Err("caller deadline sent session/cancel or replayed a request".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let target: collaboration_protocol::SessionRef = serde_json::from_value(json!({
        "endpoint":endpoint,"sessionId":"requester"
    }))?;
    let directory = root.path().to_owned();
    let (release_clock, clock_released) = std::sync::mpsc::channel();
    let clock_guard =
        tokio::task::spawn_blocking(move || clock_released.recv_timeout(Duration::from_secs(2)));
    // Keep real handshake IO from advancing paused timers. Only this test moves
    // the caller deadline, after the peer has received the actual prompt.
    tokio::time::pause();
    let mut running = tokio::spawn(async move {
        AcpConversation::create_and_prompt(
            &CollaborationAccess::api(&directory),
            ConversationCreatePromptRequest {
                create: ConversationCreateRequest {
                    operation_id: collaboration_protocol::OperationId::generate(),
                    endpoint: target.endpoint.clone(),
                    cwd: directory,
                    session: None,
                    fork: None,
                    model: Some("gpt-5.6-sol".to_owned()),
                    effort: Some("low".to_owned()),
                    access: Some("workspace-write".to_owned()),
                    created_by: Some(target.clone()),
                    approver: Some(target),
                    root_message_id: None,
                },
                prompt: ConversationPromptRequest {
                    message: PublicPromptContent::HumanUser {
                        text: "held prompt".to_owned().try_into()?,
                    },
                    effort: None,
                    timeout_seconds: 1,
                },
            },
            CancellationToken::new(),
        )
        .await
        .map_err(Into::<Box<dyn std::error::Error + Send + Sync>>::into)
    });
    tokio::time::timeout(Duration::from_secs(2), submission)
        .await
        .map_err(|error| {
            format!(
                "actual prompt receipt: {error}; caller_finished={}, peer_finished={}",
                running.is_finished(),
                peer.is_finished()
            )
        })??;
    let pending_before_expiry = futures_util::poll!(&mut running).is_pending();
    tokio::time::advance(Duration::from_millis(1001)).await;
    let outcome = tokio::time::timeout(Duration::from_secs(2), &mut running).await;
    tokio::time::resume();
    let _released = release_clock.send(());
    clock_guard.await??;
    if outcome.is_err() {
        running.abort();
        let _joined = running.await;
    }
    let peer_result = tokio::time::timeout(Duration::from_secs(2), peer).await;
    served.stop().await?;
    let result = outcome.map_err(|error| format!("controlled caller deadline: {error}"))???;
    peer_result.map_err(|error| format!("peer EOF after detach: {error}"))???;
    if !pending_before_expiry {
        return Err("held prompt settled before its caller deadline".into());
    }
    if result.end != ConversationEnd::Detached {
        return Err(format!("caller deadline did not detach: {:?}", result.end).into());
    }
    if String::from(result.target.session_id) != "deadline-thread" {
        return Err("caller deadline changed its exact target".into());
    }
    if result.permission_required {
        return Err("caller deadline invented a permission requirement".into());
    }
    Ok(())
}
