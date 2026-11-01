//! Observer facts must precede and equal the actual mapper's consumed input.
use super::*;
use crate::quota::quota_refresh_adapter::observation::{
    RefreshObservation, RefreshObserver, ServiceReturn, observe_refresher,
};

fn assert_same_return(source: &ServiceReturn, consumed: &ServiceReturn) {
    assert_eq!(
        std::mem::discriminant(source),
        std::mem::discriminant(consumed)
    );
    assert_eq!(source.account_outcomes(), consumed.account_outcomes());
    if let (ServiceReturn::Error { message: source }, ServiceReturn::Error { message: consumed }) =
        (source, consumed)
    {
        assert_eq!(source, consumed);
    }
}

#[tokio::test]
async fn refresh_observer_records_service_return_before_equal_mapping_input() {
    for labels in [
        vec!["healthy-one"],
        vec!["healthy-one", "failed-two"],
        vec!["failed-one", "failed-two"],
    ] {
        let observer = RefreshObserver::default();
        let result = observer.scope(run_adapter_case(&labels)).await;
        let events = observer.observations();
        let [
            RefreshObservation::ServiceReturned(source),
            RefreshObservation::MappingConsumed(consumed),
        ] = events.as_slice()
        else {
            panic!("the raw service return must precede mapping: {events:?}");
        };
        assert_same_return(source, consumed);
        assert_eq!(source.account_outcomes().len(), labels.len());
        match labels.as_slice() {
            ["healthy-one"] => assert_eq!(result, QuotaRefreshResult::Refreshed),
            ["healthy-one", "failed-two"] => {
                assert!(matches!(result, QuotaRefreshResult::Partial { .. }))
            }
            _ => assert!(matches!(
                result,
                QuotaRefreshResult::Failed { overall: None, .. }
            )),
        }
    }
}

#[tokio::test]
async fn refresh_observer_records_actual_factory_setup_error_before_mapping_returns() {
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("not-a-directory");
    std::fs::write(&blocked, b"fixture setup cannot open this as a store").unwrap();
    let observer = RefreshObserver::default();
    let refresher = observe_refresher(
        interactive_quota_refresher(blocked, "https://chatgpt.com/backend-api".to_owned()),
        observer.clone(),
    );
    let result = refresher().await;
    let events = observer.observations();
    let [
        RefreshObservation::SetupFailed { message },
        RefreshObservation::MappingReturned(mapped),
    ] = events.as_slice()
    else {
        panic!("actual setup branch must precede the returned mapping: {events:?}");
    };
    assert_eq!(mapped, &result);
    assert_eq!(
        &QuotaRefreshResult::Failed {
            failed: vec![],
            overall: Some(message.clone())
        },
        mapped
    );
    assert!(!message.is_empty());
}
