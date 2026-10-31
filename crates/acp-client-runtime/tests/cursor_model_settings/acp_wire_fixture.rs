use std::{path::PathBuf, sync::Arc};

use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderBehavior,
    ProviderPersistenceTarget, RefusedApprovalOffer, SessionEventSink,
};
use session_event_model::{ApprovalRequest, SessionEvent, SessionSettings};
use tokio_util::sync::CancellationToken;

pub struct NoopInteractionPort;

struct SharedSettingsBehavior;

impl ProviderBehavior for SharedSettingsBehavior {
    fn can_reuse_setting_confirmation(&self, _: acp_client_runtime::ProviderSettingKind) -> bool {
        false
    }
    fn model_metadata_source(&self) -> acp_client_runtime::ModelMetadataSource {
        crate::test_behavior::TestProviderBehavior.model_metadata_source()
    }
    fn decode_model_metadata(
        &self,
        value: &serde_json::Value,
    ) -> acp_client_runtime::ModelReasoningMetadata {
        crate::test_behavior::TestProviderBehavior.decode_model_metadata(value)
    }
    fn observe(
        &self,
        update: acp_client_runtime::ProviderUpdate,
    ) -> Result<acp_client_runtime::ProviderFacts, acp_client_runtime::ProviderError> {
        crate::test_behavior::TestProviderBehavior.observe(update)
    }
    fn prepare(
        &self,
        submission: session_event_model::ProviderSubmission,
    ) -> Result<Vec<session_event_model::PromptContent>, acp_client_runtime::ProviderError> {
        crate::test_behavior::TestProviderBehavior.prepare(submission)
    }
    fn compact(
        &self,
    ) -> Result<acp_client_runtime::PreparedCompact, acp_client_runtime::ProviderError> {
        crate::test_behavior::TestProviderBehavior.compact()
    }
}

impl InteractionPort for NoopInteractionPort {
    type Context = ();
    type OperationId = u64;

    fn operation_id(_: &()) -> u64 {
        1
    }
    fn binding_retirement(_: &()) -> CancellationToken {
        CancellationToken::new()
    }
    fn request_approval(
        &self,
        _: (),
        _: ApprovalRequest,
        _: CancellationToken,
        _: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }
    fn request_question(
        &self,
        _: (),
        _: session_event_model::QuestionRequest,
        _: CancellationToken,
        _: CancellationToken,
    ) -> InteractionFuture<'_, session_event_model::QuestionResponse> {
        Box::pin(async { session_event_model::QuestionResponse::Cancelled })
    }
    fn record_refusal(&self, _: (), _: RefusedApprovalOffer) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_all(&self, _: (), _: &'static str) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

#[derive(Default)]
pub struct SettingsSink {
    pub settings: std::sync::Mutex<Vec<SessionSettings>>,
    pub changed: tokio::sync::Notify,
}

impl SessionEventSink for SettingsSink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    #[expect(
        clippy::unwrap_in_result,
        reason = "This test double must fail on broken fixture invariants instead of reporting a product error."
    )]
    fn publish(&self, _: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        if let SessionEvent::SettingsChanged { settings } = event {
            self.settings.lock().expect("settings lock").push(settings);
            self.changed.notify_one();
        }
        Ok(())
    }
}

pub struct AcpWireFixture {
    pub root: tempfile::TempDir,
    pub receipt: PathBuf,
    pub client: AgentSessionClient<NoopInteractionPort>,
    pub sink: Arc<SettingsSink>,
}

enum ClientInitialization {
    Standard,
    CursorWithMcp,
    #[cfg(feature = "test-observation")]
    CursorWithoutMcp,
}

impl AcpWireFixture {
    pub async fn start(scenario: &str, cursor_picker: bool) -> Self {
        Self::initialize(
            scenario,
            if cursor_picker {
                ClientInitialization::CursorWithMcp
            } else {
                ClientInitialization::Standard
            },
        )
        .await
    }

    #[cfg(feature = "test-observation")]
    pub async fn start_without_mcp(scenario: &str) -> Self {
        Self::initialize(scenario, ClientInitialization::CursorWithoutMcp).await
    }

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    async fn initialize(scenario: &str, initialization: ClientInitialization) -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let receipt = root.path().join("wire.json");
        let launch = ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../verification/fixtures/cursor-settings-agent.ts"
                )
                .into(),
                scenario.into(),
                receipt.to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        };
        let sink = Arc::new(SettingsSink::default());
        let behavior: Arc<dyn ProviderBehavior> = if scenario == "shared-settings" {
            Arc::new(SharedSettingsBehavior)
        } else {
            Arc::new(crate::test_behavior::TestProviderBehavior)
        };
        let client = match initialization {
            ClientInitialization::CursorWithMcp => {
                AgentSessionClient::initialize_with_mcp_http(
                    launch,
                    acp_client_runtime::ProviderModelPicker::CursorParameterized,
                    "fixture",
                    "http://127.0.0.1:1/mcp",
                    Arc::new(NoopInteractionPort),
                    sink.clone(),
                    behavior,
                )
                .await
            }
            ClientInitialization::Standard => {
                AgentSessionClient::initialize(
                    launch,
                    Arc::new(NoopInteractionPort),
                    sink.clone(),
                    behavior,
                )
                .await
            }
            #[cfg(feature = "test-observation")]
            ClientInitialization::CursorWithoutMcp => {
                AgentSessionClient::initialize_with_model_picker(
                    launch,
                    acp_client_runtime::ProviderModelPicker::CursorParameterized,
                    Arc::new(NoopInteractionPort),
                    sink.clone(),
                    behavior,
                )
                .await
            }
        }
        .expect("peer initializes");
        Self {
            root,
            receipt,
            client,
            sink,
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    pub fn requests(&self) -> Vec<serde_json::Value> {
        serde_json::from_slice(&std::fs::read(&self.receipt).expect("wire receipt"))
            .expect("wire JSON")
    }
}
