//! Protocol-fault peers stand in only for replies the real server cannot intentionally emit.
use crate::picker_data_query::{SessionsPickerDataQuery, SessionsPickerRoot};
use crate::session_picker_record_loader::session_picker_record_loader;
use crate::sessions::router_connection_registry;
use crate::sessions::{
    SessionsProvider, SessionsSort, SessionsSource, discover_repository_identity,
};
use crate::{
    source_inventory_request::PickerSourceContext, source_inventory_request::SourceInventoryRequest,
};
use axum::{Json, Router, extract::State, routing::post};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[path = "native_partition_fixture.rs"]
pub mod native_partitions;

pub(crate) const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";

#[derive(Clone, Copy)]
pub enum ReplyScenario {
    NativePartition(native_partitions::NativeScenario),
    ChangedEpoch,
    WrongRequestId,
    WrongPageEndpoint,
    RepeatedCursor,
    ProviderRows,
    HoldPage,
    HoldInventory,
    RepeatedPartialThenFault,
    HoldSecondPage,
    HoldSecondWrongEndpoint,
    HoldSecondChangedEpoch,
    PageBudget { complete: bool },
    RowBudget(usize),
    SparsePages,
    ProviderSkipped,
}

pub(crate) struct PeerState {
    scenario: ReplyScenario,
    service: String,
    epoch: String,
    pub(crate) calls: Mutex<Vec<(String, Value)>>,
    pub(crate) native_started: tokio::sync::Semaphore,
    release_page: tokio::sync::Semaphore,
    held_settled: tokio::sync::Semaphore,
}

pub struct ReadPeer {
    url: String,
    pub(crate) state: Arc<PeerState>,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl ReadPeer {
    pub async fn start(scenario: ReplyScenario) -> Self {
        Self::start_bound(scenario, SERVICE).await
    }

    pub async fn start_bound(scenario: ReplyScenario, service: &str) -> Self {
        Self::start_bound_at(scenario, service, EPOCH).await
    }

    pub async fn start_bound_at(scenario: ReplyScenario, service: &str, epoch: &str) -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(PeerState {
            scenario,
            service: service.to_owned(),
            epoch: epoch.to_owned(),
            calls: Mutex::new(Vec::new()),
            native_started: tokio::sync::Semaphore::new(0),
            release_page: tokio::sync::Semaphore::new(0),
            held_settled: tokio::sync::Semaphore::new(0),
        });
        let router = Router::new()
            .route("/mcp", post(reply))
            .with_state(state.clone());
        let shutdown = CancellationToken::new();
        let cancel = shutdown.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(cancel.cancelled_owned())
                .await
        });
        Self {
            url: format!("http://{address}/mcp"),
            state,
            shutdown,
            task,
        }
    }

    pub fn request(&self) -> SourceInventoryRequest {
        let registry = router_connection_registry::RouterConnectionRegistry::parse(&json!({"version":1,"routers":[{
            "name":"Source peer","connection":{"kind":"remote","serviceId":self.state.service,"mcpUrl":self.url}
        }]}).to_string()).unwrap();
        SourceInventoryRequest {
            source_context: PickerSourceContext::ConfiguredHosted(registry.routers[0].clone()),
            query: SessionsPickerDataQuery {
                root: SessionsPickerRoot::Any,
                provider: SessionsProvider::Any,
                source: SessionsSource::All,
                sort: SessionsSort::Updated,
                search: String::new(),
                include_empty_sessions: false,
            },
            request_generation: 9,
            continuation: None,
        }
    }

    pub fn loader(&self) -> crate::source_inventory_request::SourceInventoryLoader {
        session_picker_record_loader(
            None,
            std::path::PathBuf::new(),
            discover_repository_identity(std::path::Path::new("/tmp")),
            None,
        )
    }

    pub async fn wait_for_held_page(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.state.native_started.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }

    pub async fn release_held_page(&self) {
        self.state.release_page.add_permits(1);
        tokio::time::timeout(Duration::from_secs(2), self.state.held_settled.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }

    pub fn page_calls(&self) -> usize {
        self.state
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == "sessions_list" || method == "provider_sessions_list")
            .count()
    }

    pub fn inventory_calls(&self) -> usize {
        self.state
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == "endpoints_list")
            .count()
    }

    pub fn release_all_held_pages(&self) {
        self.state.release_page.add_permits(4);
    }

    pub async fn finish(self) {
        self.state.release_page.add_permits(4);
        self.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(2), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

struct HeldReplyGuard(Arc<PeerState>);
impl Drop for HeldReplyGuard {
    fn drop(&mut self) {
        self.0.held_settled.add_permits(1);
    }
}

async fn reply(State(state): State<Arc<PeerState>>, Json(request): Json<Value>) -> Json<Value> {
    let method = request
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    let arguments = request.pointer("/params/arguments").unwrap().clone();
    let continuation_request = arguments
        .get("cursor")
        .is_some_and(|cursor| !cursor.is_null());
    let (inventory_calls, native_calls) = {
        let mut calls = state.calls.lock().unwrap();
        calls.push((method.clone(), arguments));
        (
            calls
                .iter()
                .filter(|(name, _)| name == "endpoints_list")
                .count(),
            calls
                .iter()
                .filter(|(name, _)| name == "sessions_list")
                .count(),
        )
    };
    if let ReplyScenario::NativePartition(scenario) = state.scenario {
        return Json(native_partitions::reply(&state, &request, scenario).await);
    }
    let native_endpoint = json!({"serviceId":state.service,"endpointId":"codex-local"});
    let provider_endpoint = json!({"serviceId":state.service,"endpointId":"claude-local"});
    let result = match method.as_str() {
        "endpoints_list" => {
            if matches!(state.scenario, ReplyScenario::HoldInventory) && inventory_calls == 1 {
                let _settled = HeldReplyGuard(Arc::clone(&state));
                state.native_started.add_permits(1);
                state.release_page.acquire().await.unwrap().forget();
            }
            let mut endpoints = vec![
                json!({"endpoint":native_endpoint,"label":"Native","availability":{"state":"unprobed"},
                "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"native.sock","schemaDigest":null,"generation":null}]}),
            ];
            if matches!(
                state.scenario,
                ReplyScenario::ProviderRows | ReplyScenario::ProviderSkipped
            ) {
                endpoints.push(json!({"endpoint":provider_endpoint,"label":"Source provider","availability":{"state":"unprobed"},
                    "channels":[{"kind":"externalProvider","transport":"stdioAcp","bindingId":"00000000-0000-4000-8000-000000000003","bindingGeneration":1,
                        "runtime":{"provider":"claudeCode","runtimeName":"claude"},"capabilities":[{"name":"create","status":"supported","evidence":"advertised"}]}]}));
            }
            let epoch = if (matches!(state.scenario, ReplyScenario::ChangedEpoch)
                && inventory_calls > 1)
                || (matches!(state.scenario, ReplyScenario::HoldSecondChangedEpoch)
                    && native_calls > 1)
            {
                "00000000-0000-4000-8000-000000000004"
            } else {
                state.epoch.as_str()
            };
            json!({"serviceEpoch":epoch,"sequence":1,"endpoints":endpoints})
        }
        "sessions_list" => {
            if matches!(state.scenario, ReplyScenario::RepeatedPartialThenFault)
                && continuation_request
            {
                let _settled = HeldReplyGuard(Arc::clone(&state));
                state.native_started.add_permits(1);
                state.release_page.acquire().await.unwrap().forget();
            }
            if matches!(
                (state.scenario, native_calls),
                (ReplyScenario::HoldPage, 1)
                    | (
                        ReplyScenario::HoldSecondPage
                            | ReplyScenario::HoldSecondWrongEndpoint
                            | ReplyScenario::HoldSecondChangedEpoch,
                        2
                    )
            ) {
                let _settled = HeldReplyGuard(Arc::clone(&state));
                state.native_started.add_permits(1);
                state.release_page.acquire().await.unwrap().forget();
            }
            let endpoint = if (matches!(state.scenario, ReplyScenario::RepeatedPartialThenFault)
                && continuation_request)
                || matches!(state.scenario, ReplyScenario::WrongPageEndpoint)
                || matches!(
                    (state.scenario, native_calls),
                    (ReplyScenario::HoldSecondWrongEndpoint, 2)
                ) {
                json!({"serviceId":SERVICE,"endpointId":"wrong-endpoint"})
            } else {
                native_endpoint.clone()
            };
            let cursor = match state.scenario {
                ReplyScenario::RepeatedPartialThenFault if !continuation_request => {
                    Some("opaque-repeat-source-page".to_owned())
                }
                ReplyScenario::HoldSecondPage
                | ReplyScenario::HoldSecondWrongEndpoint
                | ReplyScenario::HoldSecondChangedEpoch
                    if native_calls == 1 =>
                {
                    Some("opaque-held-second".to_owned())
                }
                ReplyScenario::RepeatedCursor => Some("repeated".to_owned()),
                ReplyScenario::PageBudget { complete } if !complete || native_calls < 65 => {
                    Some(format!("opaque +/=?☀ {native_calls}"))
                }
                ReplyScenario::RowBudget(total) if native_calls * 100 < total => {
                    Some(format!("opaque +/=?☀ {native_calls}"))
                }
                ReplyScenario::SparsePages if native_calls < 3 => {
                    Some(format!("opaque +/=?☀ {native_calls}"))
                }
                _ => None,
            };
            let sessions = if matches!(state.scenario, ReplyScenario::RepeatedPartialThenFault)
                && !continuation_request
            {
                vec![native_row(&native_endpoint, 0)]
            } else if let ReplyScenario::RowBudget(total) = state.scenario {
                let first = (native_calls - 1) * 100;
                (first..total.min(first + 100))
                    .map(|index| native_row(&native_endpoint, index))
                    .collect::<Vec<_>>()
            } else if matches!(
                (state.scenario, native_calls),
                (ReplyScenario::SparsePages, 3)
                    | (
                        ReplyScenario::HoldSecondPage
                            | ReplyScenario::HoldSecondWrongEndpoint
                            | ReplyScenario::HoldSecondChangedEpoch,
                        1
                    )
            ) {
                vec![native_row(&native_endpoint, 0)]
            } else {
                Vec::new()
            };
            json!({"endpoint":endpoint,"observedAt":"2026-10-11T00:00:00Z","sessions":sessions,"generation":null,"nextCursor":cursor})
        }
        "provider_sessions_list" => {
            if request
                .pointer("/params/arguments/source")
                .and_then(Value::as_str)
                == Some("interactive")
            {
                assert_eq!(
                    request
                        .pointer("/params/arguments/source")
                        .and_then(Value::as_str),
                    Some("interactive")
                );
                json!({"endpoint":provider_endpoint,"observedAt":"2026-10-11T00:00:00Z","nextCursor":null,"sessions":[],"nativeCoverage":{"peer":{"kind":if request.pointer("/params/arguments/view").and_then(Value::as_str)==Some("stored") {"notRequested"}else {"complete"}},"background":{"kind":"complete"}}})
            } else {
                json!({"skippedRecords":if matches!(state.scenario,ReplyScenario::ProviderSkipped){1}else{0},"endpoint":provider_endpoint,"observedAt":"2026-10-11T00:00:00Z","nextCursor":null,
            "sessions":[{"target":{"endpoint":provider_endpoint,"sessionId":"provider-only-id"},"workingDirectory":"/source/provider",
                "updatedAt":2000,"state":"idle","requestedPolicy":{"access":"write-restricted"},"approver":{"endpoint":native_endpoint,"sessionId":"fixture-owner"},
                "createdBy":{"endpoint":native_endpoint,"sessionId":"fixture-owner"}}]})
            }
        }
        _ => panic!("unexpected read method"),
    };
    let id = if matches!(state.scenario, ReplyScenario::WrongRequestId) {
        json!(99)
    } else {
        request["id"].clone()
    };
    Json(
        json!({"jsonrpc":"2.0","id":id,"result":{"content":[],"structuredContent":result,"isError":false}}),
    )
}

fn native_row(endpoint: &Value, index: usize) -> Value {
    json!({"target":{"endpoint":endpoint,"sessionId":format!("source-{index}")},"name":null,"title":"Literal source title","source":"interactive",
        "workingDirectory":"/source/catalog","observation":{"kind":"stored","updatedAt":"2026-10-11T00:00:00Z"},"model":"literal-source-model","gitBranch":null,"idleSeconds":0})
}
