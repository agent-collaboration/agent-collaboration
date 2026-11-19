use super::actor_exit_handoff::RunCheckingSink;
use super::{NoInteraction, ScriptedLineAgent};
use acp_client_runtime::{
    AgentSessionClient, ExternalProviderRuntimeError, ProviderSettingRequest, ProviderTurnInput,
};
use provider_session_machines::ProviderSettingKind;
use router_clock::{ClockInstant, ManualClock};
use session_event_model::{InputId, PromptContent, ProviderSubmission, SettingApplicationIntent};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn a_silent_restore_fast_off_uses_the_injected_clock_before_input() {
    // Arrange a session with a confirmed fast-off preference before its actor exits.
    let clock = Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH));
    let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
    let (agent, agent_task) = ScriptedLineAgent::start(agent_end);
    agent.offer_fast_on_admission();
    let sink = Arc::new(RunCheckingSink::default());
    let client = AgentSessionClient::initialize_in_process(
        client_end,
        agent_task,
        Arc::new(NoInteraction),
        sink.clone(),
        Arc::new(crate::test_behavior::TestProviderBehavior),
        clock.clone(),
    )
    .await
    .expect("initialize ACP");
    let cwd = PathBuf::from("/scenario");
    let session = client
        .create_session(cwd.clone())
        .await
        .expect("create session");
    client
        .set_setting(
            session.clone(),
            ProviderSettingRequest {
                kind: ProviderSettingKind::Fast,
                value: "off".to_owned(),
                intent: SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
        .expect("confirm fast off");
    client
        .end_session_actor(session.clone())
        .await
        .expect("end the live actor");
    sink.unloaded_offered().await;

    // Restore reads fast on. The provider holds the reapply answer while input arrives.
    agent.hold_setting_answers();
    agent.hold_next_restore_answer();
    let load = client.load_session(session.clone(), cwd);
    tokio::pin!(load);
    tokio::select! {
        () = agent.received("session/load") => {},
        result = &mut load => panic!("load answered before the wire barrier: {result:?}"),
    }
    let offset = Duration::from_secs(1);
    clock
        .advance(offset)
        .expect("separate restore and setting deadlines");
    agent.release_load_answers();
    load.await.expect("restore is admitted");
    agent.fast_off_requests_received(2).await;
    let setting_wait = Duration::from_secs(30);
    assert!(
        clock.has_not_due_check_now(ClockInstant::ZERO.after(offset + setting_wait)),
        "the fast-off response wait must register its deadline on the injected clock"
    );
    let prompt = ProviderTurnInput::Prompt(ProviderSubmission::plain(vec![
        PromptContent::text("wait for fast off".to_owned()).expect("prompt text"),
    ]));
    let input = client.submit_with_approval_dispatch_for_input(
        session,
        InputId::generate(),
        prompt,
        (),
        None,
    );
    tokio::pin!(input);
    assert!(futures_util::poll!(&mut input).is_pending());
    assert!(agent.prompt_sessions().is_empty());
    clock
        .advance(setting_wait)
        .expect("expire the fast-off wait");

    // Silence settles the wait and refuses input under the existing settings rule.
    let failure = input.await.expect_err("unconfirmed fast off holds input");
    assert!(
        matches!(
            failure,
            ExternalProviderRuntimeError::SettingsUnresolved { .. }
        ),
        "{failure:?}"
    );
    assert!(
        agent.prompt_sessions().is_empty(),
        "input reached the ACP wire"
    );
    agent.release_setting_answers();
    client.shutdown().await;
}
