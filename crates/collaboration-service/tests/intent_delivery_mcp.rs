//! Real board admission, renderer, prepared push and Layer-0 routing; only the agent endpoint stands in.
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
    fn claim(&self, _: &collaboration_protocol::SessionRef) -> DeliveryFuture<'_, RouteClaim> {
        Box::pin(async { Ok(RouteClaim::Holds) })
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
    ) -> DeliveryFuture<'a, DeliveryReceipt> {
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
            Ok(DeliveryReceipt {
                outcome,
                reachability: Some(SessionReachability::ProviderAcp),
                client: None,
            })
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
    let registry = Arc::new(ProjectStoreRegistry::new(directory.path()));
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: registry.clone(),
        fleet: Arc::new(FleetClient::open_local_stand_in(directory.path()).await?),
        leases: Arc::new(LocalLeaseAuthority::default()),
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
            admission.subscription_window_store(automation.clone()),
        ))),
        push_store: automation.clone(),
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
    delivery.start().await?;
    let api = ServedCollaborationApi::start(
        directory.path(),
        CollaborationApplication::new(
            ServiceIdentity::new(
                "00000000-0000-4000-8000-000000000001",
                "00000000-0000-4000-8000-000000000002",
            )?
            .with_project_admission(admission.clone())
            .with_automation_store(automation.clone())
            .with_subscription_delivery_service(delivery.clone(), routes),
        ),
    )
    .await?;
    let actor = json!({"kind":"human","humanId":"owner"});
    let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"resubmits":null,"actingFor":null,"actor":actor});
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
    accepted(&api.call("project_create", create).await?);
    let mut grant = header();
    grant["projectId"] = json!(project);
    grant["observedEpoch"] = json!(1);
    grant["operation"] = json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null});
    accepted(&api.call("role_grant", grant).await?);
    let root = PostId::generate();
    let target = json!({"kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"intent-endpoint-stand-in"}});
    let mut create = header();
    create["projectId"] = json!(project);
    create["observedEpoch"] = json!(1);
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
    let push = automation
        .lock()
        .await
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
    assert!(matches!(notice.state(), IntentState::HandedOff {}));
    read.finish().await?;
    let replay = api.call("thread_create", create).await?;
    assert_eq!(
        replay.pointer("/result/result"),
        original.pointer("/result/result")
    );
    // A later session delivery is a protocol barrier: native intent scans are ordered by UUIDv7 ID.
    let human_post = PostId::generate();
    let mut human_request = header();
    human_request["observedEpoch"] = json!(1);
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
    barrier["observedEpoch"] = json!(1);
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
    let barrier_push = automation
        .lock()
        .await
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
    let RecordPosition::Project(human_cursor) = human_notice.envelope.position else {
        return Err("human intent position".into());
    };
    let before_claim = read.high_water();
    read.finish().await?;
    let IntentReply::Deferred {
        recipient,
        replacements,
    } = admission.claim_intent(&project, &human_intent).await?
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
        automation
            .lock()
            .await
            .intent_consumed(&project, &human_intent, human_cursor)
            .await?
    );
    outcomes.lock().await.push_back(DeliveryOutcome::Rejected(
        collaboration_protocol::DeliveryRejection {
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
                        let consumed = automation
                            .lock()
                            .await
                            .intent_consumed(
                                &project,
                                &intent.intent_id,
                                match intent.envelope.position {
                                    RecordPosition::Project(cursor) => cursor,
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
    let push = automation
        .lock()
        .await
        .get_push_record(&completed_barrier.payload.push_id)
        .await?
        .ok_or("barrier push")?;
    let collaboration_protocol::PushHeaderFacts::BoardNotification { cause, .. } =
        push.header_facts
    else {
        return Err("refusal barrier kind".into());
    };
    assert_eq!(cause.id.as_str(), refusal_barrier.as_str());
    let push = automation
        .lock()
        .await
        .get_push_record(&rejected_delivery.payload.push_id)
        .await?
        .ok_or("refused push")?;
    assert_eq!(
        push.delivery_state,
        collaboration_protocol::PushDeliveryState::Rejected
    );
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
            IntentState::Refused {
                reason: IntentRefusal::Delivery { .. }
            }
        ),
        "a definitive session push-port refusal must be recorded: {refused:?}"
    );
    read.finish().await?;
    delivery.shutdown().await;
    let mut reopened =
        automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await?;
    assert!(
        reopened
            .intent_consumed(&project, &human_intent, human_cursor)
            .await?,
        "C5 progress survives restart"
    );
    reopened.close().await?;
    assert!(matches!(
        received.try_recv(),
        Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected)
    ));
    api.stop().await?;
    Ok(())
}
fn accepted(value: &Value) {
    assert_eq!(
        value.pointer("/result/result/decision/kind"),
        Some(&json!("accepted")),
        "{value}"
    )
}
