//! Test-only facts copied where the real service return enters the screen mapper.
use super::*;
use codex_router_quota_service::quota::QuotaRefreshReport;
use std::{future::Future, sync::Mutex};

#[derive(Clone, Debug)]
pub(crate) enum ServiceReturn {
    Ok(QuotaRefreshReport),
    AllAccountsFailed(Vec<AccountRefreshOutcome>),
    Error { message: String },
}

impl ServiceReturn {
    pub(crate) fn account_outcomes(&self) -> &[AccountRefreshOutcome] {
        match self {
            Self::Ok(report) => &report.account_outcomes,
            Self::AllAccountsFailed(outcomes) => outcomes,
            Self::Error { .. } => &[],
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum RefreshObservation {
    ServiceReturned(ServiceReturn),
    MappingConsumed(ServiceReturn),
    SetupFailed { message: String },
    MappingReturned(QuotaRefreshResult),
}

#[derive(Clone, Default)]
pub(crate) struct RefreshObserver(Arc<Mutex<Vec<RefreshObservation>>>);

tokio::task_local! {
    static ACTIVE_OBSERVER: RefreshObserver;
}

impl RefreshObserver {
    pub(crate) async fn scope<T>(&self, future: impl Future<Output = T>) -> T {
        ACTIVE_OBSERVER.scope(self.clone(), future).await
    }

    pub(crate) fn observations(&self) -> Vec<RefreshObservation> {
        self.0.lock().unwrap().clone()
    }
}

fn record(observation: RefreshObservation) {
    let _ = ACTIVE_OBSERVER.try_with(|observer| {
        observer.0.lock().unwrap().push(observation);
    });
}

pub(crate) fn service_ok(report: &QuotaRefreshReport) {
    record(RefreshObservation::ServiceReturned(ServiceReturn::Ok(
        report.clone(),
    )));
}

pub(crate) fn mapping_ok(report: &QuotaRefreshReport) {
    record(RefreshObservation::MappingConsumed(ServiceReturn::Ok(
        report.clone(),
    )));
}

pub(crate) fn all_failed(outcomes: &[AccountRefreshOutcome]) {
    record(RefreshObservation::ServiceReturned(
        ServiceReturn::AllAccountsFailed(outcomes.to_vec()),
    ));
}

pub(crate) fn mapping_all_failed(outcomes: &[AccountRefreshOutcome]) {
    record(RefreshObservation::MappingConsumed(
        ServiceReturn::AllAccountsFailed(outcomes.to_vec()),
    ));
}

pub(crate) fn service_error(error: &impl std::fmt::Display) {
    let message = error.to_string();
    record(RefreshObservation::ServiceReturned(ServiceReturn::Error {
        message: message.clone(),
    }));
    record(RefreshObservation::MappingConsumed(ServiceReturn::Error {
        message,
    }));
}

pub(crate) fn setup_error(error: &impl std::fmt::Display) {
    record(RefreshObservation::SetupFailed {
        message: error.to_string(),
    });
}

pub(crate) fn observe_refresher(
    inner: QuotaRefresher,
    observer: RefreshObserver,
) -> QuotaRefresher {
    Arc::new(move || {
        let inner = inner.clone();
        let observer = observer.clone();
        Box::pin(async move {
            observer
                .scope(async move {
                    let result = inner().await;
                    record(RefreshObservation::MappingReturned(result.clone()));
                    result
                })
                .await
        })
    })
}
