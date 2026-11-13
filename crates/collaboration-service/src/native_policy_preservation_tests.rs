//! Native delivery preserves capability fallback and recorded authority.
use super::*;

#[derive(Clone, Copy)]
enum QueueFallbackReply {
    Accepted,
    Idle,
    NotLoaded,
    MissingTurn,
    Rejected,
    Lost,
}

// Upstream schemas and replies are stand-ins; dispatch, validation and sockets stay real.
async fn queue_capability_probe(
    queue_supported: bool,
    reply: QueueFallbackReply,
) -> Result<NativeMessageOutcome, TestError> {
    let directory = tempfile::tempdir()?;
    let (backend, generation, endpoint) =
        backend_with_queue_support(directory.path(), queue_supported);
    let listener = tokio::net::UnixListener::bind(directory.path().join("native.sock"))?;
    let (finished_sender, finished_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let connection = tokio::select! {
            accepted = listener.accept() => Some(accepted?),
            _ = finished_receiver => None,
        };
        let Some((stream, _)) = connection else {
            return Ok::<_, TestError>(Vec::new());
        };
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let mut requests = Vec::new();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let request: Value = serde_json::from_str(&text)?;
            let method = request["method"].as_str().ok_or("missing native method")?;
            let result = match method {
                "initialize" => json!({}),
                "initialized" => continue,
                "thread/read" => {
                    let status = match reply {
                        QueueFallbackReply::Idle => "idle",
                        QueueFallbackReply::NotLoaded => "notLoaded",
                        _ => "active",
                    };
                    json!({"thread":{"id":THREAD,"status":{"type":status}}})
                }
                "thread/turns/list" => {
                    json!({"data":if matches!(reply, QueueFallbackReply::MissingTurn) {vec![]} else {vec![json!({"id":"active-native-turn","status":"inProgress"})]}})
                }
                "turn/steer" | "thread/queue/add" => {
                    if method
                        != if queue_supported {
                            "thread/queue/add"
                        } else {
                            "turn/steer"
                        }
                        || request["params"]["threadId"] != THREAD
                        || request["params"]["clientUserMessageId"] != "queue-fallback-correlation"
                        || request["params"]["input"]
                            != json!([{"type":"text","text":"queue fallback exact body λ"}])
                        || (!queue_supported
                            && request["params"]["expectedTurnId"] != "active-native-turn")
                    {
                        return Err("native queue fallback changed operation, target, correlation, turn or content".into());
                    }
                    if matches!(reply, QueueFallbackReply::Lost) {
                        requests.push(request);
                        break;
                    }
                    if matches!(reply, QueueFallbackReply::Rejected) {
                        socket.send(Message::Text(json!({"id":request["id"],"error":{"code":-32099,"message":"fixture arbitrary rejection"}}).to_string().into())).await?;
                        requests.push(request);
                        continue;
                    }
                    if queue_supported {
                        json!({"queuedSubmission":{"id":"native-queue-item"}})
                    } else {
                        json!({"turnId":"active-native-turn"})
                    }
                }
                _ => return Err("queue fallback resumed or started a turn".into()),
            };
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":result})
                        .to_string()
                        .into(),
                ))
                .await?;
            requests.push(request);
        }
        Ok::<_, TestError>(requests)
    });
    let outcome = dispatch_message(NativeMessageRequest {
        attempt_evidence: None,
        params: NativeMessageParams {
            target: target(),
            generation,
            body: NativeMessageBody::PreparedPush(
                "queue fallback exact body λ".to_owned().try_into()?,
            ),
            delivery: MessageDelivery::Queue,
            client_user_message_id: Some("queue-fallback-correlation".to_owned().try_into()?),
        },
        id: json!("queue-fallback-request"),
        service_id: &backend.endpoint.service_id,
        backend: &backend,
        endpoints: &[endpoint],
        held_connection: None,
        load_policy: LoadPolicy::MayLoad,
        approval_broker: None,
        recorded_route: None,
        attempt_deadline: None,
    })
    .await;
    let _ = finished_sender.send(());
    let requests = server.await??;
    let expected = if matches!(
        reply,
        QueueFallbackReply::Idle | QueueFallbackReply::NotLoaded
    ) {
        vec!["initialize", "thread/read"]
    } else if matches!(reply, QueueFallbackReply::MissingTurn) {
        vec!["initialize", "thread/read", "thread/turns/list"]
    } else if queue_supported {
        vec!["initialize", "thread/read", "thread/queue/add"]
    } else {
        vec![
            "initialize",
            "thread/read",
            "thread/turns/list",
            "turn/steer",
        ]
    };
    let observed = requests
        .iter()
        .map(|request| request["method"].as_str().ok_or("missing native method"))
        .collect::<Result<Vec<_>, _>>()?;
    if observed != expected {
        return Err(format!("queue fallback must inspect once and never replay a mutation: expected {expected:?}, observed {observed:?}").into());
    }
    Ok(outcome)
}

#[tokio::test]
async fn missing_native_queue_schema_steers_exact_message_into_active_turn() {
    let NativeMessageOutcome::Accepted(receipt) =
        queue_capability_probe(false, QueueFallbackReply::Accepted)
            .await
            .unwrap()
    else {
        panic!("missing queue capability must use native steer");
    };
    assert_eq!(
        String::from(receipt.client_user_message_id),
        "queue-fallback-correlation"
    );
    assert_eq!(
        receipt.resume_effect,
        collaboration_protocol::AcceptedResumeEffect::NotRequested
    );
    assert!(
        matches!(receipt.acceptance, collaboration_protocol::NativeSendAcceptance::SteerAccepted { turn_id, submission_id: None } if String::from(turn_id.clone()) == "active-native-turn")
    );
}

#[tokio::test]
async fn missing_native_queue_schema_preserves_active_turn_requirements() {
    for (reply, kind) in [
        (QueueFallbackReply::Idle, "noActiveTurn"),
        (QueueFallbackReply::NotLoaded, "noActiveTurn"),
        (QueueFallbackReply::MissingTurn, "localRejection"),
    ] {
        let NativeMessageOutcome::Failed(failure) =
            queue_capability_probe(false, reply).await.unwrap()
        else {
            panic!("native steer fallback must require an active turn");
        };
        assert_eq!(
            failure.response.pointer("/error/data/kind"),
            Some(&json!(kind))
        );
        assert_eq!(
            failure.response.pointer("/error/data/effects/submission"),
            Some(&json!("notDispatched"))
        );
        assert_eq!(
            failure.response.pointer("/error/data/effects/resume"),
            Some(&json!("notRequested"))
        );
        assert_eq!(
            failure.response.pointer("/error/data/stage"),
            Some(&json!("steer"))
        );
    }
}

#[tokio::test]
async fn native_queue_and_steer_failures_preserve_effects_without_replay() {
    for queue_supported in [false, true] {
        for (reply, kind, submission) in [
            (QueueFallbackReply::Rejected, "nativeRejected", "rejected"),
            (QueueFallbackReply::Lost, "outcomeUnknown", "unknown"),
        ] {
            let NativeMessageOutcome::Failed(failure) =
                queue_capability_probe(queue_supported, reply)
                    .await
                    .unwrap()
            else {
                panic!("native mutation failure must remain a failure");
            };
            assert_eq!(
                failure.response.pointer("/error/data/kind"),
                Some(&json!(kind))
            );
            assert_eq!(
                failure.response.pointer("/error/data/effects/submission"),
                Some(&json!(submission))
            );
            assert_eq!(
                failure.response.pointer("/error/data/effects/resume"),
                Some(&json!("notRequested"))
            );
            assert_eq!(
                failure.response.pointer("/error/data/clientUserMessageId"),
                Some(&json!("queue-fallback-correlation"))
            );
            assert_eq!(
                failure.response.pointer("/error/data/stage"),
                Some(&json!(if queue_supported { "queue" } else { "steer" }))
            );
        }
    }
}

#[tokio::test]
async fn policy_loaded_generic_preserves_each_mode_without_resume() {
    for mode in [
        MessageDelivery::Auto,
        MessageDelivery::Queue,
        MessageDelivery::Steer,
    ] {
        let fixture = RecordedPolicyFixture::create_without_route().await.unwrap();
        let server = policy_server(
            &fixture,
            PolicyServerCase {
                cold: ReplyPolicy::Matching,
                loaded: ReplyPolicy::Matching,
                loaded_delivery: mode,
                turn_intent: None,
                retire_loaded: None,
                stored_run: None,
            },
        )
        .unwrap();
        let broker: Arc<dyn ApprovalBroker> = fixture.broker.clone();
        let weak = Arc::downgrade(&broker);
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            dispatch_message(fixture.message_request(&weak)),
        )
        .await
        .unwrap();
        let mut request = fixture.message_request(&weak);
        request.params.delivery = mode;
        let second = tokio::time::timeout(Duration::from_secs(5), dispatch_message(request))
            .await
            .unwrap();
        let requests = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(first, NativeMessageOutcome::Accepted(_)));
        let NativeMessageOutcome::Accepted(receipt) = second else {
            panic!("generic loaded route refused");
        };
        assert_eq!(
            receipt.resume_effect,
            collaboration_protocol::AcceptedResumeEffect::NotRequested
        );
        assert!(
            requests[1]
                .iter()
                .all(|request| request["method"] != "thread/resume")
        );
        let method = match mode {
            MessageDelivery::Auto => "turn/start",
            MessageDelivery::Queue => "thread/queue/add",
            MessageDelivery::Steer => "turn/steer",
        };
        assert!(
            requests[1]
                .iter()
                .any(|request| request["method"] == method)
        );
    }
}

#[tokio::test]
async fn policy_loaded_expired_broker_refuses_each_mode_before_resume_or_input() {
    for mode in [
        MessageDelivery::Auto,
        MessageDelivery::Queue,
        MessageDelivery::Steer,
    ] {
        let mut fixture = RecordedPolicyFixture::create().await.unwrap();
        let server = policy_server(
            &fixture,
            PolicyServerCase {
                cold: ReplyPolicy::Matching,
                loaded: ReplyPolicy::Matching,
                loaded_delivery: mode,
                turn_intent: None,
                retire_loaded: None,
                stored_run: None,
            },
        )
        .unwrap();
        let broker: Arc<dyn ApprovalBroker> = fixture.broker.clone();
        let weak = Arc::downgrade(&broker);
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            dispatch_message(fixture.message_request(&weak)),
        )
        .await
        .unwrap();
        drop(broker);
        let interaction_store = Arc::new(
            interaction_storage::InteractionStore::open(
                fixture.directory.path().join("interaction.sqlite"),
            )
            .await
            .unwrap(),
        );
        fixture.broker = ServiceInteractionBroker::load(
            fixture.backend.endpoint.service_id.clone(),
            fixture.backend.clone(),
            interaction_store,
        )
        .await
        .unwrap();
        assert!(weak.upgrade().is_none());
        let mut request = fixture.message_request(&weak);
        request.params.delivery = mode;
        let second = tokio::time::timeout(Duration::from_secs(5), dispatch_message(request))
            .await
            .unwrap();
        let requests = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(first, NativeMessageOutcome::Accepted(_)));
        let NativeMessageOutcome::Failed(failure) = second else {
            panic!("expired authority was accepted");
        };
        assert_eq!(
            failure.response.pointer("/error/data/effects/resume"),
            Some(&json!("notRequested"))
        );
        assert_eq!(
            failure.response.pointer("/error/data/effects/submission"),
            Some(&json!("notDispatched"))
        );
        assert!(requests[1].iter().all(|request| !matches!(
            request["method"].as_str(),
            Some("thread/resume" | "turn/start" | "thread/queue/add" | "turn/steer")
        )));
    }
}
