//! Queue reconciliation on the native delivery route: a real `deliver` whose queue reply is
//! lost, then `reconcile_attempt` against the native queue. Native replies and schema are
//! stand-ins; the route, its evidence and the broker's approval routes are real.
use super::*;
use agent_automation::{AttemptId, PreparationEffect};
use collaboration_contracts::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext, DeliveryFuture,
    SessionDeliveryRoute, layer_zero,
};
use collaboration_protocol::{
    DeliveryClientReceipt, DeliveryCorrelationId, DeliveryOutcome, MachineId, OperationId, PushId,
    RouterLink,
};

/// Keeps the route's latest attempt evidence in memory, as the mail path does.
#[derive(Default)]
struct CapturedEvidence(Mutex<Option<RouteEffectEvidence<SessionRef, CodexGeneration>>>);

impl AttemptEvidenceSink for CapturedEvidence {
    fn record(
        &self,
        evidence: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        *self.0.lock().unwrap() = Some(evidence);
        Box::pin(async { Ok(()) })
    }
}

struct QueueObservation {
    resume: PreparationEffect,
    reconciliation: AttemptReconciliation,
}

#[derive(Clone, Copy)]
enum QueueProbeCase {
    MatchingItem,
    WrongItem,
    DuplicateItem,
    GenericRoute,
    GenericRegistersDuringNativeRead,
}

/// Delivers one queued input whose `thread/queue/add` reply is lost, then reconciles the
/// attempt from its recorded evidence against the native queue the stand-in reports.
async fn queue_probe(case: QueueProbeCase) -> Result<QueueObservation, TestError> {
    let fixture = if matches!(
        case,
        QueueProbeCase::GenericRoute | QueueProbeCase::GenericRegistersDuringNativeRead
    ) {
        RecordedPolicyFixture::create_without_route().await?
    } else {
        RecordedPolicyFixture::create().await?
    };
    let directory =
        collaboration_runtime::EndpointDirectory::new(fixture.backend.endpoint.service_id.clone());
    directory.publish(fixture.endpoint.clone())?;
    let route = collaboration_runtime::CodexAppServerDeliveryRoute::new(
        fixture.clock.clone(),
        fixture.backend.endpoint.service_id.clone(),
        directory,
        fixture.backend.clone(),
        Arc::new(collaboration_runtime::UnmaterializedThreadHolder::new()),
    )
    .with_approval_broker(&fixture.broker);
    let push_id = PushId::try_from(OperationId::generate().as_str().to_owned())?;
    let link = RouterLink::new(
        MachineId::from(fixture.backend.endpoint.service_id.clone()),
        push_id.clone(),
    );
    let request = layer_zero::DeliveryRequest {
        payload: layer_zero::PreparedPush {
            push_id: push_id.clone(),
            line: format!("Policy queue witness · {link}").try_into()?,
            load_policy: LoadPolicy::MayLoad,
        },
        target: target(),
        mode: MessageDelivery::Queue,
        precondition: collaboration_contracts::DeliveryPrecondition::Unpinned,
        correlation: DeliveryCorrelationId::try_from(push_id.as_str().to_owned())?,
        attempt: AttemptId::generate(),
    };
    let listener = tokio::net::UnixListener::bind(fixture.directory.path().join("native.sock"))?;
    let mutation_broker = fixture.broker.clone();
    let mutation_scratch = fixture
        .directory
        .path()
        .join("scratch")
        .to_string_lossy()
        .into_owned();
    let cwd = fixture.directory.path().to_string_lossy().into_owned();
    let mut roots = fixture
        .filesystem
        .iter()
        .filter(|(_, access)| *access == &json!("write"))
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    if matches!(case, QueueProbeCase::MatchingItem) {
        roots.push("/literal-extra-queue-root".to_owned());
    }
    let native = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let mut queued = None;
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let request: Value = serde_json::from_str(&text)?;
            let method = request["method"].as_str().ok_or("method missing")?;
            let result = match method {
                "initialize" => json!({}),
                "initialized" => continue,
                "thread/read" => {
                    if matches!(case, QueueProbeCase::GenericRegistersDuringNativeRead) {
                        mutation_broker
                            .register_route(ApprovalRoute {
                                thread_id: THREAD.into(),
                                created_by: target(),
                                approver: target(),
                                access: collaboration_protocol::RouterAccess::WorkspaceWrite,
                                scratch_path: mutation_scratch.clone(),
                                root_message_id: None,
                            })
                            .await?;
                    }
                    json!({"thread":{"id":THREAD,"cwd":cwd,"status":{"type":"idle"}}})
                }
                "thread/resume" => {
                    json!({"thread":{"id":THREAD,"cwd":cwd},"cwd":cwd,"activePermissionProfile":{"id":"router-workspace-write"},"sandbox":{"type":"workspaceWrite","networkAccess":true,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true,"writableRoots":roots}})
                }
                // The reply to the queue mutation is lost: the connection closes first.
                "thread/queue/add" => {
                    queued = Some(request.clone());
                    break;
                }
                _ => return Err("unexpected native mutation".into()),
            };
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .await?;
        }
        drop(socket);
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let request: Value = serde_json::from_str(&text)?;
            let result = match request["method"].as_str() {
                Some("initialize") => json!({}),
                Some("initialized") => continue,
                Some("thread/queue/list") => {
                    let queued = queued.as_ref().ok_or("queued request missing")?;
                    let mut item = json!({"id":"reconciled-input","clientUserMessageId":queued.pointer("/params/clientUserMessageId"),"input":queued.pointer("/params/input")});
                    if matches!(case, QueueProbeCase::WrongItem) {
                        item["clientUserMessageId"] = json!("unrelated-push");
                    }
                    let items = if matches!(case, QueueProbeCase::DuplicateItem) {
                        vec![item.clone(), item]
                    } else {
                        vec![item]
                    };
                    json!({"data":items,"nextCursor":null})
                }
                _ => return Err("reconciliation replayed input".into()),
            };
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .await?;
        }
        Ok::<_, TestError>(())
    });
    let evidence = CapturedEvidence::default();
    let delivered = tokio::time::timeout(Duration::from_secs(5), route.deliver(request, &evidence))
        .await??
        .into_receipt();
    if !matches!(delivered.outcome, DeliveryOutcome::Unknown) {
        return Err(format!("a lost queue reply must be unknown: {delivered:?}").into());
    }
    let recorded = evidence
        .0
        .lock()
        .unwrap()
        .take()
        .ok_or("no attempt evidence")?;
    let RouteEffectEvidence::CodexAppServer(effects) = &recorded else {
        return Err("native attempt evidence expected".into());
    };
    if effects.client_user_message_id.as_deref() != Some(push_id.as_str()) {
        return Err("attempt correlation differs from the prepared push".into());
    }
    let resume = effects.resume;
    let reconciliation = tokio::time::timeout(
        Duration::from_secs(5),
        route.reconcile_attempt(AttemptReconciliationContext {
            target: target(),
            prepared_push_id: push_id,
            mode: MessageDelivery::Queue,
            recorded,
        }),
    )
    .await??;
    tokio::time::timeout(Duration::from_secs(5), native).await???;
    Ok(QueueObservation {
        resume,
        reconciliation,
    })
}

fn queued_receipt(
    observed: QueueObservation,
) -> Result<(PreparationEffect, collaboration_protocol::DeliveryReceipt), TestError> {
    let AttemptReconciliation::Accepted(receipt) = observed.reconciliation else {
        return Err("exact queue item was not reconciled".into());
    };
    Ok((observed.resume, *receipt))
}

#[tokio::test]
async fn policy_queue_accepted_resume_reconciliation_causal() {
    let (resume, receipt) =
        queued_receipt(queue_probe(QueueProbeCase::MatchingItem).await.unwrap()).unwrap();
    assert_eq!(resume, PreparationEffect::Accepted);
    assert!(
        matches!(receipt.outcome, DeliveryOutcome::Queued),
        "exact queue item must resolve accepted-resume submission"
    );
    let Some(DeliveryClientReceipt::CodexAppServer(client)) = receipt.client else {
        panic!("client receipt missing");
    };
    assert_eq!(
        client.resume_effect,
        collaboration_protocol::AcceptedResumeEffect::Accepted
    );
    assert_eq!(
        client.permission_differences,
        vec![collaboration_protocol::PermissionDifference::WritableRoots]
    );
}

#[tokio::test]
async fn policy_queue_wrong_or_duplicate_item_stays_uncertain() {
    for case in [QueueProbeCase::WrongItem, QueueProbeCase::DuplicateItem] {
        let observed = queue_probe(case).await.unwrap();
        assert_eq!(observed.resume, PreparationEffect::Accepted);
        assert!(matches!(
            observed.reconciliation,
            AttemptReconciliation::StillUnknown
        ));
    }
}

#[tokio::test]
async fn policy_queue_generic_route_reconciles_without_inventing_resume() {
    let (resume, receipt) =
        queued_receipt(queue_probe(QueueProbeCase::GenericRoute).await.unwrap()).unwrap();
    assert_eq!(resume, PreparationEffect::NotRequested);
    assert!(matches!(receipt.outcome, DeliveryOutcome::Queued));
    let Some(DeliveryClientReceipt::CodexAppServer(client)) = receipt.client else {
        panic!("client receipt missing");
    };
    assert_eq!(
        client.resume_effect,
        collaboration_protocol::AcceptedResumeEffect::NotRequested
    );
}

#[tokio::test]
async fn policy_generic_membership_is_pinned_before_native_io() {
    let (resume, receipt) = queued_receipt(
        queue_probe(QueueProbeCase::GenericRegistersDuringNativeRead)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        resume,
        PreparationEffect::NotRequested,
        "generic attempt reclassified and resumed after native read"
    );
    assert!(matches!(receipt.outcome, DeliveryOutcome::Queued));
    let Some(DeliveryClientReceipt::CodexAppServer(client)) = receipt.client else {
        panic!("client receipt missing");
    };
    assert_eq!(
        client.resume_effect,
        collaboration_protocol::AcceptedResumeEffect::NotRequested
    );
}

/// The recorded native effects a queued Codex attempt leaves before its outcome is known.
fn recorded_codex_effects(
    fixture: &RecordedPolicyFixture,
) -> RouteEffectEvidence<SessionRef, CodexGeneration> {
    serde_json::from_value(json!({"kind":"codexAppServer","target":target(),"generation":fixture.generation,"clientUserMessageId":"loaded-policy-input","nativeTurnId":null,"nativeSubmissionId":null,"allocation":"notRequested","resume":"notRequested","submission":"notDispatched","cessation":"notApplicable"})).unwrap()
}
#[tokio::test]
async fn policy_queue_invalid_effects_never_inspect_or_accept_a_queue_item() {
    let fixture = RecordedPolicyFixture::create().await.unwrap();
    let listener =
        tokio::net::UnixListener::bind(fixture.directory.path().join("native.sock")).unwrap();
    let push: collaboration_protocol::PushId = OperationId::generate()
        .as_str()
        .to_owned()
        .try_into()
        .unwrap();
    for (resume, allocation, wrong_correlation, wrong_target, missing_generation) in [
        (
            PreparationEffect::Unknown,
            PreparationEffect::NotRequested,
            false,
            false,
            false,
        ),
        (
            PreparationEffect::Rejected,
            PreparationEffect::NotRequested,
            false,
            false,
            false,
        ),
        (
            PreparationEffect::NotDispatched,
            PreparationEffect::NotRequested,
            false,
            false,
            false,
        ),
        (
            PreparationEffect::Accepted,
            PreparationEffect::Accepted,
            false,
            false,
            false,
        ),
        (
            PreparationEffect::Accepted,
            PreparationEffect::NotRequested,
            true,
            false,
            false,
        ),
        (
            PreparationEffect::Accepted,
            PreparationEffect::NotRequested,
            false,
            true,
            false,
        ),
        (
            PreparationEffect::Accepted,
            PreparationEffect::NotRequested,
            false,
            false,
            true,
        ),
    ] {
        let RouteEffectEvidence::CodexAppServer(mut effects) = recorded_codex_effects(&fixture)
        else {
            panic!("native fixture expected");
        };
        effects.resume = resume;
        effects.allocation = allocation;
        effects.client_user_message_id = Some(if wrong_correlation {
            "unrelated-push".into()
        } else {
            push.as_str().into()
        });
        if wrong_target {
            effects.target = Some(SessionRef {
                endpoint: target().endpoint,
                session_id: "unrelated-thread".to_owned().try_into().unwrap(),
            });
        }
        if missing_generation {
            effects.generation = None;
        }
        let context = collaboration_contracts::AttemptReconciliationContext {
            target: target(),
            prepared_push_id: push.clone(),
            mode: MessageDelivery::Queue,
            recorded: effects.into(),
        };
        let observed = tokio::select! {
            biased;
            _=listener.accept()=>panic!("invalid evidence reached queue inspection"),
            result=collaboration_runtime::test_support::codex_queue_reconciliation::reconcile(&fixture.backend,context)=>result,
        };
        assert!(matches!(
            observed,
            collaboration_contracts::AttemptReconciliation::StillUnknown
        ));
    }
}
