use super::*;
use agent_router_quota_tui::{
    QuotaAccountAction, QuotaAccountCommand, QuotaAccountCommandResult, QuotaAccountRefusal,
};

#[tokio::test]
async fn captured_actions_fixture_executes_copy_disable_enable_on_actual_a_only() {
    let state = OwnedQuotaState::create(Scenario::CapturedActions)
        .await
        .unwrap();
    let policies_before = read_policy_facts(state.root()).await.unwrap();
    let (_output, events) = events();
    let effects = AccountEffects::default();
    let commands = effects.commands(state.root().to_owned(), Scenario::CapturedActions, events);
    for action in [
        QuotaAccountAction::CopyId,
        QuotaAccountAction::Disable,
        QuotaAccountAction::Enable,
    ] {
        assert_eq!(
            commands(QuotaAccountCommand {
                target: state.target.clone(),
                action
            })
            .await,
            QuotaAccountCommandResult::Succeeded
        );
        let actual = read_accounts(state.root()).await.unwrap();
        assert_eq!(
            actual[1].status, "enabled",
            "the separately selectable B is not changed"
        );
        assert_eq!(
            actual[0].status,
            if action == QuotaAccountAction::Disable {
                "disabled"
            } else {
                "enabled"
            }
        );
    }
    assert_eq!(
        *effects.clipboard.lock().unwrap(),
        vec![state.target.account_id.as_str()]
    );
    assert_eq!(effects.records.lock().unwrap().len(), 3);
    assert_eq!(
        read_policy_facts(state.root()).await.unwrap(),
        policies_before,
        "copy/status effects cannot hide unrelated persisted policy writes"
    );
}

async fn remove_case(scenario: Scenario) {
    let state = OwnedQuotaState::create(scenario).await.unwrap();
    let before = state.model(120).await.unwrap();
    assert_eq!(
        before.rows[scenario.captured_position()].account_id,
        state.target.account_id
    );
    let (_output, events) = events();
    let effects = AccountEffects::default();
    let commands = effects.commands(state.root().to_owned(), scenario, events);
    assert_eq!(
        commands(QuotaAccountCommand {
            target: state.target.clone(),
            action: QuotaAccountAction::Remove
        })
        .await,
        QuotaAccountCommandResult::Succeeded
    );
    let after = read_accounts(state.root()).await.unwrap();
    assert_eq!(after.len(), scenario.account_count() - 1);
    assert!(
        after
            .iter()
            .all(|account| account.account_id != state.target.account_id.as_str())
    );
    assert!(effects.clipboard.lock().unwrap().is_empty());
    assert_eq!(effects.records.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn remove_middle_fixture_uses_real_removal_owner() {
    remove_case(Scenario::RemoveMiddle).await;
}
#[tokio::test]
async fn remove_last_fixture_uses_real_removal_owner() {
    remove_case(Scenario::RemoveLast).await;
}
#[tokio::test]
async fn remove_only_fixture_uses_real_removal_owner() {
    remove_case(Scenario::RemoveOnly).await;
}

#[tokio::test]
async fn remove_held_fixture_blocks_at_real_commit_seam_until_release() {
    let state = OwnedQuotaState::create(Scenario::RemoveHeld).await.unwrap();
    let (_output, events) = events();
    let effects = AccountEffects::default();
    let commands = effects.commands(state.root().to_owned(), Scenario::RemoveHeld, events);
    let request = QuotaAccountCommand {
        target: state.target.clone(),
        action: QuotaAccountAction::Remove,
    };
    let mut task = tokio::spawn(async move { commands(request).await });
    tokio::select! {
        _ = effects.removal_gate.wait_until_entered() => (),
        result = &mut task => panic!("actual removal owner returned before its causal gate: {result:?}"),
    }
    assert!(
        effects.records.lock().unwrap().is_empty(),
        "no completion exists while the real commit seam is held"
    );
    assert!(
        read_accounts(state.root())
            .await
            .unwrap()
            .iter()
            .any(|account| account.account_id == state.target.account_id.as_str())
    );
    effects.removal_gate.release().unwrap();
    assert_eq!(task.await.unwrap(), QuotaAccountCommandResult::Succeeded);
    assert!(
        read_accounts(state.root())
            .await
            .unwrap()
            .iter()
            .all(|account| account.account_id != state.target.account_id.as_str())
    );
}

#[tokio::test]
async fn remove_requires_disabled_fixture_reenables_before_actual_owner_and_retains_refusal() {
    let state = OwnedQuotaState::create(Scenario::RemoveRequiresDisabled)
        .await
        .unwrap();
    let (_output, events) = events();
    let effects = AccountEffects::default();
    let commands = effects.commands(
        state.root().to_owned(),
        Scenario::RemoveRequiresDisabled,
        events,
    );
    let request = QuotaAccountCommand {
        target: state.target.clone(),
        action: QuotaAccountAction::Remove,
    };
    let mut task = tokio::spawn(async move { commands(request).await });
    tokio::select! {
        _ = effects.removal_gate.wait_until_entered() => (),
        result = &mut task => panic!("actual removal owner returned before its causal gate: {result:?}"),
    }
    effects
        .reenable_before_owner(
            state.root(),
            Scenario::RemoveRequiresDisabled,
            &state.target.account_id,
        )
        .await
        .unwrap();
    assert_eq!(
        task.await.unwrap(),
        QuotaAccountCommandResult::Refused {
            reason: QuotaAccountRefusal::RequiresDisabled
        }
    );
    let actual = read_accounts(state.root()).await.unwrap();
    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].status, "enabled");
    assert_eq!(effects.records.lock().unwrap().len(), 1);
}

async fn login_seed(scenario: Scenario) {
    use codex_router_quota_service::quota_display_model::QuotaLoginState;
    let state = OwnedQuotaState::create(scenario).await.unwrap();
    let model = state.model(120).await.unwrap();
    let target = model
        .rows
        .iter()
        .find(|row| row.account_id == state.target.account_id)
        .unwrap();
    assert_eq!(target.login_state, QuotaLoginState::Required);
    if scenario == Scenario::LoginSucceeded {
        owned_state::replace_login_generation(state.root(), &state.target.account_id)
            .await
            .unwrap();
    }
    let actual = state.model(120).await.unwrap();
    let target = actual
        .rows
        .iter()
        .find(|row| row.account_id == state.target.account_id)
        .unwrap();
    assert_eq!(
        target.login_state,
        if scenario == Scenario::LoginSucceeded {
            QuotaLoginState::NotRequired
        } else {
            QuotaLoginState::Required
        }
    );
    assert_eq!(
        target.active_credential_generation,
        Some(if scenario == Scenario::LoginSucceeded {
            8
        } else {
            7
        })
    );
    assert_eq!(
        scenario.login_exit(),
        Some(match scenario {
            Scenario::LoginSucceeded => 0,
            Scenario::LoginCancelled130 => 130,
            _ => 7,
        })
    );
}
#[tokio::test]
async fn login_succeeded_fixture_returns_a_genuinely_fresh_available_report() {
    login_seed(Scenario::LoginSucceeded).await;
}
#[tokio::test]
async fn login_cancelled130_fixture_keeps_original_login_required_report() {
    login_seed(Scenario::LoginCancelled130).await;
}
#[tokio::test]
async fn login_failed7_fixture_keeps_original_login_required_report() {
    login_seed(Scenario::LoginFailed7).await;
}

#[tokio::test]
async fn login_fixture_factory_records_real_owner_and_private_clipboard_and_leaves_ordinary_scope_unbound()
 {
    use crate::quota::quota_account_adapter::{
        fixture_factory, interactive_quota_account_commands,
    };
    let state = OwnedQuotaState::create(Scenario::LoginSucceeded)
        .await
        .unwrap();
    let (_output, events) = events();
    let effects = AccountEffects::default();
    let request = |action| QuotaAccountCommand {
        target: state.target.clone(),
        action,
    };
    assert!(fixture_factory::commands_for(state.root()).is_none());
    let ordinary = interactive_quota_account_commands(state.root().to_owned());
    assert_eq!(
        ordinary(request(QuotaAccountAction::Disable)).await,
        QuotaAccountCommandResult::Succeeded
    );
    assert_eq!(
        read_accounts(state.root())
            .await
            .unwrap()
            .iter()
            .find(|row| row.account_id == state.target.account_id.as_str())
            .unwrap()
            .status,
        "disabled"
    );
    assert_eq!(
        ordinary(request(QuotaAccountAction::Enable)).await,
        QuotaAccountCommandResult::Succeeded
    );
    assert!(effects.records.lock().unwrap().is_empty());
    state
        .with_account_commands(
            effects.commands(state.root().to_owned(), Scenario::LoginSucceeded, events),
            async {
                let bound = interactive_quota_account_commands(state.root().to_owned());
                assert_eq!(
                    bound(request(QuotaAccountAction::Disable)).await,
                    QuotaAccountCommandResult::Succeeded
                );
                assert_eq!(
                    effects.records.lock().unwrap().len(),
                    1,
                    "factory must reach the real recorded owner before any clipboard call"
                );
                assert_eq!(
                    read_accounts(state.root())
                        .await
                        .unwrap()
                        .iter()
                        .find(|row| row.account_id == state.target.account_id.as_str())
                        .unwrap()
                        .status,
                    "disabled"
                );
                assert_eq!(
                    bound(request(QuotaAccountAction::CopyId)).await,
                    QuotaAccountCommandResult::Succeeded
                );
            },
        )
        .await
        .unwrap();
    assert_eq!(effects.records.lock().unwrap().len(), 2);
    assert_eq!(
        *effects.clipboard.lock().unwrap(),
        vec![state.target.account_id.as_str()]
    );
    assert!(fixture_factory::commands_for(state.root()).is_none());
    let ordinary_after = interactive_quota_account_commands(state.root().to_owned());
    assert_eq!(
        ordinary_after(request(QuotaAccountAction::Enable)).await,
        QuotaAccountCommandResult::Succeeded
    );
    assert_eq!(
        read_accounts(state.root())
            .await
            .unwrap()
            .iter()
            .find(|row| row.account_id == state.target.account_id.as_str())
            .unwrap()
            .status,
        "enabled"
    );
    assert_eq!(
        effects.records.lock().unwrap().len(),
        2,
        "ordinary scope cannot keep a stale recorder binding"
    );
}

#[tokio::test]
async fn login_fixture_factory_rejects_a_non_fixture_root_before_owner() {
    use crate::quota::quota_account_adapter::interactive_quota_account_commands;
    let state = OwnedQuotaState::create(Scenario::LoginSucceeded)
        .await
        .unwrap();
    let non_fixture_root = tempfile::tempdir().unwrap();
    let (_output, events) = events();
    let effects = AccountEffects::default();
    state
        .with_account_commands(
            effects.commands(state.root().to_owned(), Scenario::LoginSucceeded, events),
            async {
                let guarded =
                    interactive_quota_account_commands(non_fixture_root.path().to_owned());
                let actual = guarded(QuotaAccountCommand {
                    target: state.target.clone(),
                    action: QuotaAccountAction::Disable,
                })
                .await;
                assert_eq!(
                    actual,
                    QuotaAccountCommandResult::Failed {
                        message: "Quota fixture account factory rejected a different root"
                            .to_owned()
                    }
                );
            },
        )
        .await
        .unwrap();
    assert!(
        effects.records.lock().unwrap().is_empty(),
        "wrong root must not reach the bound owner"
    );
    assert!(effects.clipboard.lock().unwrap().is_empty());
    assert!(non_fixture_root.path().read_dir().unwrap().next().is_none());
    assert!(
        read_accounts(state.root())
            .await
            .unwrap()
            .iter()
            .all(|row| row.status == "enabled")
    );
}
