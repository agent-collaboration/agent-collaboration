//! The real reader actors serialize each target; the external client is held at a channel seam.
use super::super::subscription_service::OwnerObservation;
use crate::BoardAvailability;
use crate::SubscriptionDeliveryService;
use crate::SubscriptionDeliveryServiceProps;
use crate::SystemSubscriptionClock;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_contracts::layer_zero::DeliveryRequest;
use collaboration_contracts::layer_zero::PreparedPush;
use collaboration_protocol::{
    DeliveryCorrelationId, DeliveryNextAction, DeliveryOutcome, DeliveryReceipt, DeliveryRejection,
    DeliveryRejectionReason, MessageDelivery, PushId, PushKind, SessionRef,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot};

struct ClientCall {
    request: DeliveryRequest,
    reply: oneshot::Sender<DeliveryReceipt>,
}
struct HeldClient {
    calls: mpsc::UnboundedSender<ClientCall>,
    supports_queue: bool,
}
impl SessionMessageDelivery for HeldClient {
    fn supports_delivery_mode(&self, _: &SessionRef, mode: MessageDelivery) -> bool {
        mode != MessageDelivery::Queue || self.supports_queue
    }
    fn deliver<'a>(
        &'a self,
        request: DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        Box::pin(async move {
            let (reply, response) = oneshot::channel();
            self.calls
                .send(ClientCall { request, reply })
                .map_err(|_| DeliveryContractError::ClientOperation)?;
            response
                .await
                .map(collaboration_contracts::scripted_delivery_result)
                .map_err(|_| DeliveryContractError::ClientOperation)
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}
struct RunningPresence;
impl TargetPresenceProbe for RunningPresence {
    fn presence(&self, _: &SessionRef) -> DeliveryFuture<'_, TargetPresence> {
        Box::pin(async { Ok(TargetPresence::Running) })
    }
}

#[tokio::test]
async fn actor_validation_classifies_invalid_input_before_queue_acceptance_or_client_io() {
    let runtime = Runtime::new().await;
    let result = runtime
        .service
        .attempt_mail_once(PushKind::Wake, request("reader-a", MessageDelivery::Auto))
        .await;
    assert_eq!(
        result.unsent_cause(),
        Some(collaboration_contracts::UnsentCause::InvalidInput)
    );
    assert!(matches!(
        result.receipt().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: false,
            ..
        }
    ));
    assert!(runtime.calls.is_empty());
    runtime.close().await;
}

struct Runtime {
    service: SubscriptionDeliveryService,
    store: Arc<interaction_storage::InteractionStore>,
    calls: mpsc::UnboundedReceiver<ClientCall>,
    observations: broadcast::Receiver<OwnerObservation>,
    _directory: tempfile::TempDir,
}
impl Runtime {
    async fn new() -> Self {
        Self::with_clock(Arc::new(SystemSubscriptionClock)).await
    }
    async fn with_clock(clock: Arc<dyn crate::SubscriptionClock>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            interaction_storage::InteractionStore::open(
                directory.path().join("interaction.sqlite"),
            )
            .await
            .unwrap(),
        );
        let (calls, receiver) = mpsc::unbounded_channel();
        let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Unavailable,
            push_store: store.clone(),
            delivery: Arc::new(HeldClient {
                calls,
                supports_queue: false,
            }),
            presence: Arc::new(RunningPresence),
            machine_identity: MachineIdentity::new(
                "00000000-0000-4000-8000-000000000001"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                Some("Native attempt test"),
            )
            .unwrap(),
            clock,
        });
        let observations = service.observe_owners();
        service.start().await.unwrap();
        Self {
            service,
            store,
            calls: receiver,
            observations,
            _directory: directory,
        }
    }
    async fn next_call(&mut self) -> ClientCall {
        tokio::time::timeout(Duration::from_secs(5), self.calls.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn queued(&mut self, push: &PushId) {
        tokio::time::timeout(Duration::from_secs(5),async {
            loop {
                if matches!(self.observations.recv().await.unwrap(),OwnerObservation::MailAttemptQueued(id) if &id==push) {return;}
            }
        }).await.unwrap();
    }
    async fn close(self) {
        self.service.shutdown().await;
        drop(self.service);
        Arc::try_unwrap(self.store)
            .unwrap_or_else(|_| panic!("attempt store retained"))
            .close()
            .await
            .unwrap();
    }
}
fn request(session: &str, mode: MessageDelivery) -> DeliveryRequest {
    let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap();
    DeliveryRequest {
        correlation:DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).unwrap(),
        payload:PreparedPush {push_id,line:"frozen native mail".to_owned().try_into().unwrap(),load_policy:LoadPolicy::LoadedOnly},
        target:serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":session})).unwrap(),
        mode,precondition:DeliveryPrecondition::Unpinned,attempt:agent_automation::AttemptId::generate(),
    }
}
fn started() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::Started,
        reachability: Some(collaboration_protocol::SessionReachability::CodexAppServer),
        client: None,
    }
}
fn no_route() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::Rejected(DeliveryRejection {
            native_failure: None,
            reason: DeliveryRejectionReason::NoRoute,
            next_action: DeliveryNextAction::InspectTarget,
            client_code: None,
            detail: Some("no route owns this session".to_owned()),
            claims: None,
        }),
        reachability: None,
        client: None,
    }
}

#[tokio::test]
async fn accepted_attempts_are_serial_per_reader_and_another_reader_progresses_while_one_is_blocked()
 {
    let mut runtime = Runtime::new().await;
    let first = request("reader-a", MessageDelivery::Auto);
    let first_id = first.payload.push_id.clone();
    let service = runtime.service.clone();
    let first = tokio::spawn(async move {
        service
            .attempt_mail_once(PushKind::DirectMessage, first)
            .await
            .into_receipt()
    });
    let first_call = runtime.next_call().await;
    assert_eq!(first_call.request.payload.push_id, first_id);
    let second = request("reader-a", MessageDelivery::Auto);
    let second_id = second.payload.push_id.clone();
    let service = runtime.service.clone();
    let second = tokio::spawn(async move {
        service
            .attempt_mail_once(PushKind::DirectMessage, second)
            .await
            .into_receipt()
    });
    runtime.queued(&second_id).await;
    let other = request("reader-b", MessageDelivery::Auto);
    let other_id = other.payload.push_id.clone();
    let service = runtime.service.clone();
    let other = tokio::spawn(async move {
        service
            .attempt_mail_once(PushKind::DirectMessage, other)
            .await
            .into_receipt()
    });
    let other_call = runtime.next_call().await;
    assert_eq!(
        other_call.request.payload.push_id, other_id,
        "same-reader attempt escaped its actor"
    );
    assert!(runtime.calls.try_recv().is_err());
    // Real storage remains readable while the client effect is held open.
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            runtime.store.get_push_record(&first_id)
        )
        .await
        .unwrap()
        .unwrap()
        .is_none(),
        "carrier wrote a push outside its caller's transaction"
    );
    other_call.reply.send(started()).unwrap();
    assert_eq!(other.await.unwrap().outcome, DeliveryOutcome::Started);
    first_call.reply.send(started()).unwrap();
    assert_eq!(first.await.unwrap().outcome, DeliveryOutcome::Started);
    let second_call = runtime.next_call().await;
    assert_eq!(second_call.request.payload.push_id, second_id);
    second_call.reply.send(started()).unwrap();
    assert_eq!(second.await.unwrap().outcome, DeliveryOutcome::Started);
    assert!(runtime.calls.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test]
async fn lost_client_response_is_unknown_once_and_shutdown_before_acceptance_is_known_unsent() {
    let mut runtime = Runtime::new().await;
    let service = runtime.service.clone();
    let attempt = tokio::spawn(async move {
        service
            .attempt_mail_once(
                PushKind::DirectMessage,
                request("reader-a", MessageDelivery::Auto),
            )
            .await
            .into_receipt()
    });
    let call = runtime.next_call().await;
    drop(call.reply);
    assert_eq!(attempt.await.unwrap().outcome, DeliveryOutcome::Unknown);
    assert!(
        runtime.calls.try_recv().is_err(),
        "uncertain attempt was replayed"
    );
    runtime.service.shutdown().await;
    let not_started = runtime
        .service
        .attempt_mail_once(
            PushKind::DirectMessage,
            request("reader-a", MessageDelivery::Auto),
        )
        .await
        .into_receipt();
    assert!(matches!(
        not_started.outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    assert!(runtime.calls.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test]
async fn no_route_normalization_keeps_queue_auto_and_steer_semantics() {
    let mut runtime = Runtime::new().await;
    for mode in [
        MessageDelivery::Queue,
        MessageDelivery::Auto,
        MessageDelivery::Steer,
    ] {
        let service = runtime.service.clone();
        let attempt = tokio::spawn(async move {
            service
                .attempt_mail_once(PushKind::DirectMessage, request("reader-a", mode))
                .await
                .into_receipt()
        });
        let call = runtime.next_call().await;
        call.reply.send(no_route()).unwrap();
        let receipt = attempt.await.unwrap();
        match mode {
            MessageDelivery::Queue => assert!(
                matches!(receipt.outcome,DeliveryOutcome::Rejected(rejection) if rejection.reason==DeliveryRejectionReason::QueueUnsupported)
            ),
            MessageDelivery::Auto => assert!(matches!(
                receipt.outcome,
                DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    ..
                }
            )),
            MessageDelivery::Steer => assert_eq!(
                serde_json::to_value(receipt).unwrap(),
                serde_json::to_value(no_route()).unwrap()
            ),
        }
    }
    runtime.close().await;
}

#[tokio::test]
async fn notification_cooldown_holds_a_second_item_without_blocking_a_direct_message() {
    let mut runtime = Runtime::new().await;
    let make_notification = || {
        let mut request = request("reader-a", MessageDelivery::Auto);
        request.payload.line = "board notification".to_owned().try_into().unwrap();
        request
    };
    let first = make_notification();
    let service = runtime.service.clone();
    let first = tokio::spawn(async move {
        service
            .attempt_mail_once(PushKind::BoardNotification, first)
            .await
    });
    runtime.next_call().await.reply.send(started()).unwrap();
    assert_eq!(
        first.await.unwrap().receipt().outcome,
        DeliveryOutcome::Started
    );

    // Legacy expiry uses stored carrier recovery, exercised by the cadence/reopen proof.
    for kind in [PushKind::BoardNotification, PushKind::SubscriptionActivity] {
        let second = make_notification();
        let service = runtime.service.clone();
        let mut second = tokio::spawn(async move { service.attempt_mail_once(kind, second).await });
        let held = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                result = &mut second => result.unwrap(),
                call = runtime.calls.recv() => {
                    call.unwrap().reply.send(started()).unwrap();
                    let result = second.await.unwrap();
                    panic!("{kind:?} escaped cooldown: {:?}", result.receipt().outcome);
                }
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            held.receipt().outcome,
            DeliveryOutcome::NotSubmitted {
                retryable: true,
                ..
            }
        ));
    }
    let service = runtime.service.clone();
    let dm = tokio::spawn(async move {
        service
            .attempt_mail_once(
                PushKind::DirectMessage,
                request("reader-a", MessageDelivery::Auto),
            )
            .await
    });
    runtime.next_call().await.reply.send(started()).unwrap();
    assert_eq!(
        dm.await.unwrap().receipt().outcome,
        DeliveryOutcome::Started
    );
    runtime.close().await;
}

#[tokio::test]
#[cfg(debug_assertions)]
async fn debug_clock_control_waits_for_the_due_reader_scan_after_native_io_finishes() {
    use std::{
        future::{Future, poll_fn},
        task::Poll,
    };
    let start = chrono::DateTime::from_timestamp_millis(1_000_000).unwrap();
    let clock = Arc::new(crate::DebugSubscriptionClock::new(start));
    let mut runtime = Runtime::with_clock(clock.clone()).await;
    let service = runtime.service.clone();
    let native = tokio::spawn(async move {
        service
            .attempt_mail_once(
                PushKind::DirectMessage,
                request("reader-a", MessageDelivery::Auto),
            )
            .await
    });
    let call = runtime.next_call().await;
    let advanced = start + chrono::Duration::seconds(600);
    let mut control = Box::pin(runtime.service.debug_advance_notification_clock(advanced));
    assert!(
        poll_fn(|context| Poll::Ready(control.as_mut().poll(context)))
            .await
            .is_pending()
    );
    assert_eq!(
        crate::SubscriptionClock::now(&*clock),
        advanced,
        "control reached the clock step before the barrier"
    );
    call.reply.send(started()).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), control)
            .await
            .unwrap()
            .unwrap(),
        1
    );
    assert_eq!(
        native.await.unwrap().receipt().outcome,
        DeliveryOutcome::Started
    );
    runtime.close().await;
}

#[path = "notification_cadence_proofs.rs"]
#[cfg(debug_assertions)]
mod notification_cadence_proofs;

#[tokio::test]
#[cfg(debug_assertions)]
async fn debug_clock_control_refuses_a_failed_reader_scan_instead_of_reporting_completion() {
    use sqlx::Connection;
    let clock = Arc::new(crate::DebugSubscriptionClock::new(
        chrono::DateTime::from_timestamp_millis(0).unwrap(),
    ));
    let mut runtime = Runtime::with_clock(clock).await;
    let service = runtime.service.clone();
    let first = tokio::spawn(async move {
        service
            .attempt_mail_once(
                PushKind::BoardNotification,
                request("reader-a", MessageDelivery::Auto),
            )
            .await
    });
    runtime.next_call().await.reply.send(started()).unwrap();
    assert_eq!(
        first.await.unwrap().receipt().outcome,
        DeliveryOutcome::Started
    );
    let held = runtime
        .service
        .attempt_mail_once(
            PushKind::BoardNotification,
            request("reader-a", MessageDelivery::Auto),
        )
        .await;
    assert!(matches!(
        held.receipt().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(runtime._directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE notification_windows SET flush_due_at_ms=42")
        .execute(&mut observer)
        .await
        .unwrap();
    observer.close().await.unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.service.debug_advance_notification_clock(
            chrono::DateTime::from_timestamp_millis(600_000).unwrap(),
        ),
    )
    .await
    .unwrap();
    assert!(
        result.is_err(),
        "a refused stored window cannot be a completed scan"
    );
    assert!(runtime.calls.try_recv().is_err());
    let service = runtime.service.clone();
    let healthy = tokio::spawn(async move {
        service
            .attempt_mail_once(
                PushKind::BoardNotification,
                request("reader-b", MessageDelivery::Auto),
            )
            .await
    });
    let call = runtime.next_call().await;
    assert_eq!(
        call.request.target,
        request("reader-b", MessageDelivery::Auto).target
    );
    call.reply.send(started()).unwrap();
    assert_eq!(
        healthy.await.unwrap().receipt().outcome,
        DeliveryOutcome::Started,
        "one damaged recipient must not stop a healthy reader"
    );
    runtime.close().await;
}
