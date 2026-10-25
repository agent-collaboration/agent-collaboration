//! Real board admission, renderer, prepared push and Layer-0 routing; only the agent endpoint stands in.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/native_delivery.rs"]
mod native_delivery;
use board_fixture::native_mail_owner;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushKind, RouterOriginRef, SessionReachability,
};
use collaboration_service::*;
use message_board::*;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use tokio::sync::{Mutex, mpsc};
type TestError = Box<dyn std::error::Error + Send + Sync>;
struct CapturingEndpoint {
    requests: mpsc::Sender<layer_zero::DeliveryRequest>,
    outcomes: Arc<Mutex<std::collections::VecDeque<DeliveryOutcome>>>,
}
impl SessionDeliveryRoute for CapturingEndpoint {
    fn reachability(&self) -> SessionReachability {
        SessionReachability::ProviderAcp
    }
    fn claim(
        &self,
        _: &collaboration_protocol::SessionRef,
    ) -> DeliveryFuture<'_, collaboration_service::NativeRouteClaim> {
        Box::pin(async { Ok(collaboration_service::NativeRouteClaim::holds()) })
    }
    fn presence(
        &self,
        _: &collaboration_protocol::SessionRef,
    ) -> DeliveryFuture<'_, RoutePresence> {
        Box::pin(async { Ok(RoutePresence::Running) })
    }
    fn deliver<'a>(
        &'a self,
        request: layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_service::NativeDeliveryResult> {
        Box::pin(async move {
            let outcome = self
                .outcomes
                .lock()
                .await
                .pop_front()
                .unwrap_or(DeliveryOutcome::Started);
            self.requests
                .send(request)
                .await
                .map_err(|_| DeliveryContractError::ClientOperation)?;
            Ok(native_delivery::scripted(DeliveryReceipt {
                outcome,
                reachability: Some(SessionReachability::ProviderAcp),
                client: None,
            }))
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn explicit_intent_is_claimed_and_sent_as_a_stored_board_notification()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let registry = board_fixture::new_registry(directory.path())?;
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("intent proof".to_owned())?,
    };
    let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
        directory.path(),
        Arc::new(board_fixture::PendingRegistration),
        &origin,
    )
    .await?;
    let fleet = Arc::new(fleet);
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: registry.clone(),
        fleet: fleet.clone(),
        identity: MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(
                "00000000-0000-4000-8000-000000000001".to_owned(),
            )?,
            Some("intent proof"),
        )?,
    })?);
    let automation = Arc::new(Mutex::new(
        automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await?,
    ));
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await?,
    );
    let (sent, mut received) = mpsc::channel(8);
    let outcomes = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    let routes = Arc::new(SessionDeliveryRouter::new(vec![Arc::new(
        CapturingEndpoint {
            requests: sent,
            outcomes: outcomes.clone(),
        },
    )]));
    let delivery = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Available(Arc::new(Mutex::new(
            admission.subscription_window_store(interactions.clone()),
        ))),
        push_store: interactions.clone(),
        delivery: routes.clone(),
        presence: routes.clone(),
        machine_identity: MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(
                "00000000-0000-4000-8000-000000000001".to_owned(),
            )?,
            Some("intent proof"),
        )?,
        clock: Arc::new(SystemSubscriptionClock),
    });
    let identity = ServiceIdentity::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission.clone())
    .with_automation_store(automation.clone())
    .with_subscription_delivery_service(delivery.clone(), routes.clone());
    let (identity, native) = native_mail_owner::NativeMailOwner::start(
        identity,
        board_fixture::fixture_peer()?,
        MailPushDeliveryProps {
            interactions: interactions.clone(),
            actors: delivery.clone(),
            presence: routes,
            machine: MachineIdentity::new(
                "00000000-0000-4000-8000-000000000001"
                    .to_owned()
                    .try_into()?,
                Some("intent proof"),
            )?,
            clock: Arc::new(SystemSubscriptionClock),
        },
    )?;
    let publisher = Arc::new(NotificationPublisher::new(
        admission.clone(),
        interactions.clone(),
        Arc::downgrade(&native.runtime_handle()),
    ));
    let identity = identity.with_notification_publisher(publisher.clone());
    delivery.start().await?;
    let api =
        ServedCollaborationApi::start(directory.path(), CollaborationApplication::new(identity))
            .await?;
    let actor = json!({"kind":"human","humanId":"owner"});
    let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"actingFor":null,"actor":actor});
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let mut create = header();
    create["workspaceId"] = json!(workspace);
    create["name"] = json!("intents");
    create["description"] = json!("");
    accepted(&api.call("workspace_create", create).await?);
    let mut create = header();
    create["workspaceId"] = json!(workspace);
    create["boardId"] = json!(board);
    create["name"] = json!("board");
    create["description"] = json!("");
    create["kind"] = json!({"kind":"area"});
    accepted(&api.call("board_create", create).await?);
    let mut create = header();
    create["boardId"] = json!(board);
    create["projectId"] = json!(project);
    create["title"] = json!("project");
    board_fixture::prepare_registered_creation(
        &fleet,
        &registry,
        &serde_json::from_value(create.clone())?,
        &origin,
        &runtime_primitives::SystemClock,
    )
    .await?;
    accepted(&api.call("project_create", create).await?);
    let mut grant = header();
    grant["projectId"] = json!(project);
    grant["operation"] = json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null});
    accepted(&api.call("role_grant", grant).await?);
    let root = PostId::generate();
    let target = json!({"kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"intent-endpoint-stand-in"}});
    let mut create = header();
    create["projectId"] = json!(project);
    create["postId"] = json!(root);
    create["about"] = json!({"kind":"project","subject":{"line":"Addressed work"}});
    create["text"] = json!("Addressed work");
    create["references"] = json!([]);
    create["role"] = Value::Null;
    create["watch"] = json!(false);
    create["addressees"] = json!([{"kind":"actor","recipient":target}]);
    let original = api.call("thread_create", create.clone()).await?;
    accepted(&original);
    let request = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await?
        .ok_or("intent never reached Layer-0 endpoint")?;
    assert_eq!(serde_json::to_value(&request.target)?, target["session"]);
    assert_eq!(request.payload.load_policy, LoadPolicy::LoadedOnly);
    assert!(
        request
            .payload
            .line
            .as_str()
            .to_lowercase()
            .contains("board"),
        "{}",
        request.payload.line.as_str()
    );
    let push = interactions
        .get_push_record(&request.payload.push_id)
        .await?
        .ok_or("prepared push missing")?;
    assert_eq!(push.kind, PushKind::BoardNotification);
    assert!(push.body.is_none() && push.activity.is_none());
    let origin =
        RouterOriginRef::parse_canonical(push.origin_router_ref.as_deref().ok_or("origin")?)?;
    let RouterOriginRef::BoardIntent {
        project: push_project,
        intent,
    } = origin
    else {
        return Err("wrong board origin".into());
    };
    assert_eq!(push_project, project);
    let store = registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let Some(Record::NotifyIntent(notice)) = read.record(&intent.clone().into()).await? else {
        return Err("intent missing".into());
    };
    assert!(matches!(notice.state(), IntentState::HandedOff { .. }));
    read.finish().await?;
    let replay = api.call("thread_create", create).await?;
    assert_eq!(
        replay.pointer("/result/result"),
        original.pointer("/result/result")
    );
    // A later session delivery is a protocol barrier: native intent scans are ordered by UUIDv7 ID.
    let human_post = PostId::generate();
    let mut human_request = header();
    human_request["placement"] =
        json!({"kind":"thread","thread":{"thread":ThreadId::from(root.clone())}});
    human_request["postId"] = json!(human_post);
    human_request["text"] = json!("Human addressee");
    human_request["references"] = json!([]);
    human_request["addressees"] =
        json!([{"kind":"actor","recipient":{"kind":"human","humanId":"owner-attention"}}]);
    accepted(&api.call("thread_post", human_request).await?);
    let barrier_post = PostId::generate();
    let mut barrier = header();
    barrier["placement"] =
        json!({"kind":"thread","thread":{"thread":ThreadId::from(root.clone())}});
    barrier["postId"] = json!(barrier_post);
    barrier["text"] = json!("Session delivery barrier");
    barrier["references"] = json!([]);
    barrier["addressees"] = json!([{"kind":"actor","recipient":target}]);
    accepted(&api.call("thread_post", barrier.clone()).await?);
    let barrier_delivery = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await?
        .ok_or("barrier delivery missing")?;
    let barrier_push = interactions
        .get_push_record(&barrier_delivery.payload.push_id)
        .await?
        .ok_or("barrier push missing")?;
    let collaboration_protocol::PushHeaderFacts::BoardNotification { cause, .. } =
        &barrier_push.header_facts
    else {
        return Err("barrier header".into());
    };
    assert_eq!(cause.id.as_str(), barrier_post.as_str());
    let mut read = store.snapshot().await?;
    let notices = read.records(RecordKind::NotifyIntent).await?;
    let human_notice = notices
        .iter()
        .find_map(|record| match record {
            Record::NotifyIntent(notice) if notice.cause.id.as_str() == human_post.as_str() => {
                Some(notice)
            }
            _ => None,
        })
        .ok_or("human intent missing")?;
    assert!(
        matches!(human_notice.state(), IntentState::Emitted {}),
        "C5 must defer a human without changing replicated intent state: {human_notice:?}"
    );
    let human_intent = human_notice.intent_id.clone();
    let RecordPlacement::Project(human_entry) = &human_notice.envelope.position else {
        return Err("human intent position".into());
    };
    let human_cursor = human_entry.cursor;
    let before_claim = read.high_water();
    read.finish().await?;
    let IntentReply::Deferred {
        recipient,
        replacements,
    } = admission
        .claim_intent(&project, &human_intent, None)
        .await?
    else {
        return Err("human claim did not return Deferred".into());
    };
    assert!(replacements.is_empty());
    assert_eq!(
        serde_json::to_value(recipient.actor.declared_identity())?,
        json!({"kind":"human","humanId":"owner-attention"})
    );
    let read = store.snapshot().await?;
    assert_eq!(
        read.high_water(),
        before_claim,
        "Deferred writes neither intent nor request ledger"
    );
    read.finish().await?;
    assert!(
        interactions
            .intent_consumed(&project, &human_intent, human_cursor)
            .await?
    );
    outcomes.lock().await.push_back(DeliveryOutcome::Rejected(
        collaboration_protocol::DeliveryRejection {
            native_failure: None,
            reason: collaboration_protocol::DeliveryRejectionReason::PermissionDenied,
            next_action: collaboration_protocol::DeliveryNextAction::CorrectRequest,
            client_code: None,
            detail: Some("endpoint refused the notification".to_owned()),
            claims: None,
        },
    ));
    let refused_post = PostId::generate();
    let mut refusal = barrier.clone();
    refusal["requestId"] = json!(RequestId::generate());
    refusal["postId"] = json!(refused_post);
    accepted(&api.call("thread_post", refusal).await?);
    let refusal_barrier = PostId::generate();
    barrier["requestId"] = json!(RequestId::generate());
    barrier["postId"] = json!(refusal_barrier);
    accepted(&api.call("thread_post", barrier).await?);
    let rejected_delivery =
        match tokio::time::timeout(std::time::Duration::from_secs(5), received.recv()).await {
            Ok(Some(request)) => request,
            _ => {
                let mut read = store.snapshot().await?;
                for record in read.records(RecordKind::NotifyIntent).await? {
                    if let Record::NotifyIntent(intent) = record {
                        let consumed = interactions
                            .intent_consumed(
                                &project,
                                &intent.intent_id,
                                match &intent.envelope.position {
                                    RecordPlacement::Project(entry) => entry.cursor,
                                    _ => return Err("intent position".into()),
                                },
                            )
                            .await?;
                        eprintln!(
                            "intent diagnostic: cause={} state={:?} consumed={consumed}",
                            intent.cause.id.as_str(),
                            intent.state()
                        );
                    }
                }
                read.finish().await?;
                return Err("refused session delivery timed out".into());
            }
        };
    let completed_barrier =
        tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
            .await
            .map_err(|_| "refusal barrier delivery timed out")?
            .ok_or("refusal barrier missing")?;
    let push = interactions
        .get_push_record(&completed_barrier.payload.push_id)
        .await?
        .ok_or("barrier push")?;
    let collaboration_protocol::PushHeaderFacts::BoardNotification { cause, .. } =
        push.header_facts
    else {
        return Err("refusal barrier kind".into());
    };
    assert_eq!(cause.id.as_str(), refusal_barrier.as_str());
    let push = interactions
        .get_push_record(&rejected_delivery.payload.push_id)
        .await?
        .ok_or("refused push")?;
    assert_eq!(
        push.delivery_state,
        collaboration_protocol::PushDeliveryState::Rejected
    );
    assert!(matches!(
        &push.last_outcome.as_ref().ok_or("terminal native outcome missing")?.outcome,
        DeliveryOutcome::Rejected(rejection)
            if rejection.reason == collaboration_protocol::DeliveryRejectionReason::PermissionDenied
                && rejection.detail.as_deref() == Some("endpoint refused the notification")
    ));
    let mut read = store.snapshot().await?;
    let notices = read.records(RecordKind::NotifyIntent).await?;
    let refused = notices
        .iter()
        .find_map(|record| match record {
            Record::NotifyIntent(intent) if intent.cause.id.as_str() == refused_post.as_str() => {
                Some(intent)
            }
            _ => None,
        })
        .ok_or("refused intent")?;
    assert!(
        matches!(
            refused.state(),
            IntentState::HandedOff { item } if item.as_str() == rejected_delivery.payload.push_id.as_str()
        ),
        "a terminal delivery keeps the intent's frozen handoff identity: {refused:?}"
    );
    read.finish().await?;
    api.stop().await?;
    delivery.shutdown().await;
    publisher.shutdown().await;
    drop(delivery);
    drop(publisher);
    drop(native.stop().await?);
    Arc::try_unwrap(interactions)
        .map_err(|_| "intent fixture retained its native message owner")?
        .close()
        .await?;
    let reopened =
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await?;
    assert!(
        reopened
            .intent_consumed(&project, &human_intent, human_cursor)
            .await?,
        "C5 progress survives restart"
    );
    let restored_push = reopened
        .get_push_record(&push.push_id)
        .await?
        .ok_or("refused push missing after reopen")?;
    assert_eq!(
        serde_json::to_value(restored_push)?,
        serde_json::to_value(push)?
    );
    reopened.close().await?;
    assert!(matches!(
        received.try_recv(),
        Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected)
    ));
    fleet_runtime.shutdown().await?;
    Ok(())
}
fn accepted(value: &Value) {
    assert_eq!(
        value.pointer("/result/result/decision/kind"),
        Some(&json!("accepted")),
        "{value}"
    )
}
