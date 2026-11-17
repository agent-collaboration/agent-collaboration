//! The connection loop never waits on one session's mailbox (design note
//! §11.7, M5; Lead D-d): a session whose actor is held never holds up
//! another session's commands.

use crate::{
    AgentSessionClient, EventSinkClosed, ExternalProviderLaunch, HistoryReplayFuture,
    ProviderPersistenceTarget, ProviderSettingRequest, SessionEventSink,
};
use provider_session_machines::ProviderSettingKind;
use session_event_model::InputId;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const STEERING_AGENT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-steering-agent.ts"
);

struct DiscardingSink;

impl SessionEventSink for DiscardingSink {
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
    fn publish(
        &self,
        _session_id: &str,
        _event: session_event_model::SessionEvent,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

/// How many times the agent has read `method`.
fn recorded(receipt: &Path, method: &str) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
        .map_or(0, |frames| {
            frames
                .iter()
                .filter(|frame| frame["method"] == method)
                .count()
        })
}

/// Releases the fixture's `<receipt>.<name>` hold and rings its doorbell.
fn release(receipt: &Path, name: &str) {
    std::fs::write(format!("{}.{name}", receipt.display()), b"").expect("release file");
    let mut doorbell = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(format!("{}.doorbell", receipt.display()))
        .expect("doorbell");
    std::io::Write::write_all(&mut doorbell, b"ring\n").expect("ring");
}

/// Oracle: §11.7 and Lead D-d, "the loop never awaits a single session's
/// mailbox" (wrong build: a bounded mailbox the loop awaits): session A's
/// actor is held while the agent answers its effort change, and A's mailbox
/// is filled past its old bound of 16. Session B's prompt still goes out to
/// the agent while A is held. (The fixture answers one request at a time, so
/// B's answer follows A's release; the agent records each frame as it
/// arrives.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_held_in_a_setting_change_never_holds_up_another_sessions_prompt() {
    // Arrange: two sessions; prompts answer at once.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    release(&receipt, "release");
    let client = Arc::new(
        AgentSessionClient::initialize(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments: vec![
                    STEERING_AGENT.to_owned(),
                    "send-race".to_owned(),
                    receipt.display().to_string(),
                    "hold-effort-change".to_owned(),
                ],
                environment: Vec::new(),
                persistence_target: ProviderPersistenceTarget::Unspecified,
            },
            Arc::new(super::super::turn_admission_tests::NoopInteractionPort),
            Arc::new(DiscardingSink),
            Arc::new(crate::test_behavior::TestProviderBehavior),
        )
        .await
        .expect("fixture initializes"),
    );
    let held = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session A");
    let other = client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session B");
    let setting = tokio::spawn({
        let client = Arc::clone(&client);
        let held = held.clone();
        async move {
            client
                .set_setting(
                    held,
                    ProviderSettingRequest {
                        kind: ProviderSettingKind::Effort,
                        value: "high".to_owned(),
                        intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                        expected_model: None,
                    },
                )
                .await
        }
    });
    while recorded(&receipt, "session/set_config_option") == 0 {
        tokio::task::yield_now().await;
    }
    let inspections = (0..24)
        .map(|_| {
            let client = Arc::clone(&client);
            let held = held.clone();
            tokio::spawn(async move { client.session_activity(held).await })
        })
        .collect::<Vec<_>>();

    // Act
    let text: session_event_model::PromptContent =
        serde_json::from_value(serde_json::json!({"type": "text", "text": "other session"}))
            .expect("text block");
    let prompting = tokio::spawn({
        let client = Arc::clone(&client);
        async move {
            client
                .submit_with_approval_dispatch_for_input(
                    other,
                    InputId::generate(),
                    crate::ProviderTurnInput::Prompt(
                        session_event_model::ProviderSubmission::plain(vec![text]),
                    ),
                    (),
                    None,
                )
                .await
        }
    });
    while recorded(&receipt, "session/prompt") == 0 {
        tokio::task::yield_now().await;
    }
    let still_held = !setting.is_finished();
    release(&receipt, "setting-release");
    let prompted = prompting.await.expect("prompt task");
    let applied = setting.await.expect("setting task");
    for inspection in inspections {
        let _activity = inspection.await.expect("inspection task");
    }
    client.shutdown().await;

    // Assert
    assert!(prompted.is_ok(), "{prompted:?}");
    assert!(
        still_held,
        "session A was still held when B's prompt went out"
    );
    assert!(applied.is_ok(), "{applied:?}");
}
