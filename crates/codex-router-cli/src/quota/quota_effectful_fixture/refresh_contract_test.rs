use super::*;
use crate::quota::quota_refresh_adapter::observation::RefreshObservation;
use agent_router_quota_tui::QuotaRefreshResult;
async fn refresh_case(scenario: Scenario) {
    let state = OwnedQuotaState::create(scenario).await.unwrap();
    let (_output, events) = events();
    let effects = RefreshEffects::new(state.root().to_owned(), scenario, events).unwrap();
    let refresher = effects.refresher.clone();
    let mut task = tokio::spawn(async move { refresher().await });
    tokio::select! {
        _ = effects.wait_until_held() => (),
        result = &mut task => panic!("actual refresh returned before its causal gate: {result:?}"),
    }
    assert_eq!(effects.readback()["serviceCalls"], 1);
    assert!(
        effects.observer.observations().is_empty(),
        "the actual service return cannot be observed before release"
    );
    effects.release().unwrap();
    let result = task.await.unwrap();
    let expected = match scenario {
        Scenario::RefreshAllSuccess => "refreshed",
        Scenario::RefreshPartial => "partial",
        _ => "failed",
    };
    assert_eq!(receipts::refresh_result(&result)["kind"], expected);
    let events = effects.observer.observations();
    assert!(
        matches!(events.last(),Some(RefreshObservation::MappingReturned(mapped)) if mapped==&result)
    );
    match scenario {
        Scenario::RefreshAllSuccess => assert_eq!(result, QuotaRefreshResult::Refreshed),
        Scenario::RefreshPartial => assert!(
            matches!(result,QuotaRefreshResult::Partial {ref failed} if failed.len()==1&&failed[0].account_id.as_str()=="effect-account-1")
        ),
        Scenario::RefreshAllFailed => assert!(
            matches!(result,QuotaRefreshResult::Failed {ref failed,overall:None} if failed.len()==2)
        ),
        _ => assert!(
            matches!(result,QuotaRefreshResult::Failed {ref failed,overall:Some(_)} if failed.is_empty())
        ),
    }
    let live = effects.readback();
    let receipt = effects.finish().await.unwrap();
    assert_eq!(
        receipt["observations"], live["observations"],
        "live and final readbacks share the actual observer facts"
    );
    assert_eq!(receipt["serviceCalls"], 1);
    assert_eq!(receipt["released"], true);
    let requests = receipt["requests"].as_array().unwrap();
    assert_eq!(
        requests.len(),
        match scenario {
            Scenario::RefreshAllSuccess => 8,
            Scenario::RefreshPartial => 6,
            Scenario::RefreshAllFailed => 4,
            _ => 0,
        }
    );
    assert!(requests.iter().all(|request| request["method"] == "GET"));
    if scenario == Scenario::RefreshAllSuccess {
        let model = state.model(120).await.unwrap();
        assert!(
            model
                .rows
                .iter()
                .all(|row| row.weekly_window.contains("9%")),
            "actual fresh report projects the loopback weekly quota: {model:?}"
        );
    }
}
#[tokio::test]
async fn refresh_all_success_fixture_runs_real_http_service_and_mapper() {
    refresh_case(Scenario::RefreshAllSuccess).await;
}
#[tokio::test]
async fn refresh_partial_fixture_keeps_actual_service_ok_and_failed_account() {
    refresh_case(Scenario::RefreshPartial).await;
}
#[tokio::test]
async fn refresh_all_failed_fixture_keeps_actual_all_accounts_failed_list() {
    refresh_case(Scenario::RefreshAllFailed).await;
}
#[tokio::test]
async fn refresh_setup_failed_fixture_uses_actual_factory_error_without_attempted_accounts() {
    refresh_case(Scenario::RefreshSetupFailed).await;
}
