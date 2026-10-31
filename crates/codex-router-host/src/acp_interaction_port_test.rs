use super::*;

struct StalledNoticeDelivery {
    entered: Arc<tokio::sync::Notify>,
    dropped: Arc<tokio::sync::Notify>,
}

struct NoticeInFlight(Arc<tokio::sync::Notify>);

impl Drop for NoticeInFlight {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

impl collaboration_contracts::SessionMessageDelivery for StalledNoticeDelivery {
    fn deliver<'a>(
        &'a self,
        _: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn collaboration_contracts::AttemptEvidenceSink,
    ) -> collaboration_contracts::DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult>
    {
        let entered = Arc::clone(&self.entered);
        let dropped = Arc::clone(&self.dropped);
        Box::pin(async move {
            let _in_flight = NoticeInFlight(dropped);
            entered.notify_one();
            std::future::pending().await
        })
    }

    fn reconcile_attempt(
        &self,
        _: collaboration_contracts::AttemptReconciliationContext,
    ) -> collaboration_contracts::DeliveryFuture<'_, collaboration_contracts::AttemptReconciliation>
    {
        Box::pin(async { Ok(collaboration_contracts::AttemptReconciliation::StillUnknown) })
    }
}

#[derive(Default)]
struct RecordedInteractionSink(std::sync::Mutex<Vec<SessionEvent>>);

impl SessionEventSink for RecordedInteractionSink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _: &str) -> acp_client_runtime::HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "A poisoned fixture event capture must fail its test."
    )]
    fn publish(
        &self,
        _: &str,
        event: SessionEvent,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        self.0.lock().expect("event lock").push(event);
        Ok(())
    }
}

#[tokio::test]
async fn stalled_session_notices_do_not_delay_turn_cancellation_or_resolution() {
    use acp_client_runtime::InteractionPort as _;
    let directory = tempfile::tempdir().expect("broker directory");
    let service_id: collaboration_protocol::UuidIdentity = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89"
        .to_owned()
        .try_into()
        .expect("service ID");
    let endpoint = collaboration_protocol::EndpointRef {
        service_id: service_id.clone(),
        endpoint_id: "cursor-local".to_owned().try_into().expect("endpoint ID"),
    };
    let broker = ServiceInteractionBroker::load(
        service_id.clone(),
        collaboration_runtime::NativeControlBackend {
            endpoint: endpoint.clone(),
            gate: collaboration_runtime::NativeGenerationGate::default(),
            codex_home: directory.path().to_owned(),
        },
        directory.path().join("approval-routes.json"),
    )
    .await
    .expect("broker");
    let push_store = Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&directory.path().join("approval-pushes.sqlite"))
            .await
            .expect("approval push store"),
    ));
    let machine_identity =
        collaboration_contracts::MachineIdentity::new(service_id.clone(), Some("fixture-host"))
            .expect("machine identity");
    let service_id_text = String::from(service_id.clone());
    let broker_context =
        collaboration_service::ApplicationFixture::new(&service_id_text, &service_id_text)
            .expect("service identity")
            .with_machine_identity(machine_identity)
            .expect("machine identity belongs to service")
            .with_automation_store(push_store)
            .with_approval_broker(Arc::clone(&broker));
    drop(broker_context);
    let notice_entered = Arc::new(tokio::sync::Notify::new());
    let notice_dropped = Arc::new(tokio::sync::Notify::new());
    broker
        .install_session_delivery(Arc::new(StalledNoticeDelivery {
            entered: Arc::clone(&notice_entered),
            dropped: Arc::clone(&notice_dropped),
        }))
        .expect("notice route");
    let sink = Arc::new(RecordedInteractionSink::default());
    let port = Arc::new(HostInteractionPort::default());
    port.install_broker(Arc::clone(&broker)).await;
    port.install_event_sink(Arc::clone(&sink) as Arc<dyn SessionEventSink>)
        .await;
    let target: collaboration_protocol::SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":endpoint,"sessionId":"provider-session"
    }))
    .expect("target");
    let approver: collaboration_protocol::SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":endpoint,"sessionId":"approver-session"
    }))
    .expect("approver");
    let context = || ExternalProviderApprovalContext {
        requester: target.clone().into(),
        approver: approver.clone().into(),
        target: target.clone(),
        operation_id: OperationId::generate(),
        binding_generation: serde_json::from_value(serde_json::json!({
            "serviceEpoch":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","generation":1
        }))
        .expect("generation"),
        binding_retirement: CancellationToken::new(),
    };
    let approval = serde_json::from_value(serde_json::json!({
            "requestId":"stalled-approval", "title":"Run command", "options":[
                {"optionId":"allow-once","label":"Allow once","choice":{"effect":"allow","scope":"once"}}
            ]
        })).expect("approval");
    let approval_cancel = CancellationToken::new();
    let approval_task = tokio::spawn({
        let port = Arc::clone(&port);
        let cancellation = approval_cancel.clone();
        let context = context();
        async move {
            port.request_approval(context, approval, cancellation, CancellationToken::new())
                .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), notice_entered.notified())
        .await
        .expect("approval notice entered");
    approval_cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_millis(500), approval_task)
            .await
            .expect("approval cancellation bounded")
            .expect("approval task"),
        ApprovalPortOutcome::Cancelled
    ));
    tokio::time::timeout(
        std::time::Duration::from_millis(500),
        notice_dropped.notified(),
    )
    .await
    .expect("settled approval aborted its notice task");
    let question = serde_json::from_value(serde_json::json!({
        "requestId":"stalled-question","prompt":"Proceed?","fields":[
            {"kind":"boolean","fieldId":"yes","label":"Yes","description":null,"required":true}
        ]
    }))
    .expect("question");
    let question_cancel = CancellationToken::new();
    let question_task = tokio::spawn({
        let port = Arc::clone(&port);
        let cancellation = question_cancel.clone();
        let context = context();
        async move {
            port.request_question(context, question, cancellation, CancellationToken::new())
                .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), notice_entered.notified())
        .await
        .expect("question notice entered");
    question_cancel.cancel();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_millis(500), question_task)
            .await
            .expect("question cancellation bounded")
            .expect("question task"),
        QuestionResponse::Cancelled
    );
    tokio::time::timeout(
        std::time::Duration::from_millis(500),
        notice_dropped.notified(),
    )
    .await
    .expect("settled question aborted its notice task");
    let events = sink.0.lock().expect("event lock");
    assert_eq!(events.len(), 4);
    assert!(matches!(
        events.first(),
        Some(SessionEvent::InteractionRequested { .. })
    ));
    assert!(
        matches!(events.get(1), Some(SessionEvent::InteractionResolved { request_id }) if request_id == "stalled-approval")
    );
    assert!(matches!(
        events.get(2),
        Some(SessionEvent::InteractionRequested { .. })
    ));
    assert!(
        matches!(events.get(3), Some(SessionEvent::InteractionResolved { request_id }) if request_id == "stalled-question")
    );
}

#[test]
fn human_approver_reaches_typed_broker_without_a_synthetic_session() {
    let target: collaboration_protocol::SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"cursor-local"},
        "sessionId":"provider-session"
    }))
    .expect("target");
    let context = ExternalProviderApprovalContext {
        requester: target.clone().into(),
        approver: serde_json::from_value(serde_json::json!({"humanId":"owner"})).expect("human"),
        target,
        operation_id: collaboration_protocol::OperationId::generate(),
        binding_generation: serde_json::from_value(serde_json::json!({
            "serviceEpoch":"1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80","generation":1
        }))
        .expect("generation"),
        binding_retirement: CancellationToken::new(),
    };
    let (_, approver) = typed_participants(&context).expect("broker participants");
    assert!(matches!(approver, Identity::Human { human_id } if human_id.as_str() == "owner"));
}
