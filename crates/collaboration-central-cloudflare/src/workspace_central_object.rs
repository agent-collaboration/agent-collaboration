//! Org-scoped central: shared core over a statement-only DO adapter, no Fleet engine.
use crate::{
    object_location::ObjectAddress,
    sign_in_trust::CloudflareSignInTrust,
    statement_store::{self, AlarmPolicy},
    worker_clock::WorkspaceClock,
};
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use collaboration_central_core::WorkspaceRuntime;
use fleet_contracts::{WorkspaceRequest, WorkspaceResponse};
use fleet_identity_protocol::OrganizationId;
use fleet_rpc::{CallFailure, Envelope, Transport};
use fleet_runtime::{ClockIds, FleetRuntime};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use futures_util::lock::Mutex;
use sqlx_durable_object::DoConnection;
use std::sync::Arc;
use worker::{
    DurableObject, Env, Request, Response, Result, State, durable_object,
    send::{SendFuture, SendWrapper},
    wasm_bindgen,
};

struct CentralStatements {
    #[cfg(feature = "test-support")]
    faults: Arc<fleet_runtime::TestFaults>,
    #[cfg(feature = "test-support")]
    zero_next_write_alarm: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(feature = "test-support")]
    witness_env: SendWrapper<Env>,
    connection: SendWrapper<DoConnection>,
    state: Arc<SendWrapper<State>>,
    clock: WorkspaceClock,
    decide_alarm: central_storage::EngineAlarmDecider,
}
impl CentralStore for CentralStatements {
    fn run_batch(
        &self,
        batch: StatementBatch,
    ) -> impl std::future::Future<Output = std::result::Result<BatchOutcome, StoreFailure>> + Send
    {
        SendFuture::new(async move {
            #[cfg(feature = "test-support")]
            let fault = self.faults.take_commit(&batch);
            #[cfg(feature = "test-support")]
            if let Some(result) = fault.as_ref().and_then(|fault| fault.before()) {
                return result;
            }
            #[cfg(feature = "test-support")]
            if let Some(failure) = self.faults.take_alarm_batch_failure(&batch) {
                return Err(failure);
            }
            let decide_alarm = self.decide_alarm.clone();
            #[cfg(feature = "test-support")]
            let decide_alarm = if !batch.statements.is_empty()
                && matches!(
                    batch.alarm,
                    central_storage::AlarmUpdate::SetEngineFromQuery { .. }
                )
                && self
                    .zero_next_write_alarm
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                // The store calls this after the guarded SQL. Workerd rejects epoch zero at setAlarm.
                let zero_alarm: central_storage::EngineAlarmDecider =
                    Arc::new(move |times, now, write| {
                        decide_alarm(times, now, write)?;
                        Ok(Some(0))
                    });
                zero_alarm
            } else {
                decide_alarm
            };
            let result = statement_store::run_batch(
                &self.connection,
                batch.into(),
                AlarmPolicy::Engine(self.state.storage(), self.clock.clone(), decide_alarm),
            )
            .await;
            #[cfg(feature = "test-support")]
            if fault.is_some_and(|fault| {
                matches!(
                    fault.kind,
                    fleet_runtime::EngineCommitFaultKind::AppliedAnswerLost
                )
            }) && matches!(result, Ok(BatchOutcome::Applied { .. }))
            {
                return Err(StoreFailure::AnswerLost);
            }
            result
        })
    }
    fn read(
        &self,
        statement: Statement,
    ) -> impl std::future::Future<Output = std::result::Result<ReadOutcome, StoreFailure>> + Send
    {
        SendFuture::new(async move {
            #[cfg(feature = "test-support")]
            if let Some(failure) = self.faults.take_read_failure(&statement) {
                return Err(failure);
            }
            #[cfg(feature = "test-support")]
            let sql = statement.sql;
            let result = statement_store::read(&self.connection, statement.into()).await;
            #[cfg(feature = "test-support")]
            workspace_link_witness::read_completed(
                &self.witness_env,
                self.state.id().to_string(),
                sql,
                &result,
            )
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
            result
        })
    }
    fn published_alarm(
        &self,
    ) -> impl std::future::Future<
        Output = std::result::Result<Option<chrono::DateTime<chrono::Utc>>, StoreFailure>,
    > + Send {
        SendFuture::new(async move {
            let alarm = self
                .state
                .storage()
                .get_alarm()
                .await
                .map_err(|_| StoreFailure::Unavailable)?;
            alarm
                .map(|millis| {
                    chrono::DateTime::from_timestamp_millis(millis).ok_or_else(|| {
                        StoreFailure::Corrupt {
                            reason: central_storage::CorruptReason::BadValue {
                                column: "alarm.deadline".into(),
                            },
                        }
                    })
                })
                .transpose()
        })
    }
}
#[derive(Clone)]
struct WorkspaceTransport {
    env: Arc<SendWrapper<Env>>,
    #[cfg(feature = "test-support")]
    lose_read: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(feature = "test-support")]
    calls: Arc<workspace_cases::WorkspaceCallCounts>,
}
impl WorkspaceStores for WorkspaceTransport {
    fn call_workspace(
        &self,
        id: &workspace_model::WorkspaceId,
        call: StoreCall,
    ) -> impl std::future::Future<Output = std::result::Result<StoreReply, StoreFailure>> + Send
    {
        let id = id.clone();
        SendFuture::new(async move {
            #[cfg(feature = "test-support")]
            self.calls
                .legacy
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            #[cfg(feature = "test-support")]
            let is_read = matches!(&call, StoreCall::Read(_));
            let answer = crate::cloudflare_transport::call_store(&self.env, id, call.into()).await;
            #[cfg(feature = "test-support")]
            if is_read
                && self
                    .lose_read
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(StoreFailure::AnswerLost);
            }
            answer
        })
    }
}
impl Transport<WorkspaceRequest> for WorkspaceTransport {
    fn exchange(
        &self,
        request: Envelope<workspace_ids::WorkspaceId, WorkspaceRequest>,
    ) -> impl std::future::Future<
        Output = std::result::Result<
            Envelope<workspace_ids::WorkspaceId, WorkspaceResponse>,
            CallFailure,
        >,
    > + Send {
        SendFuture::new(async move {
            #[cfg(feature = "test-support")]
            self.calls
                .rpc
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let answer = crate::cloudflare_transport::workspace_exchange(
                &self.env,
                request.target.clone(),
                "/workspace-rpc",
                &request,
                None,
            )
            .await;
            #[cfg(feature = "test-support")]
            if self
                .lose_read
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(CallFailure::AnswerLost);
            }
            answer
        })
    }
}

type OrgRuntime = WorkspaceRuntime<CentralStatements, WorkspaceTransport>;
enum AlarmFailure {
    Store(StoreFailure),
    Platform(worker::Error),
}
#[durable_object]
pub struct CentralObject {
    state: Arc<SendWrapper<State>>,
    env: Env,
    connection: DoConnection,
    runtime: Mutex<Option<Arc<OrgRuntime>>>,
    clock: WorkspaceClock,
    due_batches: Arc<fleet_runtime::DueBatchState>,
    mail_pushes: Arc<std::sync::Mutex<Vec<fleet_contracts::MailDeliveryReady>>>,
    #[cfg(feature = "test-support")]
    controls: workspace_cases::Controls,
    #[cfg(feature = "test-support")]
    launcher_clock_sampled: Mutex<bool>,
}
impl CentralObject {
    fn check_physical_organization(&self, organization: &OrganizationId) -> Result<()> {
        if self
            .env
            .durable_object("CENTRAL")?
            .id_from_name(organization.as_str())?
            .to_string()
            != self.state.id().to_string()
        {
            return Err(worker::Error::RustError("wrong physical org".to_owned()));
        }
        Ok(())
    }

    fn statement_host(&self) -> Arc<CentralStatements> {
        Arc::new(CentralStatements {
            #[cfg(feature = "test-support")]
            faults: self.controls.faults.clone(),
            #[cfg(feature = "test-support")]
            zero_next_write_alarm: self.controls.zero_next_write_alarm.clone(),
            #[cfg(feature = "test-support")]
            witness_env: SendWrapper::new(self.env.clone()),
            connection: SendWrapper::new(DoConnection::new(self.state.storage())),
            state: self.state.clone(),
            clock: self.clock.clone(),
            decide_alarm: fleet_runtime::engine_alarm_decider(self.due_batches.clone()),
        })
    }
    async fn migrate(&self) -> std::result::Result<(), StoreFailure> {
        statement_store::migrate(
            &self.connection,
            Arc::new(fleet_storage_schema::workspace_migrations::central_migrator()),
        )
        .await
        .map_err(|_| StoreFailure::Unavailable)
    }
    async fn runtime(&self, organization: OrganizationId) -> Result<Arc<OrgRuntime>> {
        self.runtime_checked(organization)
            .await
            .map_err(|error| worker::Error::RustError(error.to_string()))
    }
    async fn runtime_checked(
        &self,
        organization: OrganizationId,
    ) -> std::result::Result<Arc<OrgRuntime>, StoreFailure> {
        let mut initialized = self.runtime.lock().await;
        if let Some(runtime) = initialized.as_ref() {
            return Ok(runtime.clone());
        }
        self.migrate().await?;
        let host = self.statement_host();
        let clock: Arc<dyn fleet_runtime_ports::Clock> = Arc::new(self.clock.clone());
        let runtime = WorkspaceRuntime::new(FleetRuntime::open(
            organization.clone(),
            host,
            WorkspaceTransport {
                env: Arc::new(SendWrapper::new(self.env.clone())),
                #[cfg(feature = "test-support")]
                lose_read: self.controls.lose_read.clone(),
                #[cfg(feature = "test-support")]
                calls: self.controls.workspace_calls.clone(),
            },
            clock.clone(),
            Arc::new(ClockIds::new(self.clock.clone())),
            self.due_batches.clone(),
            Arc::new(crate::signin_observation::observe_signin),
        ));
        let runtime = match crate::organization_signin_link::trust_settings(&self.env) {
            Ok(Some(settings)) => {
                match CloudflareSignInTrust::new(settings).for_organization(organization, clock) {
                    Ok(check) => runtime.with_sign_in_verification(check),
                    Err(
                        fleet_contracts::FleetOrganizationStatementRefusal::InvalidConfiguration,
                    ) => runtime,
                    Err(_) => return Err(StoreFailure::Unavailable),
                }
            }
            // Invalid/missing trust never permits sign-in. The default checker
            // returns the closed InvalidConfiguration refusal for core to observe.
            Ok(None) | Err(_) => runtime,
        };
        let mail_pushes = self.mail_pushes.clone();
        let state_for_revocation = self.state.clone();
        let runtime = Arc::new(
            runtime
                .with_mail_delivery_observer(Arc::new(move |message| {
                    mail_pushes
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(message);
                }))
                .with_machine_revocation_observer(Arc::new(move |machine_id| {
                    for socket in state_for_revocation.get_websockets_with_tag(machine_id.as_str())
                    {
                        let _ = socket.close(Some(1008), Some("machine authority revoked"));
                    }
                })),
        );
        runtime.recompute_alarm().await?;
        *initialized = Some(runtime.clone());
        Ok(runtime)
    }
    async fn alarm_runtime(
        &self,
    ) -> std::result::Result<Option<(OrganizationId, Arc<OrgRuntime>)>, AlarmFailure> {
        self.migrate().await.map_err(AlarmFailure::Store)?;
        let host = self.statement_host();
        let identity = collaboration_central_core::prepare_central_alarm(host.as_ref())
            .await
            .map_err(AlarmFailure::Store)?;
        let collaboration_central_core::CentralOrganizationIdentity::Stamped { organization_id } =
            identity
        else {
            return Ok(None);
        };
        self.check_physical_organization(&organization_id)
            .map_err(AlarmFailure::Platform)?;
        #[cfg(feature = "test-support")]
        signin_launcher_controls::sample_clock(self, &organization_id)
            .await
            .map_err(AlarmFailure::Platform)?;
        let runtime = self
            .runtime_checked(organization_id.clone())
            .await
            .map_err(AlarmFailure::Store)?;
        Ok(Some((organization_id, runtime)))
    }
}
impl DurableObject for CentralObject {
    fn new(state: State, env: Env) -> Self {
        Self {
            connection: DoConnection::new(state.storage()),
            state: Arc::new(SendWrapper::new(state)),
            env,
            runtime: Mutex::new(None),
            clock: WorkspaceClock::default(),
            due_batches: Arc::new(fleet_runtime::DueBatchState::default()),
            mail_pushes: Default::default(),
            #[cfg(feature = "test-support")]
            controls: workspace_cases::Controls::default(),
            #[cfg(feature = "test-support")]
            launcher_clock_sampled: Mutex::new(false),
        }
    }
    async fn fetch(&self, request: Request) -> Result<Response> {
        let address = request
            .headers()
            .get("x-central-object-address")?
            .ok_or_else(|| worker::Error::RustError("central address missing".to_owned()))?;
        let ObjectAddress::Central(organization) = serde_json::from_str(&address)? else {
            return Response::error("wrong object kind", 400);
        };
        self.check_physical_organization(&organization)?;
        #[cfg(feature = "test-support")]
        {
            signin_launcher_controls::sample_clock(self, &organization).await?;
            if request.path() == "/test/raw-signin-observe" {
                return signin_launcher_controls::observe(self, &organization).await;
            }
            if request.path().starts_with("/test/raw-") {
                return workspace_cases::raw_control(self, request).await;
            }
        }
        self.migrate()
            .await
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
        let identity =
            collaboration_central_core::read_central_organization(self.statement_host().as_ref())
                .await
                .map_err(|error| worker::Error::RustError(error.to_string()))?;
        if matches!(identity, collaboration_central_core::CentralOrganizationIdentity::Stamped { organization_id } if organization_id != organization)
        {
            if crate::organization_signin_link::is_signin_path(&request.path()) {
                return Response::empty().map(|response| response.with_status(503));
            }
            return Response::error("central organization mismatch", 403);
        }
        #[cfg(feature = "test-support")]
        if request.path().starts_with("/test/") {
            return workspace_cases::control(self, organization, request).await;
        }
        if crate::organization_signin_link::is_signin_path(&request.path()) {
            let runtime = match self.runtime_checked(organization).await {
                Ok(runtime) => runtime,
                Err(_) => return Response::empty().map(|response| response.with_status(503)),
            };
            let request: worker::HttpRequest = request.try_into()?;
            let (parts, body) = request.into_parts();
            let body = match crate::http_body::complete(body).await? {
                crate::http_body::BodyCompletion::Complete(body) => body,
                crate::http_body::BodyCompletion::OverLimit => {
                    return crate::http_body::over_limit();
                }
            };
            use tower_service::Service;
            let response = collaboration_central_core::organization_signin_router(runtime.clone())
                .call(http::Request::from_parts(parts, body))
                .await?;
            self.flush_ready_mail(&runtime).await;
            return response.try_into();
        }
        if let Some(path_organization) = crate::workspace_link_entry::organization(&request.path())
        {
            if path_organization.as_ref() != Ok(&organization) {
                return Response::error("wrong organization", 403);
            }
            return self.upgrade_machine_link(organization, &request).await;
        }
        Response::error("unknown central route", 404)
    }

    async fn websocket_message(
        &self,
        socket: worker::WebSocket,
        message: worker::WebSocketIncomingMessage,
    ) -> Result<()> {
        self.handle_machine_message(socket, message).await
    }
    async fn websocket_close(
        &self,
        socket: worker::WebSocket,
        _code: usize,
        _reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        socket.close(Some(1000), Some("closed"))
    }
    async fn websocket_error(
        &self,
        socket: worker::WebSocket,
        _error: worker::Error,
    ) -> Result<()> {
        socket.close(Some(1011), Some("link error"))
    }

    async fn alarm(&self) -> Result<Response> {
        #[cfg(feature = "test-support")]
        return workspace_cases::alarm_with_receipt(self).await;
        #[cfg(not(feature = "test-support"))]
        {
            let runtime = match self.alarm_runtime().await {
                Ok(Some((_, runtime))) => runtime,
                Ok(None) => return Response::ok("unprovisioned"),
                Err(AlarmFailure::Store(error)) => {
                    return Err(worker::Error::RustError(error.to_string()));
                }
                Err(AlarmFailure::Platform(error)) => return Err(error),
            };
            let result = runtime.after_event().await;
            self.flush_ready_mail(&runtime).await;
            result.map_err(|error| worker::Error::RustError(error.to_string()))?;
            Response::ok("alarm applied")
        }
    }
}

#[cfg(feature = "test-support")]
#[path = "workspace_cases.rs"]
pub(crate) mod workspace_cases;

#[cfg(feature = "test-support")]
#[path = "signin_launcher_controls.rs"]
mod signin_launcher_controls;

#[path = "workspace_machine_link.rs"]
mod workspace_machine_link;

#[cfg(feature = "test-support")]
#[path = "workspace_link_witness.rs"]
mod workspace_link_witness;
