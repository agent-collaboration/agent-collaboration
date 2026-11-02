//! Reopen evidence requires the actual saved facet and a fresh mount; no synthetic remount acceptance.
use super::*;
#[test]
fn grouping_reopen_audit_rejects_missing_or_stale_mount_and_unsaved_or_wrong_facet() {
    let good = json!({"storePath":"/private/tmp/owned/agent-proxy-ui.sqlite","invocations":[{"invocation":1,"initialMount":1,"finalMount":1,"initialFacet":"none","finalFacet":"provider","storedValue":"provider"},{"invocation":2,"initialMount":2,"finalMount":2,"initialFacet":"provider","finalFacet":"status","storedValue":"status"}]});
    super::super::grouping_reopen::validate(&good).unwrap();
    for (invocation, field, value) in [
        (0usize, "storedValue", Value::Null),
        (1, "initialMount", json!(1)),
        (1, "finalMount", json!(3)),
        (1, "initialFacet", json!("none")),
        (1, "storedValue", json!("provider")),
    ] {
        let mut wrong = good.clone();
        wrong["invocations"][invocation][field] = value;
        assert!(
            super::super::grouping_reopen::validate(&wrong).is_err(),
            "{wrong}"
        );
    }
    let mut missing = good;
    missing["invocations"].as_array_mut().unwrap().pop();
    assert!(super::super::grouping_reopen::validate(&missing).is_err());
}

#[tokio::test]
async fn grouping_reopen_seed_uses_same_owned_root_with_both_providers_and_existing_private_child_guard()
 {
    let state = OwnedQuotaState::create(Scenario::GroupingReopen)
        .await
        .unwrap();
    let initial = read_accounts(state.root()).await.unwrap();
    let model = state.model(120).await.unwrap();
    assert_eq!(
        model
            .rows
            .iter()
            .map(|row| row.provider)
            .collect::<std::collections::BTreeSet<_>>(),
        [
            codex_router_core::provider::Provider::Openai,
            codex_router_core::provider::Provider::Claude
        ]
        .into_iter()
        .collect()
    );
    assert!(!state.root().join("agent-proxy-ui.sqlite").exists());
    assert!(Scenario::GroupingReopen.requires_login_standin());
    assert!(!Scenario::GroupingReopen.is_login());
    assert_eq!(Scenario::GroupingReopen.login_exit(), None);
    assert_eq!(
        serde_json::to_value(Scenario::GroupingReopen).unwrap(),
        "grouping-reopen"
    );
    assert_eq!(
        serde_json::to_value(read_accounts(state.root()).await.unwrap()).unwrap(),
        serde_json::to_value(initial).unwrap()
    );
}

#[tokio::test]
async fn grouping_reopen_reader_uses_actual_reopened_store_and_frozen_mounted_choice() {
    use agent_router_quota_tui::{
        QuotaGroupFacet, QuotaGroupPreferences, QuotaUiPreferenceStore,
        test_support::{QuotaFixtureControl, QuotaFixtureOptions, controlled_quota_fixture},
    };
    use futures_util::StreamExt;
    use iocraft::prelude::*;
    let state = OwnedQuotaState::create(Scenario::GroupingReopen)
        .await
        .unwrap();
    let store =
        crate::quota::quota_group_preferences::SqliteQuotaPreferences::open(state.root()).await;
    store
        .save_group_facet(QuotaGroupFacet::Provider)
        .await
        .unwrap();
    store.close().await;
    let reopened =
        crate::quota::quota_group_preferences::SqliteQuotaPreferences::open(state.root()).await;
    let (preferences, writer) = QuotaGroupPreferences::load(Arc::new(reopened.clone())).await;
    assert_eq!(preferences.current_facet(), QuotaGroupFacet::Provider);
    let controller = QuotaFixtureController::default();
    let frozen = {
        let mut mounted = controlled_quota_fixture(
            QuotaFixtureOptions {
                view_model: state.model(76).await.unwrap(),
                ports: QuotaStatusViewOptions {
                    group_preferences: Some(preferences.clone()),
                    ..Default::default()
                },
                width: 76,
                height: 30,
                control: QuotaFixtureControl::Browse,
            },
            QuotaFixtureView {
                controller: controller.clone(),
                ..Default::default()
            },
        );
        let frames = recording_backend::recording_terminal_render_loop(
            &mut mounted.element,
            MockTerminalConfig::with_events(futures_util::stream::pending::<TerminalEvent>()),
        );
        tokio::pin!(frames);
        let published = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let canvas = frames.next().await.unwrap();
                if let Some(published) = controller.last_published() {
                    assert!(canvas.to_string().contains("OpenAI"));
                    break published;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            super::super::grouping_reopen::observed_facet(&published).unwrap(),
            QuotaGroupFacet::Provider
        );
        let wire = receipts::readback(&published);
        assert!(
            wire["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| action["id"] == "quota:group-provider" && action["checked"] == true)
        );
        published
    };
    assert!(
        controller.readback().is_err(),
        "the real mounted element has torn down"
    );
    assert_eq!(
        super::super::grouping_reopen::observed_facet(&frozen).unwrap(),
        QuotaGroupFacet::Provider,
        "the retained fact does not borrow disposed UI state"
    );
    let mut invalid = frozen;
    for action in &mut invalid.actions {
        if action.id.as_str() == "quota:group-status" {
            action.checked = Some(true);
        }
    }
    assert!(
        super::super::grouping_reopen::observed_facet(&invalid).is_err(),
        "two checked grouping choices cannot fabricate a facet"
    );
    preferences.finish();
    drop(preferences);
    tokio::time::timeout(std::time::Duration::from_secs(3), writer.run())
        .await
        .expect("owner terminal writer drain must finish while frozen readbacks remain")
        .unwrap();
    reopened.close().await;
}
