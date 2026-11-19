//! The connection loop never waits on one session's mailbox (design note
//! §11.7, M5; Lead D-d): a session whose actor is held never holds up
//! another session's commands. All in the simulation: the client, its
//! connection and the in-process agent run on one runtime, so driving it
//! until no task is ready decides each session's progress without a clock.

use super::scripted_line_agent::ScriptedLineAgent;
use super::{DiscardingSink, NoInteraction};
use acp_client_runtime::{AgentSessionClient, ProviderSettingRequest, ProviderTurnInput};
use provider_session_machines::ProviderSettingKind;
use std::sync::Arc;

/// Rounds of yielding that let every task made ready run to its next wait.
/// The longest chain here (a prompt from client to agent and back) is a few
/// dozen wake-ups; the bound is a count of rounds, never time.
const QUIESCENCE_ROUNDS: usize = 1_000;

/// Drives the runtime until every ready task has run to its next wait.
async fn run_until_quiescent() {
    for _ in 0..QUIESCENCE_ROUNDS {
        tokio::task::yield_now().await;
    }
}

/// Oracle: §11.7 and Lead D-d, "the loop never awaits a single session's
/// mailbox" (wrong build: a bounded mailbox the loop awaits): session A's
/// actor is held while the agent holds its effort answer, and A's mailbox
/// is filled past its old bound of 16. Driven until quiescent, session B's
/// prompt has been answered while A is still held.
#[test]
fn a_session_held_in_a_setting_change_never_holds_up_another_sessions_prompt() {
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        // Arrange: two sessions; the agent holds its setting answers.
        let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
        let (agent, agent_task) = ScriptedLineAgent::start(agent_end);
        agent.hold_setting_answers();
        let client = Arc::new(
            AgentSessionClient::initialize_in_process(
                client_end,
                agent_task,
                Arc::new(NoInteraction),
                Arc::new(DiscardingSink),
                Arc::new(crate::test_behavior::TestProviderBehavior),
                Arc::new(router_clock::SystemRuntimeClock::new()),
            )
            .await
            .expect("the connection initializes"),
        );
        let held = client
            .create_session(std::path::PathBuf::from("/scenario"))
            .await
            .expect("session A");
        let other = client
            .create_session(std::path::PathBuf::from("/scenario"))
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
        agent.received("session/set_config_option").await;
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
                        session_event_model::InputId::generate(),
                        ProviderTurnInput::Prompt(session_event_model::ProviderSubmission::plain(
                            vec![text],
                        )),
                        (),
                        None,
                    )
                    .await
            }
        });
        run_until_quiescent().await;
        let answered_while_held = prompting.is_finished();
        let still_held = !setting.is_finished();
        agent.release_setting_answers();
        let prompted = prompting.await.expect("prompt task");
        let applied = setting.await.expect("setting task");
        for inspection in inspections {
            let _activity = inspection.await.expect("inspection task");
        }

        // Assert
        assert!(answered_while_held, "B did not complete while A was held");
        assert!(still_held, "A's setting change was still held");
        assert!(prompted.is_ok(), "{prompted:?}");
        assert!(applied.is_ok(), "{applied:?}");
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}
