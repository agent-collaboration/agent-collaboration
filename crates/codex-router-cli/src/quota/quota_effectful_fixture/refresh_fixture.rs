//! The existing mapper/service and HTTP provider run against the owned loopback gate.
use super::{
    FixtureResult,
    configuration::Scenario,
    effect_ports::FixtureGate,
    loopback_provider::HeldLoopbackProvider,
    receipts::{self, FixtureEvents},
};
use crate::quota::quota_refresh_adapter::{
    QuotaRefreshRequest, interactive_quota_refresher,
    observation::{RefreshObserver, observe_refresher},
    refresh_quota_for_screen,
};
use agent_router_quota_tui::QuotaRefresher;
use codex_router_quota_service::{
    quota::{
        HttpQuotaRefreshProvider, QuotaRefreshProvider, QuotaRefreshProviderRequest,
        QuotaRefreshProviderResponse, current_unix_seconds,
    },
    quota_service_error::QuotaServiceError,
};
use serde_json::{Value, json};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

struct LoopbackTransport {
    inner: HttpQuotaRefreshProvider,
    base_url: String,
}
impl QuotaRefreshProvider for LoopbackTransport {
    async fn fetch_quota(
        &self,
        request: QuotaRefreshProviderRequest,
    ) -> Result<QuotaRefreshProviderResponse, QuotaServiceError> {
        // This existing dependency boundary changes only synthetic transport destination.
        // The HTTP parser, service persistence and screen result mapper remain real.
        assert_eq!(request.base_url(), "https://chatgpt.com/backend-api");
        self.inner
            .fetch_quota(QuotaRefreshProviderRequest::new_for_provider(
                request.provider(),
                request.account_id().clone(),
                request.account_label(),
                request.route_band(),
                self.base_url.clone(),
                request.access_token().clone(),
                request.chatgpt_account_id(),
            ))
            .await
    }
}

#[derive(Clone)]
pub(super) struct RefreshEffects {
    pub refresher: QuotaRefresher,
    pub observer: RefreshObserver,
    pub calls: Arc<AtomicUsize>,
    calls_while_held: Arc<AtomicUsize>,
    provider: Arc<Mutex<Option<HeldLoopbackProvider>>>,
    first_request: Arc<AtomicBool>,
    first_request_notify: Arc<tokio::sync::Notify>,
    released: Arc<AtomicBool>,
    setup_gate: FixtureGate,
    watchers: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    scenario: Scenario,
}
impl RefreshEffects {
    pub(super) fn new(
        root: PathBuf,
        scenario: Scenario,
        events: FixtureEvents,
    ) -> FixtureResult<Self> {
        let failed = match scenario {
            Scenario::RefreshPartial => vec!["routing-effect-account-1".to_owned()],
            Scenario::RefreshAllFailed => vec![
                "routing-effect-account-0".to_owned(),
                "routing-effect-account-1".to_owned(),
            ],
            _ => vec![],
        };
        let provider = if scenario == Scenario::RefreshSetupFailed {
            None
        } else {
            Some(HeldLoopbackProvider::bind_quota_refresh(&failed)?)
        };
        let transport = provider.as_ref().map(|provider| LoopbackTransport {
            inner: HttpQuotaRefreshProvider::new().unwrap(),
            base_url: format!("http://{}", provider.address()),
        });
        let provider = Arc::new(Mutex::new(provider));
        let transport = Arc::new(transport);
        let observer = RefreshObserver::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let first_request = Arc::new(AtomicBool::new(false));
        let first_request_notify = Arc::new(tokio::sync::Notify::new());
        let released = Arc::new(AtomicBool::new(false));
        let setup_gate = FixtureGate::default();
        let watchers = Arc::new(Mutex::new(Vec::new()));
        let inner: QuotaRefresher = {
            let calls = calls.clone();
            let provider = provider.clone();
            let first_request = first_request.clone();
            let first_request_notify = first_request_notify.clone();
            let setup_gate = setup_gate.clone();
            let watchers = watchers.clone();
            Arc::new(move || {
                let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                let root = root.clone();
                let events = events.clone();
                let transport = transport.clone();
                let setup_gate = setup_gate.clone();
                let provider = provider.clone();
                let first_request = first_request.clone();
                let first_request_notify = first_request_notify.clone();
                let watchers = watchers.clone();
                Box::pin(async move {
                    events
                        .write(json!({"kind":"refresh-owner-started","call":call}))
                        .unwrap();
                    if scenario == Scenario::RefreshSetupFailed {
                        setup_gate
                            .hold(&events, "refresh-setup-held")
                            .await
                            .unwrap();
                        let blocked = root.join("refresh-blocked-root");
                        std::fs::write(&blocked, b"actual credential setup failure").unwrap();
                        return interactive_quota_refresher(
                            blocked,
                            "https://chatgpt.com/backend-api".to_owned(),
                        )()
                        .await;
                    }
                    let observed_events = events.clone();
                    let watcher = tokio::spawn(async move {
                        let request = tokio::task::spawn_blocking(move || {
                            let mut provider = provider.lock().unwrap();
                            provider
                                .as_mut()
                                .unwrap()
                                .wait_for_request_count(1, Duration::from_secs(5))
                                .map(|requests| requests[0].clone())
                        })
                        .await
                        .unwrap()
                        .unwrap();
                        first_request.store(true, Ordering::SeqCst);
                        first_request_notify.notify_waiters();
                        observed_events.write(json!({"kind":"refresh-first-request-held","method":request.method,"path":request.path,"routingAccount":request.routing_account,"responseStatus":request.response_status})).unwrap();
                    });
                    watchers.lock().unwrap().push(watcher);
                    let resolver = crate::credential_runtime::AsyncCliCredentialResolver::open(
                        &root.join("state.sqlite"),
                        &root.join("secrets"),
                    )
                    .await
                    .unwrap();
                    refresh_quota_for_screen(
                        QuotaRefreshRequest {
                            router_root: root,
                            base_url: "https://chatgpt.com/backend-api".to_owned(),
                            observed_unix_seconds: current_unix_seconds(),
                        },
                        &resolver,
                        transport.as_ref().as_ref().unwrap(),
                    )
                    .await
                })
            })
        };
        Ok(Self {
            refresher: observe_refresher(inner, observer.clone()),
            observer,
            calls,
            calls_while_held: Arc::new(AtomicUsize::new(0)),
            provider,
            first_request,
            first_request_notify,
            released,
            setup_gate,
            watchers,
            scenario,
        })
    }
    pub(super) async fn wait_until_held(&self) {
        if self.scenario == Scenario::RefreshSetupFailed {
            self.setup_gate.wait_until_entered().await;
            return;
        }
        while !self.first_request.load(Ordering::SeqCst) {
            let notified = self.first_request_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.first_request.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }
    }
    pub(super) fn release(&self) -> FixtureResult<()> {
        self.calls_while_held
            .store(self.calls.load(Ordering::SeqCst), Ordering::SeqCst);
        if self.scenario == Scenario::RefreshSetupFailed {
            self.setup_gate.release()?;
        } else {
            if !self.first_request.load(Ordering::SeqCst) {
                return Err(
                    io::Error::other("refresh release preceded actual first HTTP request").into(),
                );
            }
            self.provider
                .lock()
                .map_err(|_| io::Error::other("fixture provider lock poisoned"))?
                .as_ref()
                .ok_or_else(|| io::Error::other("fixture provider has already settled"))?
                .release_get_responses()?;
        }
        if self.released.swap(true, Ordering::SeqCst) {
            return Err(io::Error::other("refresh released twice").into());
        }
        Ok(())
    }
    pub(super) fn readback(&self) -> Value {
        let observations = self.observer.observations();
        json!({"serviceCalls":self.calls.load(Ordering::SeqCst),"callsWhileHeld":self.calls_while_held.load(Ordering::SeqCst),"released":self.released.load(Ordering::SeqCst),
            "observations":observations.iter().map(receipts::refresh_observation).collect::<Vec<_>>()})
    }
    pub(super) async fn finish(&self) -> FixtureResult<Value> {
        let watchers = std::mem::take(&mut *self.watchers.lock().unwrap());
        for watcher in watchers {
            watcher.await?;
        }
        let requests=match self.provider.lock().unwrap().take() {
            Some(provider)=>provider.finish()?.into_iter().map(|request|json!({"method":request.method,"path":request.path,"routingAccount":request.routing_account,"responseStatus":request.response_status})).collect::<Vec<_>>(),
            None=>vec![],
        };
        let mut receipt = self.readback();
        receipt["requests"] = json!(requests);
        Ok(receipt)
    }
}
