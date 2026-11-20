//! Real ACP decoding/projection retains metadata without making it Codex prose.

use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use acp_client_runtime::{ExternalProviderLaunch, ProviderPersistenceTarget};
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor,
};
use collaboration_contracts::{ProviderConversationBackend, SessionEventHub};
use collaboration_protocol::{
    ConversationLoadRequest, ConversationOperationWaitRequest, ConversationPromptRequest,
    MessageContent, MessageDelivery, MessageText, OperationId, PositiveSeconds,
    ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::{
    HistoricalTurn, ProviderOperationStore, ProviderSessionEventHub, ProviderSessionRecord,
    render_historical_turn,
};
use serde_json::{Value, json};
use session_event_model::{SessionEvent, SessionItemKind, StopReason, TurnOutcome};
use tokio::sync::Mutex;

const METADATA_PROVIDER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-metadata-agent.ts"
);

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
async fn check_metadata_projection(replay: bool) {
    let root = tempfile::tempdir().expect("provider working directory");
    let mut target = super::provider_acp_message_fifo_support::target();
    target.session_id = SessionId::try_from("metadata-session".to_owned()).expect("provider ID");
    let binding = super::provider_acp_message_fifo_support::provider_binding(&target);
    let cwd =
        ProviderWorkingDirectory::try_from(root.path().display().to_string()).expect("directory");
    let policy = ProviderRequestedPolicy {
        access: RouterAccess::WriteRestricted,
    };
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("fixture store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: cwd.clone(),
            requested_policy: policy.clone(),
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 0,
            resume: Default::default(),
        })
        .await
        .expect("fixture setup");
    let hub = Arc::new(ProviderSessionEventHub::new(store.clone()));
    let board: message_board::SessionRef =
        collaboration_protocol::ProviderIdentity::session_to_board(&target).expect("board target");
    let mut attached = hub
        .attach(board.clone())
        .await
        .expect("real event subscriber");
    let runtime = ExternalProviderRuntime::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![METADATA_PROVIDER.into()],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("ACP initializes");
    let supervisor = ExternalProviderSupervisor::with_fixture_hub(
        vec![ExternalProviderBinding {
            identity: binding,
            runtime,
        }],
        store,
        hub.clone(),
    )
    .expect("real Host owner");
    let operation_id = OperationId::generate();
    if replay {
        supervisor
            .load(ConversationLoadRequest {
                operation_id: operation_id.clone(),
                target: target.clone(),
                generation: None,
                working_directory: cwd,
                requested_by: target.clone().into(),
                approver: target.clone().into(),
                requested_policy: policy,
            })
            .await
            .expect("admit replay");
    } else {
        let opened = supervisor
            .open_provider_fixture(&target.endpoint, root.path().to_owned())
            .await
            .expect("session opens");
        assert_eq!(opened, String::from(target.session_id.clone()));
        supervisor
            .prompt(ConversationPromptRequest {
                delivery: MessageDelivery::Auto,
                input_id: None,
                operation_id: operation_id.clone(),
                target: target.clone(),
                generation: None,
                requested_by: target.clone().into(),
                approver: target.into(),
                prompt: MessageContent::HumanUser {
                    text: MessageText::try_from("Reply".to_owned()).expect("prompt"),
                },
            })
            .await
            .expect("admit live prompt");
    }
    supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await
        .expect("owner operation settles");

    let mut items = Vec::new();
    let mut completed_items = Vec::new();
    let mut buffered = VecDeque::from(std::mem::take(&mut attached.snapshot));
    loop {
        let observed = match buffered.pop_front() {
            Some(observed) => observed,
            None => attached
                .receiver
                .recv()
                .await
                .expect("hub event until resync"),
        };
        match observed.event {
            SessionEvent::ResyncRequired { replay_epoch } => {
                attached = hub
                    .attach(board.clone())
                    .await
                    .expect("attach to the current epoch");
                assert!(attached.epoch >= replay_epoch);
                buffered = VecDeque::from(std::mem::take(&mut attached.snapshot));
                items.clear();
                completed_items.clear();
            }
            SessionEvent::ItemStarted { item } => items.push(item),
            SessionEvent::ItemCompleted { item_id } => completed_items.push(item_id),
            SessionEvent::TurnEnded { .. } if !replay => break,
            SessionEvent::StateChanged {
                state: session_event_model::SessionState::Idle,
            } if replay => break,
            _ => {}
        }
    }
    supervisor.shutdown().await.expect("stop Host");

    // Assert retention separately: suppressing the producer would fail these checks.
    let usage = items
        .iter()
        .find(|item| item.kind == SessionItemKind::Usage)
        .expect("usage retained");
    assert_eq!(
        serde_json::from_str::<Value>(usage.text.as_deref().expect("usage body"))
            .expect("usage JSON"),
        json!({"used":1234,"size":200000,"cost":{"amount":0.25,"currency":"USD"},
            "_meta":{"providerDetail":"usage-retained"}})
    );
    let commands = items
        .iter()
        .find(|item| item.kind == SessionItemKind::Notice)
        .expect("command catalogue retained");
    assert_eq!(
        serde_json::from_str::<Value>(commands.text.as_deref().expect("commands body"))
            .expect("commands JSON"),
        json!({"availableCommands":[{"name":"compact","description":"Compact context",
            "input":{"hint":"instructions"}}],"_meta":{"providerDetail":"retained"}})
    );
    assert_eq!(items.len(), if replay { 7 } else { 8 });
    let submitted = items
        .iter()
        .filter(|item| item.kind == SessionItemKind::UserMessage)
        .collect::<Vec<_>>();
    assert_eq!(submitted.len(), usize::from(!replay));
    let mut expected_items = Vec::new();
    if let Some(submitted) = submitted.first() {
        assert_eq!(submitted.text.as_deref(), Some("Reply"));
        assert_eq!(
            uuid::Uuid::parse_str(&submitted.item_id)
                .expect("submitted UUID")
                .get_version_num(),
            7
        );
        assert_eq!(
            completed_items
                .iter()
                .filter(|item_id| *item_id == &submitted.item_id)
                .count(),
            1
        );
        expected_items.push(json!({"type":"userMessage","id":submitted.item_id,
            "content":[{"type":"text","text":"Reply"}]}));
    }
    let assistant_id = items
        .iter()
        .find(|item| item.kind == SessionItemKind::AgentMessage)
        .expect("assistant item")
        .item_id
        .clone();
    let thought_id = items
        .iter()
        .find(|item| item.kind == SessionItemKind::AgentThought)
        .expect("reasoning item")
        .item_id
        .clone();

    // Assert consumer presentation: a raw metadata agentMessage fails this literal oracle.
    let rendered = render_historical_turn(&HistoricalTurn {
        turn_id: "observed-turn".into(),
        items,
        outcome: TurnOutcome::Ended {
            stop_reason: StopReason::EndTurn,
            local_cause: None,
        },
    });
    expected_items.extend([
        json!({"type":"agentMessage","id":assistant_id,"text":"Ordinary assistant reply"}),
        json!({"type":"reasoning","id":thought_id,"summary":["Useful reasoning"],"content":[]}),
    ]);
    assert_eq!(rendered.get("items"), Some(&Value::Array(expected_items)));
}

#[tokio::test]
async fn live_acp_metadata_is_retained_without_assistant_prose() {
    check_metadata_projection(false).await;
}

#[tokio::test]
async fn replayed_acp_metadata_is_retained_without_assistant_prose() {
    check_metadata_projection(true).await;
}
