//! Real registry/socket fixtures at the peer route boundary.

use agent_automation::{
    ClaudeCodePeerEffectEvidence, PeerProcessId, PeerSessionReference, PeerWriteEffect,
    RouteEffectEvidence,
};
use claude_code_peer_messaging::{ClaudeCodePeerSocket, ClaudeCodeSessionRegistry};
use codex_router_host::ClaudeCodePeerDeliveryRoute;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::RouteClaim;
use collaboration_contracts::RoutePresence;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::layer_zero::DeliveryRequest as PreparedDeliveryRequest;
use collaboration_contracts::layer_zero::PreparedPush;
use collaboration_protocol::{
    AttemptId, CodexGeneration, DeliveryClientReceipt, DeliveryCorrelationId, DeliveryOutcome,
    DeliveryRejectionReason, EndpointId, EndpointRef, MessageDelivery, MessageText, PushId,
    SessionId, SessionReachability, SessionRef, UuidIdentity,
};
use collaboration_runtime::SessionDeliveryRouter;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{os::unix::fs::PermissionsExt as _, path::Path, sync::Arc};
use tokio::io::{AsyncBufReadExt as _, BufReader};

struct RecordedPeerEvidence(
    tokio::sync::Mutex<Vec<RouteEffectEvidence<SessionRef, CodexGeneration>>>,
);

impl AttemptEvidenceSink for RecordedPeerEvidence {
    fn record(
        &self,
        evidence: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        Box::pin(async move {
            self.0.lock().await.push(evidence);
            Ok(())
        })
    }
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn target() -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())
                .expect("service ID"),
            endpoint_id: EndpointId::try_from("claude-local".to_owned()).expect("endpoint ID"),
        },
        session_id: SessionId::try_from("fixture-session".to_owned()).expect("session ID"),
    }
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
#[expect(
    clippy::indexing_slicing,
    reason = "Fixed test vectors define these indices."
)]
fn publish_peer(root: &Path, status: Option<&str>) -> std::path::PathBuf {
    let pid = std::process::id();
    let socket_path = root.join("peer.sock");
    let mut record = json!({
        "pid": pid,
        "sessionId": "fixture-session",
        "peerProtocol": 1,
        "messagingSocketPath": socket_path,
    });
    if let Some(status) = status {
        record["status"] = json!(status);
    }
    std::fs::write(root.join(format!("{pid}.json")), record.to_string()).expect("registry fixture");
    let digest = Sha256::digest(socket_path.to_str().expect("socket path").as_bytes());
    let key_path = root.join(format!("{pid}.{digest:x}.key"));
    std::fs::write(
        &key_path,
        json!({"peerToken":"0123456789abcdef0123456789abcdef"}).to_string(),
    )
    .expect("auth fixture");
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
        .expect("private key");
    socket_path
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
#[expect(
    clippy::indexing_slicing,
    reason = "Fixed test vectors define these indices."
)]
fn publish_peer_claim(root: &Path, process_id: u32, name: Option<&str>, cwd: Option<&str>) {
    let mut record = json!({
        "pid": process_id,
        "sessionId": "fixture-session",
        "peerProtocol": 99,
    });
    if let Some(name) = name {
        record["name"] = json!(name);
    }
    if let Some(cwd) = cwd {
        record["cwd"] = json!(cwd);
    }
    std::fs::write(root.join(format!("{process_id}.json")), record.to_string())
        .expect("registry claim fixture");
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn delivery(mode: MessageDelivery) -> PreparedDeliveryRequest {
    let target = target();
    let push_id =
        PushId::try_from(AttemptId::generate().as_str().to_owned()).expect("UUIDv7 push id");
    let correlation =
        DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).expect("push id correlation");
    let line = MessageText::try_from(format!(
        "✉️ sender · \"hello Claude\" · router://{}/push/{}",
        String::from(target.endpoint.service_id.clone()),
        push_id.as_str()
    ))
    .expect("prepared push line");
    PreparedDeliveryRequest {
        payload: PreparedPush {
            push_id,
            line,
            load_policy: collaboration_contracts::LoadPolicy::MayLoad,
        },
        target,
        mode,
        precondition: DeliveryPrecondition::Unpinned,
        correlation,
        attempt: AttemptId::generate(),
    }
}

#[tokio::test]
async fn prepared_push_reaches_peer_route_as_the_exact_router_line() {
    for mode in [
        MessageDelivery::Auto,
        MessageDelivery::Queue,
        MessageDelivery::Steer,
    ] {
        let root = tempfile::tempdir().expect("registry root");
        let socket_path = publish_peer(root.path(), Some("busy"));
        let listener = tokio::net::UnixListener::bind(&socket_path).expect("peer listener");
        let receiver = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accepted peer");
            let mut lines = BufReader::new(stream).lines();
            let _auth: Value =
                serde_json::from_str(&lines.next_line().await.expect("auth line").expect("auth"))
                    .expect("auth JSON");
            serde_json::from_str::<Value>(
                &lines.next_line().await.expect("user line").expect("user"),
            )
            .expect("user JSON")
        });
        let route = Arc::new(ClaudeCodePeerDeliveryRoute::new(
            target().endpoint,
            Arc::new(ClaudeCodeSessionRegistry::new(root.path().to_owned())),
            Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
        ));
        let router = SessionDeliveryRouter::new(vec![route]);
        let evidence = RecordedPeerEvidence(tokio::sync::Mutex::new(Vec::new()));
        let push_id = PushId::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned())
            .expect("UUIDv7 push id");
        let line = MessageText::try_from(format!(
            "✉️ Main · \"S1 is green\" · router://00000000-0000-4000-8000-000000000001/push/{}",
            push_id.as_str()
        ))
        .expect("push line");
        let correlation = DeliveryCorrelationId::try_from(push_id.as_str().to_owned())
            .expect("push id correlation");
        assert_eq!(correlation.as_str(), push_id.as_str());
        let request = PreparedDeliveryRequest {
            payload: PreparedPush {
                push_id,
                line: line.clone(),
                load_policy: collaboration_contracts::LoadPolicy::LoadedOnly,
            },
            target: target(),
            mode,
            precondition: DeliveryPrecondition::Unpinned,
            correlation,
            attempt: AttemptId::generate(),
        };

        let receipt = router
            .deliver(request, &evidence)
            .await
            .expect("prepared push delivery");

        assert_eq!(
            receipt.receipt().outcome.clone(),
            DeliveryOutcome::PeerMessageWritten
        );
        assert_eq!(
            receipt.receipt().reachability,
            Some(SessionReachability::ClaudeCodePeer)
        );
        assert!(matches!(
            receipt.receipt().client,
            Some(DeliveryClientReceipt::ClaudeCodePeer)
        ));
        let user = receiver.await.expect("receiver");
        assert_eq!(user["message"]["content"], line.as_str());
        assert!(
            !user["message"]["content"]
                .as_str()
                .expect("peer message content")
                .contains("For follow-up messages")
        );
    }
}

#[tokio::test]
async fn peer_queue_rejects_but_idle_steer_writes_to_the_live_socket() {
    let root = tempfile::tempdir().expect("registry root");
    let socket_path = publish_peer(root.path(), Some("idle"));
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("peer listener");
    let receiver = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accepted peer");
        let mut lines = BufReader::new(stream).lines();
        let _auth: Value =
            serde_json::from_str(&lines.next_line().await.expect("auth line").expect("auth"))
                .expect("auth JSON");
        serde_json::from_str::<Value>(&lines.next_line().await.expect("user line").expect("user"))
            .expect("user JSON")
    });
    let route = ClaudeCodePeerDeliveryRoute::new(
        target().endpoint,
        Arc::new(ClaudeCodeSessionRegistry::new(root.path().to_owned())),
        Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
    );
    let evidence = RecordedPeerEvidence(tokio::sync::Mutex::new(Vec::new()));

    let queued = route
        .deliver(delivery(MessageDelivery::Queue), &evidence)
        .await
        .expect("queue response");
    assert_eq!(
        queued.unsent_cause(),
        Some(collaboration_contracts::UnsentCause::Refused(
            collaboration_contracts::NativeRefusalCode::Rejection(
                DeliveryRejectionReason::QueueUnsupported
            )
        ))
    );
    let steered = route
        .deliver(delivery(MessageDelivery::Steer), &evidence)
        .await
        .expect("steer response");

    assert!(
        matches!(queued.receipt().outcome.clone(), DeliveryOutcome::Rejected(rejection)
            if rejection.reason == DeliveryRejectionReason::QueueUnsupported)
    );
    assert_eq!(
        steered.receipt().outcome.clone(),
        DeliveryOutcome::PeerMessageWritten
    );
    assert_eq!(receiver.await.expect("receiver")["type"], "user");
    assert_eq!(evidence.0.lock().await.len(), 2);
}

#[tokio::test]
async fn live_unknown_peer_protocol_reports_live_elsewhere() {
    let root = tempfile::tempdir().expect("registry root");
    let socket_path = publish_peer(root.path(), Some("idle"));
    std::fs::write(
        root.path().join(format!("{}.json", std::process::id())),
        json!({
            "pid": std::process::id(),
            "sessionId": "fixture-session",
            "status": "idle",
            "peerProtocol": 99,
            "messagingSocketPath": socket_path,
        })
        .to_string(),
    )
    .expect("unsupported registry entry");
    let route = ClaudeCodePeerDeliveryRoute::new(
        target().endpoint,
        Arc::new(ClaudeCodeSessionRegistry::new(root.path().to_owned())),
        Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
    );
    let evidence = RecordedPeerEvidence(tokio::sync::Mutex::new(Vec::new()));

    let claim = route.claim(&target()).await.expect("claim");
    let receipt = route
        .deliver(delivery(MessageDelivery::Auto), &evidence)
        .await
        .expect("delivery refusal");

    assert_eq!(
        claim.unsent_cause(),
        Some(collaboration_contracts::UnsentCause::for_rejection(
            DeliveryRejectionReason::LiveElsewhere
        ))
    );
    assert_eq!(receipt.unsent_cause(), claim.unsent_cause());
    assert!(matches!(
        claim.into_claim(),
        RouteClaim::LiveElsewhere {
            writable: false,
            detail: Some(reason),
        } if reason.contains("protocol 99")
    ));
    assert!(matches!(
        route.presence(&target()).await.expect("unsupported peer presence"),
        RoutePresence::LiveElsewhere { detail: Some(reason) } if reason.contains("protocol 99")
    ));
    assert!(
        matches!(receipt.receipt().outcome.clone(), DeliveryOutcome::Rejected(rejection)
        if rejection.reason == DeliveryRejectionReason::LiveElsewhere)
    );
    assert!(evidence.0.lock().await.is_empty());
}

#[tokio::test]
async fn registry_read_failure_preserves_the_legacy_receipt_with_processing_failure_cause() {
    let root = tempfile::tempdir().expect("registry root");
    let registry_file = root.path().join("not-a-directory");
    std::fs::write(&registry_file, "registry cannot be a file").expect("forced read error");
    let registry = Arc::new(ClaudeCodeSessionRegistry::new(registry_file));
    let legacy_detail = registry
        .lookup(&target().session_id)
        .expect_err("forced registry read error")
        .to_string();
    let route = Arc::new(ClaudeCodePeerDeliveryRoute::new(
        target().endpoint,
        registry,
        Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
    ));
    let evidence = RecordedPeerEvidence(tokio::sync::Mutex::new(Vec::new()));

    let claim = route.claim(&target()).await.expect("classified claim");
    let direct = route
        .deliver(delivery(MessageDelivery::Auto), &evidence)
        .await
        .expect("direct result");
    let router = SessionDeliveryRouter::new(vec![route]);
    let selected = router
        .deliver(delivery(MessageDelivery::Auto), &evidence)
        .await
        .expect("selected result");

    assert_eq!(
        claim.unsent_cause(),
        Some(collaboration_contracts::UnsentCause::ProcessingFailure)
    );
    assert_eq!(
        serde_json::to_value(claim.into_claim()).expect("claim JSON"),
        json!({
            "kind":"liveElsewhere", "writable":false, "detail":legacy_detail,
        })
    );
    for (result, reachability) in [
        (direct, Some(SessionReachability::ClaudeCodePeer)),
        (selected, None),
    ] {
        assert_eq!(
            result.unsent_cause(),
            Some(collaboration_contracts::UnsentCause::ProcessingFailure)
        );
        let legacy = collaboration_protocol::DeliveryReceipt {
            outcome: DeliveryOutcome::Rejected(collaboration_protocol::DeliveryRejection {
                native_failure: None,
                reason: DeliveryRejectionReason::LiveElsewhere,
                next_action: collaboration_protocol::DeliveryNextAction::InspectTarget,
                client_code: None,
                detail: Some(legacy_detail.clone()),
                claims: None,
            }),
            reachability,
            client: None,
        };
        assert_eq!(
            serde_json::to_value(result.into_receipt()).expect("receipt JSON"),
            serde_json::to_value(legacy).expect("legacy receipt JSON")
        );
    }
    assert!(evidence.0.lock().await.is_empty());
}

#[tokio::test]
async fn ambiguous_live_peer_claims_are_preserved_in_the_rejection() {
    let root = tempfile::tempdir().expect("registry root");
    let first_process_id = std::process::id();
    let second_process_id = std::os::unix::process::parent_id();
    assert_ne!(first_process_id, second_process_id);
    publish_peer_claim(
        root.path(),
        first_process_id,
        Some("first-live-terminal"),
        Some("/workspace/first"),
    );
    publish_peer_claim(root.path(), second_process_id, None, None);

    let route = Arc::new(ClaudeCodePeerDeliveryRoute::new(
        target().endpoint,
        Arc::new(ClaudeCodeSessionRegistry::new(root.path().to_owned())),
        Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
    ));
    let router = SessionDeliveryRouter::new(vec![route]);
    let evidence = RecordedPeerEvidence(tokio::sync::Mutex::new(Vec::new()));
    let receipt = router
        .deliver(delivery(MessageDelivery::Auto), &evidence)
        .await
        .expect("ambiguous route rejection");

    let DeliveryOutcome::Rejected(rejection) = receipt.receipt().outcome.clone() else {
        panic!(
            "ambiguous live records must be rejected: {:?}",
            receipt.receipt().outcome.clone()
        );
    };
    assert_eq!(
        receipt.receipt().reachability,
        Some(SessionReachability::ClaudeCodePeer)
    );
    assert_eq!(rejection.reason, DeliveryRejectionReason::LiveElsewhere);
    let claims = serde_json::to_value(&rejection)
        .expect("rejection encodes")
        .get("claims")
        .cloned()
        .expect("structured peer claims");
    let claims = claims.as_array().expect("claims array");
    assert_eq!(claims.len(), 2);
    assert!(claims.contains(&json!({
        "pid": first_process_id,
        "name": "first-live-terminal",
        "cwd": "/workspace/first"
    })));
    assert!(claims.contains(&json!({
        "pid": second_process_id,
        "name": null,
        "cwd": null
    })));
    assert!(evidence.0.lock().await.is_empty());
}

#[tokio::test]
async fn every_live_registry_status_allows_auto_and_steer_peer_writes() {
    for status in [
        Some("busy"),
        Some("idle"),
        Some("waiting"),
        Some("shell"),
        Some("compacting"),
        None,
    ] {
        for mode in [MessageDelivery::Auto, MessageDelivery::Steer] {
            let root = tempfile::tempdir().expect("registry root");
            let socket_path = publish_peer(root.path(), status);
            let listener = tokio::net::UnixListener::bind(&socket_path).expect("peer listener");
            let receiver = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accepted peer");
                let mut lines = BufReader::new(stream).lines();
                let _auth: Value = serde_json::from_str(
                    &lines.next_line().await.expect("auth line").expect("auth"),
                )
                .expect("auth JSON");
                serde_json::from_str::<Value>(
                    &lines.next_line().await.expect("user line").expect("user"),
                )
                .expect("user JSON")
            });
            let route = ClaudeCodePeerDeliveryRoute::new(
                target().endpoint,
                Arc::new(ClaudeCodeSessionRegistry::new(root.path().to_owned())),
                Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
            );
            let evidence = RecordedPeerEvidence(tokio::sync::Mutex::new(Vec::new()));

            assert_eq!(
                route.presence(&target()).await.expect("peer presence"),
                RoutePresence::Running,
                "status={status:?}"
            );

            let outcome = route
                .deliver(delivery(mode), &evidence)
                .await
                .expect("peer delivery");

            assert_eq!(
                outcome.receipt().outcome.clone(),
                DeliveryOutcome::PeerMessageWritten,
                "status={status:?}, mode={mode:?}"
            );
            let message = receiver.await.expect("peer receives write");
            assert_eq!(message["type"], "user");
            assert_eq!(
                evidence.0.lock().await.len(),
                2,
                "status={status:?}, mode={mode:?}"
            );
        }
    }
}

#[tokio::test]
async fn peer_reconciliation_rejects_evidence_for_another_session() {
    let root = tempfile::tempdir().expect("registry root");
    let route = ClaudeCodePeerDeliveryRoute::new(
        target().endpoint,
        Arc::new(ClaudeCodeSessionRegistry::new(root.path().to_owned())),
        Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
    );
    assert_eq!(
        route
            .presence(&target())
            .await
            .expect("absent peer presence"),
        RoutePresence::NotRunning,
        "a served session with no open terminal is not running"
    );
    let recorded = RouteEffectEvidence::ClaudeCodePeer(ClaudeCodePeerEffectEvidence {
        session_id: PeerSessionReference::try_from("another-session".to_owned())
            .expect("recorded session"),
        process_id: PeerProcessId::try_from(std::process::id()).expect("process"),
        write: PeerWriteEffect::Written,
    });

    let outcome = route
        .reconcile_attempt(AttemptReconciliationContext {
            target: target(),
            prepared_push_id: PushId::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned())
                .expect("UUIDv7 push id"),
            mode: MessageDelivery::Auto,
            recorded,
        })
        .await;

    assert!(outcome.is_err());
}

/// Records each peer write effect, and pauses the attempt once, at Dispatching, before the
/// socket write, until the test lets it go on.
struct PauseAtDispatching {
    writes: tokio::sync::Mutex<Vec<PeerWriteEffect>>,
    reached: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    resume: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}

impl AttemptEvidenceSink for PauseAtDispatching {
    fn record(
        &self,
        evidence: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        Box::pin(async move {
            let RouteEffectEvidence::ClaudeCodePeer(peer) = evidence else {
                return Ok(());
            };
            self.writes.lock().await.push(peer.write);
            if peer.write == PeerWriteEffect::Dispatching {
                if let Some(reached) = self.reached.lock().await.take() {
                    let _ = reached.send(());
                }
                if let Some(resume) = self.resume.lock().await.take() {
                    let _ = resume.await;
                }
            }
            Ok(())
        })
    }
}

/// How a selected peer is gone by the time its socket is written.
#[derive(Clone, Copy, Debug)]
enum PeerGone {
    /// It exited: its registry record and published key are gone.
    Exited,
    /// Its socket stopped listening; its record and key remain.
    SocketClosed,
}

#[tokio::test]
async fn a_peer_gone_between_selection_and_its_write_is_refused_as_not_running_and_never_held() {
    for gone in [PeerGone::Exited, PeerGone::SocketClosed] {
        let root = tempfile::tempdir().expect("registry root");
        let socket_path = publish_peer(root.path(), Some("idle"));
        let listener = tokio::net::UnixListener::bind(&socket_path).expect("peer listener");
        let route = Arc::new(ClaudeCodePeerDeliveryRoute::new(
            target().endpoint,
            Arc::new(ClaudeCodeSessionRegistry::new(root.path().to_owned())),
            Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
        ));
        let router = SessionDeliveryRouter::new(vec![route]);
        let (reached, reached_signal) = tokio::sync::oneshot::channel();
        let (resume_signal, resume) = tokio::sync::oneshot::channel();
        let evidence = PauseAtDispatching {
            writes: tokio::sync::Mutex::new(Vec::new()),
            reached: tokio::sync::Mutex::new(Some(reached)),
            resume: tokio::sync::Mutex::new(Some(resume)),
        };

        // The peer is selected as writable; the attempt pauses before its socket write.
        let attempt = router.deliver(delivery(MessageDelivery::Auto), &evidence);
        tokio::pin!(attempt);
        tokio::select! {
            biased;
            _ = &mut attempt => panic!("{gone:?}: the attempt ended before its write"),
            signal = reached_signal => signal.expect("the attempt reached its write"),
        }
        // The peer goes away, and the attempt goes on.
        drop(listener);
        std::fs::remove_file(&socket_path).expect("socket gone");
        if matches!(gone, PeerGone::Exited) {
            for entry in std::fs::read_dir(root.path()).expect("registry") {
                let path = entry.expect("registry entry").path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "json" || extension == "key")
                {
                    std::fs::remove_file(path).expect("record gone");
                }
            }
        }
        resume_signal.send(()).expect("resume the attempt");
        let result = attempt.await.expect("delivery result");

        assert!(
            matches!(
                &result.receipt().outcome,
                DeliveryOutcome::Rejected(rejection)
                    if rejection.reason == DeliveryRejectionReason::EndpointUnavailable
                        && rejection
                            .detail
                            .as_deref()
                            .is_some_and(|detail| detail.starts_with("target is not running"))
            ),
            "{gone:?}: refused as not running: {:?}",
            result.receipt().outcome
        );
        assert!(
            matches!(
                result.unsent_cause(),
                Some(collaboration_contracts::UnsentCause::Refused(_))
            ),
            "{gone:?}: refused, never holdable: {:?}",
            result.unsent_cause()
        );
        assert_eq!(
            *evidence.writes.lock().await,
            [PeerWriteEffect::Dispatching, PeerWriteEffect::NotDispatched],
            "{gone:?}"
        );
    }
}

#[tokio::test]
async fn a_peer_write_cut_off_after_some_bytes_stays_unknown() {
    let root = tempfile::tempdir().expect("registry root");
    let socket_path = publish_peer(root.path(), Some("idle"));
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("peer listener");
    // The peer reads its auth frame and the start of the message, then goes away.
    let receiver = tokio::spawn(async move {
        use tokio::io::AsyncReadExt as _;
        let (stream, _) = listener.accept().await.expect("accepted peer");
        let mut reader = BufReader::new(stream);
        let mut auth = String::new();
        reader.read_line(&mut auth).await.expect("auth frame");
        let mut first_bytes = [0_u8; 1024];
        reader
            .read_exact(&mut first_bytes)
            .await
            .expect("partial user frame");
    });
    let route = Arc::new(ClaudeCodePeerDeliveryRoute::new(
        target().endpoint,
        Arc::new(ClaudeCodeSessionRegistry::new(root.path().to_owned())),
        Arc::new(ClaudeCodePeerSocket::new(root.path().to_owned())),
    ));
    let router = SessionDeliveryRouter::new(vec![route]);
    let evidence = RecordedPeerEvidence(tokio::sync::Mutex::new(Vec::new()));
    let mut request = delivery(MessageDelivery::Auto);
    request.payload.line = MessageText::try_from(format!(
        "{} {}",
        "x".repeat(900_000),
        request.payload.line.as_str()
    ))
    .expect("a large push line");

    let result = router
        .deliver(request, &evidence)
        .await
        .expect("delivery result");
    receiver.await.expect("receiver");

    assert_eq!(
        result.receipt().outcome,
        DeliveryOutcome::Unknown,
        "a write that may have reached the terminal is neither refused nor held"
    );
}
