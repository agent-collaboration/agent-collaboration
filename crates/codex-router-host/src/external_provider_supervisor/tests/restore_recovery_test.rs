//! Local restore retirement and admission, through the real owning store.

#[path = "restore_recovery/admission_budget_test.rs"]
mod admission_budget_tests;

#[path = "restore_recovery/cancel_confirmation_test.rs"]
mod cancel_confirmation_tests;

#[path = "restore_recovery/refusal_answers_test.rs"]
mod refusal_answers_tests;

#[path = "restore_recovery/cursor_lost_session_test.rs"]
mod cursor_lost_session_tests;

use super::*;
use collaboration_contracts::{
    DeliveryPrecondition, LoadPolicy, SessionDeliveryRoute, UnstoredAttemptEvidenceSink,
    layer_zero::{DeliveryRequest, PreparedPush},
};
use collaboration_protocol::{
    AttemptId, ChannelDescription, ConversationCloseRequest, ConversationLoadRequest,
    ConversationOperationShowRequest, ConversationPromptRequest, ConversationResumeRequest,
    DeliveryCorrelationId, DeliveryOutcome, EndpointAvailability, EndpointDescription,
    MessageContent, MessageDelivery, ObservationTimestamp,
};
use collaboration_runtime::{EndpointDirectory, ProviderSessionEventHub, ProviderSessionRecord};
use router_clock::{ClockInstant, ManualClock, RuntimeClock as _};

async fn wait_for_queued_input(
    host: &ExternalProviderSupervisor,
    target: &SessionRef,
    operation: &OperationId,
) {
    let operation = uuid::Uuid::parse_str(operation.as_str()).expect("operation identity");
    host.session_owner_for(target)
        .expect("registered owner")
        .snapshots()
        .wait_for(|snapshot| {
            snapshot
                .queue
                .iter()
                .any(|entry| entry.input.operation == operation)
        })
        .await
        .expect("prompt is waiting on that owner");
}

struct RestoreRig {
    root: tempfile::TempDir,
    target: SessionRef,
    clock: Arc<ManualClock>,
}

impl RestoreRig {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("restore root"),
            target: SessionRef {
                endpoint: endpoint(),
                session_id: SessionId::try_from("saved-session".to_owned()).expect("session"),
            },
            clock: Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
        }
    }

    fn working_directory(&self) -> ProviderWorkingDirectory {
        ProviderWorkingDirectory::try_from(self.root.path().display().to_string())
            .expect("working directory")
    }

    async fn store(&self) -> ProviderOperationStore {
        ProviderOperationStore::open(&self.root.path().join("operations.sqlite"))
            .await
            .expect("store")
    }

    async fn seed_session(&self) {
        self.store()
            .await
            .record_session(&ProviderSessionRecord {
                target: self.target.clone(),
                working_directory: self.working_directory(),
                requested_policy: policy(),
                created_by: self.target.clone().into(),
                approver: self.target.clone().into(),
                updated_at_ms: 1,
                resume: Default::default(),
            })
            .await
            .expect("saved session");
    }

    async fn open_host(&self, behavior: &str) -> ExternalProviderSupervisor {
        self.open_host_with(&[behavior]).await
    }

    async fn open_host_with(&self, behaviors: &[&str]) -> ExternalProviderSupervisor {
        self.open_host_with_adapter(
            behaviors,
            Arc::new(acp_provider_adapters::StandardAcpBehavior),
        )
        .await
    }

    async fn open_host_with_adapter(
        &self,
        behaviors: &[&str],
        adapter: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> ExternalProviderSupervisor {
        let store = Arc::new(Mutex::new(self.store().await));
        let hub = Arc::new(ProviderSessionEventHub::new(store.clone()));
        let target: message_board::SessionRef =
            serde_json::from_value(serde_json::to_value(&self.target).expect("target JSON"))
                .expect("hub target");
        let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
            crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
                "permission-mode-agent.ts",
                vec![
                    self.root.path().join("wire.json").display().to_string(),
                    "cursor".into(),
                    "no-directories".into(),
                    "advertise-restore".into(),
                    "plain-prompts".into(),
                ]
                .into_iter()
                .chain(behaviors.iter().map(|behavior| (*behavior).to_owned()))
                .collect(),
            ),
            acp_client_runtime::ProviderModelPicker::Standard,
            "fixture",
            "http://127.0.0.1:1/mcp",
            hub,
            target.endpoint,
            adapter,
            self.clock.clone(),
        )
        .await
        .expect("fixture runtime");
        // Keep the fixture's binding generation across reseeding so it
        // exercises the same-generation persisted uncertainty rule.
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding(),
                runtime,
            }],
            store,
        )
        .expect("supervisor")
    }

    fn delivery_route(
        &self,
        host: Arc<ExternalProviderSupervisor>,
    ) -> crate::ProviderAcpDeliveryRoute {
        let directory = EndpointDirectory::new(self.target.endpoint.service_id.clone());
        let binding = binding();
        directory
            .publish(EndpointDescription {
                endpoint: self.target.endpoint.clone(),
                label: NonEmptyText::try_from("Cursor fixture".to_owned()).expect("label"),
                availability: EndpointAvailability::Available {
                    observed_at: ObservationTimestamp::try_from("2026-11-19T12:00:00Z".to_owned())
                        .expect("timestamp"),
                },
                channels: vec![ChannelDescription::ExternalProvider {
                    transport: binding.transport,
                    binding_id: binding.binding_id,
                    binding_generation: binding.generation.generation,
                    runtime: binding.runtime,
                    capabilities: binding.capabilities,
                    auto_realization: None,
                }],
            })
            .expect("endpoint");
        crate::ProviderAcpDeliveryRoute::new(
            self.target.endpoint.service_id.clone(),
            std::iter::once(self.target.endpoint.clone()).collect(),
            directory,
            host.clone(),
            host.inner.store.clone(),
            Arc::new(NoLivePeer),
            self.clock.clone(),
        )
    }

    fn load_request(&self, operation_id: OperationId) -> ConversationLoadRequest {
        ConversationLoadRequest {
            operation_id,
            target: self.target.clone(),
            generation: Some(generation()),
            working_directory: self.working_directory(),
            requested_by: self.target.clone().into(),
            approver: self.target.clone().into(),
            requested_policy: policy(),
        }
    }

    fn resume_request(&self, operation_id: OperationId) -> ConversationResumeRequest {
        ConversationResumeRequest {
            operation_id,
            target: self.target.clone(),
            generation: Some(generation()),
            working_directory: self.working_directory(),
            requested_by: self.target.clone().into(),
            approver: self.target.clone().into(),
            requested_policy: policy(),
        }
    }

    fn wire_receipt(&self) -> serde_json::Value {
        serde_json::from_slice(
            &std::fs::read(self.root.path().join("wire.json")).expect("wire receipt"),
        )
        .expect("wire JSON")
    }

    async fn until_wire_request(&self, method: &str) {
        while !std::fs::read(self.root.path().join("wire.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|wire| wire[method].as_array().map(|rows| !rows.is_empty()))
            .unwrap_or(false)
        {
            tokio::task::yield_now().await;
        }
    }

    fn prompt_request(&self, id: OperationId, text: &str) -> ConversationPromptRequest {
        ConversationPromptRequest {
            delivery: MessageDelivery::Auto,
            input_id: None,
            operation_id: id,
            target: self.target.clone(),
            generation: Some(generation()),
            requested_by: self.target.clone().into(),
            approver: self.target.clone().into(),
            prompt: MessageContent::HumanUser {
                text: MessageText::try_from(text.to_owned()).expect("prompt text"),
            },
        }
    }

    async fn assert_loads(&self, host: &ExternalProviderSupervisor) {
        let id = OperationId::generate();
        host.load(self.load_request(id.clone()))
            .await
            .expect("restore admitted");
        let settled = host.wait(wait_request(id)).await.expect("restore settled");
        assert!(
            matches!(settled.output, ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Loaded { ref target, .. }
        } if target == &self.target),
            "{settled:?}"
        );
    }
}

fn policy() -> ProviderRequestedPolicy {
    ProviderRequestedPolicy {
        access: RouterAccess::WriteRestricted,
    }
}

fn wait_request(operation_id: OperationId) -> ConversationOperationWaitRequest {
    ConversationOperationWaitRequest {
        operation_id,
        timeout_seconds: PositiveSeconds::try_from(5).expect("wait budget"),
    }
}

fn auto_message(target: SessionRef, text: &str, load_policy: LoadPolicy) -> DeliveryRequest {
    let push_id = PushId::try_from(AttemptId::generate().as_str().to_owned()).expect("push ID");
    let correlation =
        DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).expect("correlation");
    DeliveryRequest {
        payload: PreparedPush {
            line: MessageText::try_from(format!(
                "✉️ sender · \"{text}\" · router://{}/push/{}",
                String::from(target.endpoint.service_id.clone()),
                push_id.as_str()
            ))
            .expect("prepared input"),
            push_id,
            load_policy,
        },
        target,
        mode: MessageDelivery::Auto,
        precondition: DeliveryPrecondition::Unpinned,
        correlation,
        attempt: AttemptId::generate(),
    }
}

#[tokio::test]
async fn a_cold_auto_restore_timeout_finishes_the_input_without_holding_it() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = Arc::new(rig.open_host("silent-first-restore").await);
    let route = rig.delivery_route(host.clone());
    let not_loaded = route
        .deliver(
            auto_message(rig.target.clone(), "load forbidden", LoadPolicy::LoadedOnly),
            &UnstoredAttemptEvidenceSink,
        )
        .await
        .expect("loaded-only receipt");
    assert!(matches!(
        not_loaded.receipt().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    assert!(
        !rig.root.path().join("wire.json").exists(),
        "the loaded-only attempt reached ACP"
    );

    let cold_delivery = route.deliver(
        auto_message(rig.target.clone(), "retire this input", LoadPolicy::MayLoad),
        &UnstoredAttemptEvidenceSink,
    );
    tokio::pin!(cold_delivery);
    let timeout = std::time::Duration::from_secs(30);
    tokio::select! {
        () = rig.clock.not_due_check_now(ClockInstant::ZERO.after(timeout)) => {},
        receipt = &mut cold_delivery => panic!("cold load ended before its deadline: {receipt:?}"),
    }
    rig.clock.advance(timeout).expect("reach restore deadline");
    let actual = cold_delivery.await.expect("terminal cold delivery receipt");
    assert!(
        matches!(actual.receipt().outcome, DeliveryOutcome::NotSubmitted { ref reason, retryable: false } if reason.contains("did not answer the restore in time, so retry this session")),
        "{actual:?}"
    );

    rig.assert_loads(&host).await;
    let next = auto_message(
        rig.target.clone(),
        "deliver this newer input",
        LoadPolicy::MayLoad,
    );
    let next_id =
        OperationId::try_from(next.attempt.as_str().to_owned()).expect("next operation ID");
    let sent = route
        .deliver(next, &UnstoredAttemptEvidenceSink)
        .await
        .expect("later input delivered");
    assert!(
        matches!(sent.receipt().outcome, DeliveryOutcome::Started),
        "{sent:?}"
    );
    let completed = host
        .wait(wait_request(next_id))
        .await
        .expect("later input settles");
    assert!(matches!(
        completed.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    let observed: serde_json::Value = serde_json::from_slice(
        &std::fs::read(rig.root.path().join("wire.json")).expect("wire receipt"),
    )
    .expect("wire JSON");
    assert_eq!(
        observed["restores"]
            .as_array()
            .expect("restore requests")
            .len(),
        2
    );
    assert_eq!(
        observed["prompts"],
        serde_json::json!([rig.target.session_id])
    );
    route.shutdown_queue().await;
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn a_cold_auto_load_with_failed_host_preparation_settles_without_holding_input() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = Arc::new(rig.open_host("restored-in-auto").await);
    let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
    runtime.preparation_fixture.fault_restore();
    let route = rig.delivery_route(host.clone());

    let delivery = route
        .deliver(
            auto_message(rig.target.clone(), "do not hold me", LoadPolicy::MayLoad),
            &UnstoredAttemptEvidenceSink,
        )
        .await
        .expect("terminal cold input receipt");
    assert!(
        !runtime.preparation_fixture.fault_pending(),
        "the successful ACP reply reached Host preparation"
    );
    let observed: serde_json::Value = serde_json::from_slice(
        &std::fs::read(rig.root.path().join("wire.json")).expect("wire receipt"),
    )
    .expect("wire JSON");
    assert_eq!(
        observed["restores"]
            .as_array()
            .expect("restore requests")
            .len(),
        1
    );
    assert_eq!(
        observed
            .pointer("/restores/0/method")
            .and_then(serde_json::Value::as_str),
        Some("session/load")
    );
    assert_eq!(observed.get("prompts"), Some(&serde_json::json!([])));

    assert!(
        matches!(
            delivery.receipt().outcome,
            DeliveryOutcome::NotSubmitted { ref reason, retryable: false }
                if reason.contains("retry the restore")
        ),
        "a post-reply failure must finish the input with recovery guidance: {delivery:?}"
    );
    route.shutdown_queue().await;
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn cold_prompt_and_compact_keep_the_post_reply_restore_guidance() {
    for compact in [false, true] {
        let rig = RestoreRig::new();
        rig.seed_session().await;
        let host = Arc::new(rig.open_host("restored-in-auto").await);
        let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
        runtime.preparation_fixture.fault_restore();
        let route = rig.delivery_route(host.clone());
        let actor = rig.target.clone().into();
        let input_id = session_event_model::InputId::generate();
        let result = if compact {
            route.compact(rig.target.clone(), actor, input_id).await
        } else {
            let prompt = session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::text("do not submit".to_owned())
                    .expect("prompt content"),
            ]);
            route
                .prompt_contents(rig.target.clone(), actor, input_id, prompt)
                .await
        };
        let failure = result.expect_err("the load's Host preparation failed");
        assert!(!runtime.preparation_fixture.fault_pending());
        assert!(
            matches!(failure, crate::ProviderPromptContentsError::RestoreFailed { ref reason }
                if reason.contains("retry the restore")),
            "{failure:?}"
        );
        route.shutdown_queue().await;
        host.shutdown().await.expect("stop Host");
    }
}

#[derive(Clone, Copy, Debug)]
enum FailedRestoreActivation {
    AutoLoad,
    QueueLoad,
    Resume,
}

#[tokio::test]
async fn a_failed_host_activation_retires_only_its_restore_and_never_sends_the_old_input() {
    use FailedRestoreActivation::{AutoLoad, QueueLoad, Resume};

    for path in [AutoLoad, QueueLoad, Resume] {
        let rig = RestoreRig::new();
        rig.seed_session().await;
        let host = Arc::new(rig.open_host("restored-in-auto").await);
        let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
        let route = rig.delivery_route(host.clone());
        let (reached, release) = runtime.preparation_fixture.pause_restore();
        let queued_accepted = tokio::sync::Notify::new();
        let restore = async {
            match path {
                AutoLoad | QueueLoad => {
                    let mut input =
                        auto_message(rig.target.clone(), "do not send", LoadPolicy::MayLoad);
                    if matches!(path, QueueLoad) {
                        input.mode = MessageDelivery::Queue;
                    }
                    let id =
                        OperationId::try_from(input.attempt.as_str().to_owned()).expect("input ID");
                    let delivery = route
                        .deliver(input, &UnstoredAttemptEvidenceSink)
                        .await
                        .expect("delivery receipt");
                    if matches!(path, QueueLoad) {
                        assert_eq!(delivery.receipt().outcome, DeliveryOutcome::Queued);
                        queued_accepted.notify_one();
                        let failure = host
                            .wait(wait_request(id))
                            .await
                            .expect_err("queued input ends unsent");
                        assert_eq!(failure.effect, ProviderOperationEffect::None);
                    } else {
                        assert!(
                            matches!(
                                delivery.receipt().outcome,
                                DeliveryOutcome::NotSubmitted {
                                    retryable: false,
                                    ..
                                }
                            ),
                            "{delivery:?}"
                        );
                    }
                }
                Resume => {
                    let id = OperationId::generate();
                    host.resume(rig.resume_request(id.clone()))
                        .await
                        .expect("resume admitted");
                    let failure = host
                        .wait(wait_request(id))
                        .await
                        .expect_err("resume fails at the Host");
                    assert_eq!(failure.effect, ProviderOperationEffect::Applied);
                }
            }
        };
        let fault = async {
            reached.await.expect("ACP prepared the restored actor");
            if matches!(path, QueueLoad) {
                queued_accepted.notified().await;
            }
            assert!(
                matches!(
                    host.owner_session_activity(&rig.target),
                    crate::ProviderSessionActivity::NotLoaded
                ),
                "{path:?}"
            );
            runtime.preparation_fixture.fault_restore();
            release.send(()).expect("continue activation");
        };
        tokio::join!(restore, fault);

        assert!(
            matches!(
                host.owner_session_activity(&rig.target),
                crate::ProviderSessionActivity::NotLoaded
            ),
            "{path:?} left its generation live",
        );
        let wire = rig.wire_receipt();
        let expected_method = match path {
            AutoLoad | QueueLoad => "session/load",
            Resume => "session/resume",
        };
        assert_eq!(wire["restores"].as_array().expect("restores").len(), 1);
        assert_eq!(wire["restores"][0]["method"], expected_method);
        assert_eq!(wire["prompts"], serde_json::json!([]), "{path:?}");

        rig.assert_loads(&host).await;
        let next = auto_message(
            rig.target.clone(),
            "only send this new input",
            LoadPolicy::MayLoad,
        );
        let id = OperationId::try_from(next.attempt.as_str().to_owned()).expect("input ID");
        let sent = route
            .deliver(next, &UnstoredAttemptEvidenceSink)
            .await
            .expect("later input sent");
        assert_eq!(sent.receipt().outcome, DeliveryOutcome::Started);
        host.wait(wait_request(id))
            .await
            .expect("later input settles");
        let wire = rig.wire_receipt();
        assert_eq!(wire["restores"].as_array().expect("restores").len(), 2);
        assert_eq!(
            wire["prompts"],
            serde_json::json!([rig.target.session_id]),
            "{path:?}"
        );
        route.shutdown_queue().await;
        host.shutdown().await.expect("stop Host");
    }
}

#[tokio::test]
async fn a_refused_close_cannot_revive_a_restore_retired_before_commit() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig
        .open_host_with(&["restored-in-auto", "advertise-close", "hold-close-refusal"])
        .await;
    let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
    let (reached, release) = runtime.preparation_fixture.pause_restore();
    let restoring = OperationId::generate();
    host.load(rig.load_request(restoring.clone()))
        .await
        .expect("restore admitted");
    reached.await.expect("ACP prepared its route");
    let closing = OperationId::generate();
    host.close(ConversationCloseRequest {
        operation_id: closing.clone(),
        target: rig.target.clone(),
        generation: Some(generation()),
        requested_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
    })
    .await
    .expect("competing close admitted");
    rig.until_wire_request("closes").await;
    release
        .send(())
        .expect("release the stale Host preparation");
    let restore_failure = host
        .wait(wait_request(restoring))
        .await
        .expect_err("close retired the restore before Host commit");
    assert_eq!(restore_failure.effect, ProviderOperationEffect::Applied);
    node_test_fixture::release_fixture(&rig.root.path().join("wire.json"), ".release-close")
        .expect("let ACP refuse close");
    let close_failure = host
        .wait(wait_request(closing))
        .await
        .expect_err("close refused by ACP");
    assert_eq!(close_failure.effect, ProviderOperationEffect::None);
    assert!(
        matches!(
            host.owner_session_activity(&rig.target),
            crate::ProviderSessionActivity::NotLoaded
        ),
        "refused competing close reopened the failed restore"
    );
    assert_eq!(rig.wire_receipt()["prompts"], serde_json::json!([]));

    rig.assert_loads(&host).await;
    let input = OperationId::generate();
    host.prompt(rig.prompt_request(input.clone(), "one new prompt"))
        .await
        .expect("retry accepted new input");
    host.wait(wait_request(input))
        .await
        .expect("new input finished");
    assert_eq!(
        rig.wire_receipt()["prompts"],
        serde_json::json!([rig.target.session_id])
    );
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn a_public_prompt_stays_local_until_host_activation_settles() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("silent-prompts").await;
    let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
    let (reached, release) = runtime.preparation_fixture.pause_restore();
    let restoring = OperationId::generate();
    host.load(rig.load_request(restoring.clone()))
        .await
        .expect("restore admitted");
    reached.await.expect("ACP prepared its route");
    let input = OperationId::generate();
    host.prompt(rig.prompt_request(input.clone(), "do not send this input"))
        .await
        .expect("public prompt admitted");
    wait_for_queued_input(&host, &rig.target, &input).await;
    assert_eq!(rig.wire_receipt()["prompts"], serde_json::json!([]));
    runtime.preparation_fixture.fault_restore();
    release.send(()).expect("fail Host activation");
    let (restore_failure, refusal) = tokio::join!(
        host.wait(wait_request(restoring)),
        host.wait(wait_request(input.clone()))
    );
    assert_eq!(
        restore_failure.expect_err("restore failed").effect,
        ProviderOperationEffect::Applied
    );
    let refusal = refusal.expect_err("provisional input was refused locally");
    assert_eq!(refusal.effect, ProviderOperationEffect::None);
    assert_eq!(
        String::from(refusal.message),
        "the restore did not activate; load or resume this session and retry the input"
    );
    let stored = host
        .wait(wait_request(input))
        .await
        .expect_err("refusal was retained");
    assert_eq!(stored.effect, ProviderOperationEffect::None);
    assert_eq!(
        rig.clock.monotonic_now(),
        ClockInstant::ZERO,
        "retirement must not need a timeout"
    );
    assert!(matches!(
        host.owner_session_activity(&rig.target),
        crate::ProviderSessionActivity::NotLoaded
    ));
    assert_eq!(rig.wire_receipt()["prompts"], serde_json::json!([]));
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn successful_host_activation_releases_the_waiting_public_prompt() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
    let (reached, release) = runtime.preparation_fixture.pause_restore();
    let restoring = OperationId::generate();
    host.load(rig.load_request(restoring.clone()))
        .await
        .expect("restore admitted");
    reached.await.expect("ACP prepared its route");
    let input = OperationId::generate();
    host.prompt(rig.prompt_request(input.clone(), "wait to send this input"))
        .await
        .expect("public prompt admitted");
    wait_for_queued_input(&host, &rig.target, &input).await;
    assert_eq!(rig.wire_receipt()["prompts"], serde_json::json!([]));
    release.send(()).expect("publish Host activation");
    host.wait(wait_request(restoring))
        .await
        .expect("restore activated");
    let finished = host
        .wait(wait_request(input))
        .await
        .expect("prompt completed");
    assert!(matches!(
        finished.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    assert_eq!(
        rig.wire_receipt()["prompts"],
        serde_json::json!([rig.target.session_id])
    );
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn a_stale_restore_activation_failure_leaves_the_newer_run_live() {
    let rig = RestoreRig::new();
    let host = rig.open_host("restored-in-auto").await;
    let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
    let session_id = String::from(rig.target.session_id.clone());
    let (reached, release) = runtime.preparation_fixture.pause_restore();
    let earlier = runtime.load_session(session_id.clone(), rig.root.path().to_path_buf());
    let replacement = async {
        reached.await.expect("the first ACP actor registered");
        runtime
            .release_session(session_id.clone())
            .await
            .expect("release the first run");
        runtime
            .load_session(session_id.clone(), rig.root.path().to_path_buf())
            .await
            .expect("activate the replacement");
        release.send(()).expect("finish old activation");
    };
    let (earlier, ()) = tokio::join!(earlier, replacement);

    assert!(
        matches!(earlier, Err(ExternalProviderRuntimeError::SessionReleased)),
        "{earlier:?}"
    );
    assert!(matches!(
        runtime.session_activity(session_id).await,
        Ok(crate::ProviderSessionActivity::Idle)
    ));
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn explicit_restore_reports_recovery_guidance_and_keeps_its_stored_effect() {
    for resume in [false, true] {
        let rig = RestoreRig::new();
        rig.seed_session().await;
        let host = rig.open_host("restored-in-auto").await;
        let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
        runtime.preparation_fixture.fault_restore();
        let id = OperationId::generate();
        let admitted = if resume {
            host.resume(rig.resume_request(id.clone())).await
        } else {
            host.load(rig.load_request(id.clone())).await
        };
        admitted.expect("restore operation admitted");
        let failure = host
            .wait(wait_request(id.clone()))
            .await
            .expect_err("restore Host preparation failed");
        assert_eq!(
            failure.kind,
            collaboration_protocol::ConversationOperationFailureKind::Unavailable
        );
        assert_eq!(failure.effect, ProviderOperationEffect::Applied);
        assert_eq!(
            String::from(failure.message),
            "retry the restore when the Router can prepare the session"
        );
        let stored = rig
            .store()
            .await
            .inspect(&id)
            .await
            .expect("read operation")
            .expect("stored restore");
        assert_eq!(stored.stage, ProviderOperationStage::Terminal);
        assert_eq!(stored.effect, ProviderOperationEffect::Applied);
        assert_eq!(
            stored.reconciliation_state,
            ProviderReconciliationState::Confirmed
        );
        assert_eq!(rig.wire_receipt()["prompts"], serde_json::json!([]));
        host.shutdown().await.expect("stop Host");
    }
}

#[tokio::test]
async fn a_retired_load_allows_a_same_session_retry_before_and_after_store_reopen() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("silent-first-restore").await;
    let id = OperationId::generate();
    host.load(rig.load_request(id.clone()))
        .await
        .expect("first load admitted");
    let timeout = std::time::Duration::from_secs(30);
    rig.clock
        .not_due_check_now(ClockInstant::ZERO.after(timeout))
        .await;
    rig.clock
        .advance(timeout)
        .expect("reach the restore deadline");
    let failure = host
        .wait(wait_request(id.clone()))
        .await
        .expect_err("first load has no answer");
    assert_eq!(failure.effect, ProviderOperationEffect::Unknown);
    assert!(String::from(failure.message).contains("retry this session"));
    let snapshot = host
        .show(ConversationOperationShowRequest {
            operation_id: id.clone(),
        })
        .await
        .expect("first load record");
    assert_eq!(snapshot.stage, ProviderOperationStage::Terminal);
    assert_eq!(
        snapshot.reconciliation,
        ProviderReconciliationState::Unresolved
    );

    rig.assert_loads(&host).await;
    host.shutdown().await.expect("stop the first Host");
    drop(host);
    let reopened = rig.open_host("plain-prompts").await;
    rig.assert_loads(&reopened).await;
    let earlier = rig
        .store()
        .await
        .inspect(&id)
        .await
        .expect("read retired load")
        .expect("retired load retained");
    assert_eq!(earlier.effect, ProviderOperationEffect::Unknown);
    assert_eq!(
        earlier.reconciliation_state,
        ProviderReconciliationState::Unresolved
    );
    reopened.shutdown().await.expect("stop reopened Host");
}

#[tokio::test]
async fn a_disconnect_during_restore_keeps_unknown_effect_and_reconnect_guidance() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("disconnect-during-restore").await;
    let id = OperationId::generate();
    host.load(rig.load_request(id.clone()))
        .await
        .expect("load admitted");
    let failure = host
        .wait(wait_request(id.clone()))
        .await
        .expect_err("connection closed during restore");
    assert_eq!(failure.effect, ProviderOperationEffect::Unknown);
    assert!(String::from(failure.message).contains("reconnect and retry this session"));
    let row = rig
        .store()
        .await
        .inspect(&id)
        .await
        .expect("operation read")
        .expect("load recorded");
    assert_eq!(row.stage, ProviderOperationStage::Terminal);
    assert_eq!(
        row.reconciliation_state,
        ProviderReconciliationState::Unresolved
    );
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn an_uncertain_prompt_still_blocks_restore_before_and_after_store_reopen() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("refuse-prompt").await;
    rig.assert_loads(&host).await;
    let id = OperationId::generate();
    host.prompt(ConversationPromptRequest {
        delivery: MessageDelivery::Auto,
        input_id: None,
        operation_id: id.clone(),
        target: rig.target.clone(),
        generation: Some(generation()),
        requested_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
        prompt: MessageContent::HumanUser {
            text: MessageText::try_from("test input".to_owned()).expect("prompt text"),
        },
    })
    .await
    .expect("prompt admitted");
    let uncertainty = host
        .wait(wait_request(id.clone()))
        .await
        .expect_err("prompt effect is uncertain");
    assert_eq!(uncertainty.effect, ProviderOperationEffect::Unknown);
    assert_restore_blocked(&rig, &host, &id).await;
    host.shutdown().await.expect("stop first Host");
    drop(host);

    let reopened = rig.open_host("plain-prompts").await;
    assert_restore_blocked(&rig, &reopened, &id).await;
    reopened.shutdown().await.expect("stop reopened Host");
}

async fn assert_restore_blocked(
    rig: &RestoreRig,
    host: &ExternalProviderSupervisor,
    earlier: &OperationId,
) {
    let blocked = host
        .load(rig.load_request(OperationId::generate()))
        .await
        .expect_err("uncertain prompt must still guard admission");
    assert_eq!(blocked.kind, ConversationOperationFailureKind::Busy);
    assert!(String::from(blocked.message).contains(earlier.as_str()));
}
