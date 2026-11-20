//! The rig the typed extension taps drive: the stand-in interaction port and event sink, Cursor's
//! connection-level discovery behavior, the launched fixture agent with its wire capture, and the
//! rendering of published session events as tap kinds.

#![expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::acp_owner_fixture::FixtureClient as AgentSessionClient;
use acp_client_runtime::{
    ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch, HistoryReplayFuture,
    InteractionFuture, InteractionPort, ModelMetadataSource, ModelReasoning, ModelReasoningEntry,
    ModelReasoningMetadata, ModelReasoningUnknown, ProviderBehavior, ProviderPersistenceTarget,
    ProviderSessionActivity, RefusedApprovalOffer, SessionEventSink,
};
use codex_router_test_support::node_taps::stable;
use session_event_model::{
    ApprovalRequest, PromptContent, ProviderSubmission, SessionEvent, SessionItemKind,
};
use tokio_util::sync::CancellationToken;

use crate::acp_wire_capture::{CapturedFrame, read_capture};

const AGENT_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/typed-extension-agent.ts"
);

pub(super) struct NoopInteractionPort;

impl InteractionPort for NoopInteractionPort {
    type Context = ();
    type OperationId = u64;

    fn operation_id(_context: &Self::Context) -> Self::OperationId {
        1
    }
    fn binding_retirement(_context: &Self::Context) -> CancellationToken {
        CancellationToken::new()
    }
    fn request_approval(
        &self,
        _context: Self::Context,
        _request: ApprovalRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }
    fn request_question(
        &self,
        _context: Self::Context,
        _request: session_event_model::QuestionRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, session_event_model::QuestionResponse> {
        Box::pin(async { session_event_model::QuestionResponse::Cancelled })
    }
    fn record_refusal(
        &self,
        _context: Self::Context,
        _refusal: RefusedApprovalOffer,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_all(
        &self,
        _context: Self::Context,
        _reason: &'static str,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

struct CaptureEventSink(tokio::sync::mpsc::UnboundedSender<SessionEvent>);

impl SessionEventSink for CaptureEventSink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(&self, _session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.0.send(event).map_err(|_| EventSinkClosed)
    }
}

/// The test behavior, plus Cursor's connection-level model discovery. The
/// effort-option decoder belongs to the provider adapters (not a dependency of
/// this crate); this stand-in keeps each model id from the reply and records
/// the raw reply, so the tap sees what the node handed to the decoder.
#[derive(Default)]
pub(super) struct DiscoveryBehavior {
    pub(super) reply_seen: Mutex<Option<serde_json::Value>>,
}

impl ProviderBehavior for DiscoveryBehavior {
    fn initialization_authentication(
        &self,
    ) -> acp_client_runtime::ProviderInitializationAuthentication {
        crate::test_behavior::TestProviderBehavior.initialization_authentication()
    }
    fn can_reuse_setting_confirmation(
        &self,
        kind: provider_session_machines::ProviderSettingKind,
    ) -> bool {
        crate::test_behavior::TestProviderBehavior.can_reuse_setting_confirmation(kind)
    }
    fn model_metadata_source(&self) -> ModelMetadataSource {
        ModelMetadataSource::ConnectionRequest {
            method: "cursor/list_available_models",
        }
    }
    fn decode_model_metadata(&self, value: &serde_json::Value) -> ModelReasoningMetadata {
        *self.reply_seen.lock().expect("reply lock") = Some(value.clone());
        let entries = value
            .get("models")
            .and_then(serde_json::Value::as_array)
            .map(|models| {
                models
                    .iter()
                    .filter_map(|model| model.get("value").and_then(serde_json::Value::as_str))
                    .map(|id| {
                        ModelReasoningEntry::try_new(
                            id.to_owned(),
                            ModelReasoning::Unknown(ModelReasoningUnknown::UnsupportedSource),
                        )
                        .expect("model id")
                    })
                    .collect()
            });
        entries.map_or(
            ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Malformed),
            |per_model_descriptors| ModelReasoningMetadata::Available {
                per_model_descriptors,
            },
        )
    }
    fn auto_permission_mode(&self) -> acp_client_runtime::AutoPermissionMode {
        acp_client_runtime::AutoPermissionMode::UNREALIZED
    }
    fn observe(
        &self,
        update: acp_client_runtime::ProviderUpdate,
    ) -> Result<acp_client_runtime::ProviderFacts, acp_client_runtime::ProviderError> {
        crate::test_behavior::TestProviderBehavior.observe(update)
    }
    fn prepare(
        &self,
        submission: ProviderSubmission,
    ) -> Result<Vec<PromptContent>, acp_client_runtime::ProviderError> {
        crate::test_behavior::TestProviderBehavior.prepare(submission)
    }
    fn compact(
        &self,
    ) -> Result<acp_client_runtime::PreparedCompact, acp_client_runtime::ProviderError> {
        crate::test_behavior::TestProviderBehavior.compact()
    }
}

pub(super) struct Wire {
    root: tempfile::TempDir,
    pub(super) client: AgentSessionClient<NoopInteractionPort>,
    pub(super) events: tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
    pub(super) behavior: Arc<DiscoveryBehavior>,
}

impl Wire {
    pub(super) async fn start(scenario: &str) -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let receipt = root.path().join("wire.jsonl");
        let (sender, events) = tokio::sync::mpsc::unbounded_channel();
        let behavior = Arc::new(DiscoveryBehavior::default());
        let provider_behavior: Arc<dyn ProviderBehavior> = if scenario.starts_with("models-") {
            behavior.clone()
        } else {
            Arc::new(crate::test_behavior::TestProviderBehavior)
        };
        let client = AgentSessionClient::initialize(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments: vec![
                    AGENT_FIXTURE.to_owned(),
                    scenario.to_owned(),
                    receipt.to_string_lossy().into_owned(),
                ],
                environment: Vec::new(),
                persistence_target: ProviderPersistenceTarget::Unspecified,
            },
            Arc::new(NoopInteractionPort),
            Arc::new(CaptureEventSink(sender)),
            provider_behavior,
        )
        .await
        .expect("fixture initializes");
        Self {
            root,
            client,
            events,
            behavior,
        }
    }

    pub(super) async fn open_session(&mut self) {
        self.client
            .create_session(self.root.path().to_path_buf())
            .await
            .expect("session opens");
        let mut settings = false;
        loop {
            match self.events.recv().await.expect("session activation event") {
                SessionEvent::SettingsChanged { .. } => settings = true,
                SessionEvent::StateChanged {
                    state: session_event_model::SessionState::Idle,
                } => break,
                _ => {}
            }
        }
        assert!(settings, "settings must be observed before Idle");
    }

    /// The fixture's complete wire capture, every line both ways in order. Read once the client
    /// has shut down, so the capture holds the whole connection.
    pub(super) fn capture(&self) -> Vec<CapturedFrame> {
        read_capture(&self.root.path().join("wire.jsonl"))
    }
}

pub(super) fn drain_events(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) -> Vec<SessionEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

pub(super) fn submission(text: &str) -> ProviderSubmission {
    ProviderSubmission::plain(vec![
        PromptContent::text(text.to_owned()).expect("text content"),
    ])
}

pub(super) fn activity_text(activity: ProviderSessionActivity) -> String {
    format!("{activity:?}")
}

/// Renders each published event as its tap kind and decoded payload (generated turn and item
/// ids made stable); an item completion carries the kind of the item it completes.
pub(super) fn event_records(events: &[SessionEvent]) -> Vec<(String, serde_json::Value)> {
    let mut started = std::collections::HashMap::new();
    events
        .iter()
        .map(|event| {
            let payload = stable(
                serde_json::to_value(event).expect("event serializes"),
                &["turnId", "itemId"],
            );
            (event_kind(event, &mut started), payload)
        })
        .collect()
}

fn event_kind(
    event: &SessionEvent,
    started: &mut std::collections::HashMap<String, String>,
) -> String {
    match event {
        SessionEvent::TurnStarted { .. } => "TurnStarted".to_owned(),
        SessionEvent::InputAccepted { .. } => "InputAccepted".to_owned(),
        SessionEvent::TurnEnded {
            outcome: session_event_model::TurnOutcome::Lost { reason },
            ..
        } => format!("TurnEnded(Lost:{reason:?})"),
        SessionEvent::TurnEnded { outcome, .. } => format!(
            "TurnEnded({})",
            format!("{outcome:?}")
                .split([' ', '{', '('])
                .next()
                .unwrap_or("?")
        ),
        SessionEvent::ItemStarted { item } => {
            let kind = item_kind_name(&item.kind);
            started.insert(item.item_id.clone(), kind.clone());
            format!("ItemStarted({kind})")
        }
        SessionEvent::ItemCompleted { item_id } => format!(
            "ItemCompleted({})",
            started.get(item_id).map_or("?", String::as_str)
        ),
        other => format!("unexpected:{other:?}"),
    }
}

fn item_kind_name(kind: &SessionItemKind) -> String {
    match kind {
        SessionItemKind::UserMessage => "UserMessage".to_owned(),
        other => format!("{other:?}"),
    }
}
