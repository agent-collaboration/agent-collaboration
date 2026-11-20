use super::*;
use collaboration_protocol::{ProviderSettingName, ProviderSettingsSetRequest};
use provider_session_machines::{
    session_lifecycle_answers::state_answer,
    session_lifecycle_machine::SessionLifecycle,
    session_refusal_answers::{session_busy_answer, settings_hold_answer},
    session_setting_machine::UnresolvedCause,
};

fn mode_request(rig: &RestoreRig) -> ProviderSettingsSetRequest {
    ProviderSettingsSetRequest {
        target: rig.target.clone(),
        actor: rig.target.clone().into(),
        setting: ProviderSettingName::Mode,
        value: "plan".into(),
    }
}

async fn lifecycle_setting_refusal(
    rig: &RestoreRig,
    host: &ExternalProviderSupervisor,
    exact: &str,
) {
    let snapshot = host
        .owner_session_snapshot(&rig.target)
        .expect("owner snapshot");
    let failure = host
        .settings_set(mode_request(rig))
        .await
        .expect_err("setting is refused by lifecycle");
    assert_eq!(failure.message, exact);
    assert_eq!(failure.message, state_answer(&snapshot.lifecycle));
}

#[tokio::test]
async fn not_loaded_and_loading_settings_use_the_owner_lifecycle_answer() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("silent-first-restore").await;
    // Discover the saved session's owner without sending anything to the agent.
    let _owner = host
        .session_owner_for(&rig.target)
        .expect("saved session owner");
    lifecycle_setting_refusal(&rig, &host, "session is not loaded; load it or resume it").await;
    let load = OperationId::generate();
    host.load(rig.load_request(load.clone()))
        .await
        .expect("load admitted");
    rig.until_wire_request("restores").await;
    lifecycle_setting_refusal(&rig, &host, "session is loading; wait for it or close it").await;
    let deadline = rig
        .clock
        .monotonic_now()
        .after(std::time::Duration::from_secs(30));
    rig.clock.not_due_check_now(deadline).await;
    rig.clock
        .advance(std::time::Duration::from_secs(30))
        .expect("retire the silent load");
    let _retired = host.wait(wait_request(load)).await;
    host.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn closing_settings_use_the_owner_lifecycle_answer() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig
        .open_host_with(&["advertise-close", "hold-close-refusal"])
        .await;
    rig.assert_loads(&host).await;
    let close = OperationId::generate();
    host.close(ConversationCloseRequest {
        operation_id: close.clone(),
        target: rig.target.clone(),
        generation: Some(generation()),
        requested_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
    })
    .await
    .expect("close admitted");
    rig.until_wire_request("closes").await;
    let snapshot = host
        .owner_session_snapshot(&rig.target)
        .expect("closing snapshot");
    assert!(matches!(
        snapshot.lifecycle,
        SessionLifecycle::Closing { .. }
    ));
    lifecycle_setting_refusal(&rig, &host, "session is closing; wait for it").await;
    node_test_fixture::release_fixture(&rig.root.path().join("wire.json"), ".release-close")
        .expect("release close");
    let _settled = host.wait(wait_request(close)).await;
    host.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn busy_without_steering_and_explicit_steer_derive_the_running_answer() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("silent-prompts").await;
    rig.assert_loads(&host).await;
    host.prompt(rig.prompt_request(OperationId::generate(), "held foreground"))
        .await
        .expect("prompt admitted");
    rig.until_wire_request("prompts").await;
    let snapshot = host
        .owner_session_snapshot(&rig.target)
        .expect("running snapshot");
    let busy = session_busy_answer(&snapshot.lifecycle, snapshot.turn, false);

    let setting = host
        .settings_set(mode_request(&rig))
        .await
        .expect_err("prompt in progress blocks a setting");
    assert_eq!(
        setting.message,
        "a turn is running; queue the message or cancel the turn"
    );
    assert_eq!(setting.message, busy);

    let steer = OperationId::generate();
    let mut request = rig.prompt_request(steer.clone(), "explicit steer");
    request.delivery = MessageDelivery::Steer;
    let failure = match host.prompt(request).await {
        Ok(_) => host
            .wait(wait_request(steer))
            .await
            .expect_err("explicit steer is refused"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.kind,
        ConversationOperationFailureKind::UnsupportedCapability
    );
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    let answer = String::from(failure.message);
    assert_eq!(
        answer,
        "this harness can't take a steer mid-turn; a turn is running; queue the message or cancel the turn"
    );
    assert_eq!(
        answer.strip_prefix("this harness can't take a steer mid-turn; "),
        Some(busy.as_str())
    );
    host.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_prompt_on_a_saved_unloaded_session_keeps_its_kind_and_recovery_guidance() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("plain-prompts").await;
    let refusal = host
        .prompt(rig.prompt_request(OperationId::generate(), "prompt while unloaded"))
        .await
        .expect_err("the explicit prompt never loads the session");
    assert_eq!(refusal.kind, ConversationOperationFailureKind::Unavailable);
    assert_eq!(refusal.effect, ProviderOperationEffect::None);
    assert_eq!(
        String::from(refusal.message),
        "session is not loaded; load it or resume it"
    );
    assert!(!rig.root.path().join("wire.json").exists());
    host.shutdown().await.expect("fixture shutdown");
}

#[tokio::test]
async fn an_unknown_setting_uses_its_derived_prompt_hold_answer() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("malformed-mode-reply").await;
    rig.assert_loads(&host).await;
    let setting = host
        .settings_set(mode_request(&rig))
        .await
        .expect_err("mode result is unknown");
    assert_eq!(
        setting.kind,
        collaboration_protocol::ProviderSettingsFailureKind::OutcomeUnknown
    );
    let snapshot = host
        .owner_session_snapshot(&rig.target)
        .expect("held snapshot");
    assert!(snapshot.settings_block_input);
    let prompt = OperationId::generate();
    let failure = match host
        .prompt(rig.prompt_request(prompt.clone(), "prompt while held"))
        .await
    {
        Ok(_) => host
            .wait(wait_request(prompt))
            .await
            .expect_err("setting holds input"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.kind,
        ConversationOperationFailureKind::SettingsUnresolved
    );
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    let answer = String::from(failure.message);
    assert_eq!(
        answer,
        "a setting change's outcome is unknown; set a value explicitly"
    );
    assert_eq!(
        answer,
        settings_hold_answer(UnresolvedCause::OutcomeUnknown)
    );
    host.shutdown().await.expect("shutdown");
}
