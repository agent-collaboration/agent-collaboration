//! A DM to a Claude Code peer that isn't running fails at once; the Router can't start one.

use claude_code_peer_messaging::{ClaudeCodePeerSocket, ClaudeCodeSessionRegistry};
use codex_router_host::ClaudeCodePeerDeliveryRoute;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::NativeObservedOutcome;
use collaboration_contracts::NativeRouteClaim;
use collaboration_contracts::RoutePresence;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_protocol::{
    CodexGeneration, DeliveryOutcome, DeliveryRejectionReason, EndpointId, EndpointRef,
    MessageDelivery, PushDeliveryState, PushHeaderFacts, PushId, PushKind, PushOrigin,
    PushRecordDraft, SessionId, SessionReachability, SessionRef, UuidIdentity,
};
use collaboration_runtime::SessionDeliveryRouter;
use collaboration_service::BoardAvailability;
use collaboration_service::SubscriptionDeliveryService;
use collaboration_service::SubscriptionDeliveryServiceProps;

use interaction_storage::InteractionStore;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    os::unix::fs::PermissionsExt as _,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::io::{AsyncBufReadExt as _, BufReader};

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn target() -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())
                .expect("service id"),
            endpoint_id: EndpointId::try_from("claude-local".to_owned()).expect("endpoint id"),
        },
        session_id: SessionId::try_from("closed-peer-session".to_owned()).expect("session id"),
    }
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn draft(
    clock: &dyn router_clock::RuntimeClock,
    target: &SessionRef,
    mode: MessageDelivery,
    guard: Option<CodexGeneration>,
) -> PushRecordDraft {
    PushRecordDraft {
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string()).expect("push id"),
        kind: PushKind::DirectMessage,
        origin: PushOrigin::OwnerUnverified,
        origin_router_ref: None,
        target: target.clone(),
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some(format!("resume {mode:?} delivery")),
        activity: None,
        mode: Some(mode),
        guard,
        created_at: clock.utc_now(),
    }
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn generation_guard() -> CodexGeneration {
    serde_json::from_value(json!({
        "serviceEpoch":"00000000-0000-4000-8000-000000000001",
        "generation":1
    }))
    .expect("Codex generation guard")
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn publish_peer(registry: &std::path::Path, socket_path: &std::path::Path) {
    let process_id = std::process::id();
    std::fs::write(
        registry.join(format!("{process_id}.json")),
        json!({
            "pid": process_id,
            "sessionId": "closed-peer-session",
            "status": "idle",
            "peerProtocol": 1,
            "messagingSocketPath": socket_path,
        })
        .to_string(),
    )
    .expect("peer registry record");
    let digest = Sha256::digest(socket_path.to_str().expect("socket path").as_bytes());
    let key_path = registry.join(format!("{process_id}.{digest:x}.key"));
    std::fs::write(
        &key_path,
        json!({"peerToken":"0123456789abcdef0123456789abcdef"}).to_string(),
    )
    .expect("peer authentication record");
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
        .expect("private peer authentication record");
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
async fn start_peer_delivery_service(
    root: &std::path::Path,
    provider_routes: Vec<Arc<dyn SessionDeliveryRoute>>,
) -> (
    std::path::PathBuf,
    Arc<InteractionStore>,
    SubscriptionDeliveryService,
    Arc<SessionDeliveryRouter>,
) {
    let registry = root.join("claude-sessions");
    std::fs::create_dir(&registry).expect("peer registry directory");
    let automation = Arc::new(
        InteractionStore::open(root.join("interaction.sqlite"))
            .await
            .expect("push storage"),
    );
    let route = Arc::new(ClaudeCodePeerDeliveryRoute::new(
        target().endpoint,
        Arc::new(ClaudeCodeSessionRegistry::new(registry.clone())),
        Arc::new(ClaudeCodePeerSocket::new(registry.clone())),
    ));
    let mut routes: Vec<Arc<dyn SessionDeliveryRoute>> = vec![route];
    routes.extend(provider_routes);
    let router = Arc::new(SessionDeliveryRouter::new(routes));
    let delivery: Arc<dyn SessionMessageDelivery> = router.clone();
    let presence: Arc<dyn TargetPresenceProbe> = router.clone();
    let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: Arc::clone(&automation),
        delivery,
        presence,
        machine_identity: MachineIdentity::new(
            target().endpoint.service_id,
            Some("fixture-machine"),
        )
        .expect("machine identity"),
        clock: Arc::new(router_clock::ManualClock::new(
            chrono::DateTime::UNIX_EPOCH + chrono::Duration::seconds(1_792_432_800),
        )),
    });
    service.start().await.expect("reader actor startup");
    (registry, automation, service, router)
}

const NOT_RUNNING_DETAIL: &str =
    "target is not running; open the Claude Code session, then send again";

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
#[expect(clippy::panic, reason = "Fail an unexpected test outcome.")]
fn assert_rejected_not_running(record: &collaboration_protocol::PushRecord) {
    assert_eq!(record.delivery_state, PushDeliveryState::Rejected);
    let DeliveryOutcome::Rejected(rejection) = record
        .last_outcome
        .clone()
        .expect("settled outcome")
        .outcome
    else {
        panic!("a closed peer's DM must be rejected, not held");
    };
    assert_eq!(
        rejection.reason,
        DeliveryRejectionReason::EndpointUnavailable
    );
    assert_eq!(rejection.detail.as_deref(), Some(NOT_RUNNING_DETAIL));
}

#[tokio::test]
async fn closed_peer_fails_auto_dm_at_once_then_a_running_peer_receives_the_next() {
    let directory = tempfile::tempdir().expect("isolated test directory");
    let (registry, automation, service, router) =
        start_peer_delivery_service(directory.path(), Vec::new()).await;
    let target = target();
    assert!(router.supports_delivery_mode(&target, MessageDelivery::Auto));
    assert!(router.supports_delivery_mode(&target, MessageDelivery::Steer));
    assert!(!router.supports_delivery_mode(&target, MessageDelivery::Queue));
    let push_draft = draft(
        service.runtime_clock().as_ref(),
        &target,
        MessageDelivery::Auto,
        None,
    );
    let closed_push_id = push_draft.push_id.clone();
    automation
        .insert_push_record(push_draft)
        .await
        .expect("store the DM before delivery");

    let failed = (service.deliver_direct_message(target.clone(), closed_push_id.clone()))
        .await
        .expect("closed peer DM receipt");

    assert_rejected_not_running(&failed);
    assert!(
        automation
            .list_unsent_direct_messages(&target)
            .await
            .expect("list unsent DMs")
            .is_empty(),
        "nothing is held for a peer the Router can't start"
    );

    // A running peer still receives a new DM; the failed one is never resent.
    let push_draft = draft(
        service.runtime_clock().as_ref(),
        &target,
        MessageDelivery::Auto,
        None,
    );
    let push_id = push_draft.push_id.clone();
    automation
        .insert_push_record(push_draft)
        .await
        .expect("store the DM for the running peer");
    let socket_path = registry.join("peer.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("peer socket");
    publish_peer(&registry, &socket_path);
    let received = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("peer accepted one write");
        let mut lines = BufReader::new(stream).lines();
        let auth: Value = serde_json::from_str(
            &lines
                .next_line()
                .await
                .expect("auth line")
                .expect("auth frame"),
        )
        .expect("auth JSON");
        let user: Value = serde_json::from_str(
            &lines
                .next_line()
                .await
                .expect("user line")
                .expect("user frame"),
        )
        .expect("user JSON");
        (auth, user)
    });

    let delivered = (service.deliver_direct_message(target.clone(), push_id.clone()))
        .await
        .expect("resumed peer DM receipt");
    assert_eq!(delivered.push_id, push_id);
    assert_eq!(delivered.mode, Some(MessageDelivery::Auto));
    assert_eq!(delivered.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(
        delivered.last_outcome.unwrap().outcome,
        DeliveryOutcome::PeerMessageWritten
    );
    let (_auth, user) = (received).await.expect("peer receiver task");
    assert_eq!(user["type"], "user");
    assert!(
        user["message"]["content"]
            .as_str()
            .expect("rendered push line")
            .contains(push_id.as_str())
    );
    assert!(
        automation
            .list_unsent_direct_messages(&target)
            .await
            .expect("list remaining DMs")
            .is_empty()
    );
    assert_rejected_not_running(
        &automation
            .get_push_record(&closed_push_id)
            .await
            .expect("read the failed DM")
            .expect("failed DM stays recorded"),
    );
    service.shutdown().await;
}

#[tokio::test]
async fn closed_peer_rejects_queue_steer_and_guarded_dms() {
    let directory = tempfile::tempdir().expect("isolated test directory");
    let (_registry, automation, service, _router) =
        start_peer_delivery_service(directory.path(), Vec::new()).await;
    let target = target();
    let cases = [
        (
            MessageDelivery::Queue,
            None,
            DeliveryRejectionReason::EndpointUnavailable,
        ),
        (
            MessageDelivery::Steer,
            None,
            DeliveryRejectionReason::EndpointUnavailable,
        ),
        (
            MessageDelivery::Auto,
            Some(generation_guard()),
            DeliveryRejectionReason::EndpointUnavailable,
        ),
    ];

    for (mode, guard, expected_reason) in cases {
        let push_draft = draft(service.runtime_clock().as_ref(), &target, mode, guard);
        let push_id = push_draft.push_id.clone();
        automation
            .insert_push_record(push_draft)
            .await
            .expect("store the DM before delivery");
        let rejected = (service.deliver_direct_message(target.clone(), push_id))
            .await
            .expect("restricted peer DM receipt");
        assert_eq!(rejected.delivery_state, PushDeliveryState::Rejected);
        let DeliveryOutcome::Rejected(rejection) = rejected.last_outcome.unwrap().outcome else {
            panic!("mode {mode:?} must be rejected");
        };
        assert_eq!(rejection.reason, expected_reason, "mode={mode:?}");
    }
    assert!(
        automation
            .list_unsent_direct_messages(&target)
            .await
            .expect("rejected DMs are not queued")
            .is_empty()
    );
    service.shutdown().await;
}

#[tokio::test]
async fn queue_to_same_endpoint_label_on_another_service_keeps_queue_hold_behavior() {
    let directory = tempfile::tempdir().expect("isolated test directory");
    let (_registry, automation, service, router) =
        start_peer_delivery_service(directory.path(), Vec::new()).await;
    let mut target = target();
    target.endpoint.service_id =
        UuidIdentity::try_from("00000000-0000-4000-8000-000000000099".to_owned())
            .expect("foreign router service id");
    assert!(router.supports_delivery_mode(&target, MessageDelivery::Queue));
    let push_draft = draft(
        service.runtime_clock().as_ref(),
        &target,
        MessageDelivery::Queue,
        None,
    );
    let push_id = push_draft.push_id.clone();
    automation
        .insert_push_record(push_draft)
        .await
        .expect("store the DM before delivery");

    let held = (service.deliver_direct_message(target.clone(), push_id))
        .await
        .expect("foreign endpoint receipt");
    assert_eq!(held.mode, Some(MessageDelivery::Queue));
    assert_eq!(held.delivery_state, PushDeliveryState::Held);
    assert!(matches!(
        held.last_outcome.unwrap().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    service.shutdown().await;
}

/// Stands in for the ACP Claude provider: it can load this `claude-local` session.
#[derive(Default)]
struct LoadableProviderSession {
    deliveries: AtomicUsize,
}

impl SessionDeliveryRoute for LoadableProviderSession {
    fn reachability(&self) -> SessionReachability {
        SessionReachability::ProviderAcp
    }
    fn claim(&self, _target: &SessionRef) -> DeliveryFuture<'_, NativeRouteClaim> {
        Box::pin(async { Ok(NativeRouteClaim::can_load()) })
    }
    fn presence(&self, _target: &SessionRef) -> DeliveryFuture<'_, RoutePresence> {
        Box::pin(async { Ok(RoutePresence::Wakeable) })
    }
    fn deliver<'a>(
        &'a self,
        _request: collaboration_contracts::layer_zero::DeliveryRequest,
        _evidence: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, NativeDeliveryResult> {
        Box::pin(async move {
            self.deliveries.fetch_add(1, Ordering::SeqCst);
            Ok(NativeDeliveryResult::observed(
                NativeObservedOutcome::Started,
                Some(SessionReachability::ProviderAcp),
                None,
            ))
        })
    }
    fn reconcile_attempt(
        &self,
        _context: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}

#[tokio::test]
async fn a_session_the_provider_can_load_is_delivered_while_no_peer_runs() {
    let directory = tempfile::tempdir().expect("isolated test directory");
    let provider = Arc::new(LoadableProviderSession::default());
    let (_registry, automation, service, _router) =
        start_peer_delivery_service(directory.path(), vec![provider.clone()]).await;
    let target = target();
    let push_draft = draft(
        service.runtime_clock().as_ref(),
        &target,
        MessageDelivery::Auto,
        None,
    );
    let push_id = push_draft.push_id.clone();
    automation
        .insert_push_record(push_draft)
        .await
        .expect("store the DM before delivery");

    let delivered = (service.deliver_direct_message(target.clone(), push_id))
        .await
        .expect("provider DM receipt");

    assert_eq!(delivered.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(
        delivered.last_outcome.expect("settled outcome").outcome,
        DeliveryOutcome::Started
    );
    assert_eq!(provider.deliveries.load(Ordering::SeqCst), 1);
    service.shutdown().await;
}
