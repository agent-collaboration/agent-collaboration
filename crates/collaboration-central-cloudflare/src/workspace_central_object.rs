//! Org-scoped central: shared core over a statement-only DO adapter, no Fleet engine.
use crate::{
    sign_in_trust::CloudflareSignInTrust,
    statement_store::{self, AlarmPolicy},
    worker_clock::WorkspaceClock,
};
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use collaboration_central_core::{
    ClockIds, ObjectAddress, ObjectHost, WorkspaceCaller, WorkspaceRuntime, WorkspaceStoreCalls,
};
use fleet_identity_protocol::OrganizationId;
use futures_util::lock::Mutex;
use serde::{Deserialize, Serialize};
use sqlx_durable_object::DoConnection;
use std::sync::Arc;
use worker::{
    DurableObject, Env, Request, Response, Result, State, durable_object,
    send::{SendFuture, SendWrapper},
    wasm_bindgen,
};

struct CentralStatements {
    #[cfg(feature = "test-support")]
    alarm_batch_failure: Arc<std::sync::Mutex<Option<StoreFailure>>>,
    connection: SendWrapper<DoConnection>,
    state: Arc<SendWrapper<State>>,
}
impl ObjectHost for CentralStatements {
    fn run_batch(
        &self,
        batch: StatementBatch,
    ) -> impl std::future::Future<Output = std::result::Result<BatchOutcome, StoreFailure>> + Send
    {
        SendFuture::new(async move {
            #[cfg(feature = "test-support")]
            if batch.statements.is_empty()
                && matches!(batch.alarm, central_storage::AlarmUpdate::SetFromQuery(_))
                && let Some(failure) = self
                    .alarm_batch_failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
            {
                return Err(failure);
            }
            statement_store::run_batch(
                &self.connection,
                batch.into(),
                AlarmPolicy::Apply(self.state.storage()),
            )
            .await
        })
    }
    fn read(
        &self,
        statement: Statement,
    ) -> impl std::future::Future<Output = std::result::Result<ReadOutcome, StoreFailure>> + Send
    {
        SendFuture::new(statement_store::read(&self.connection, statement.into()))
    }
}
#[derive(Clone)]
struct WorkspaceTransport {
    env: Arc<SendWrapper<Env>>,
    #[cfg(feature = "test-support")]
    lose_read: Arc<std::sync::atomic::AtomicBool>,
}
impl WorkspaceStoreCalls for WorkspaceTransport {
    fn call_workspace(
        &self,
        id: &workspace_model::WorkspaceId,
        call: StoreCall,
    ) -> impl std::future::Future<Output = std::result::Result<StoreReply, StoreFailure>> + Send
    {
        let id = id.clone();
        SendFuture::new(async move {
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
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CentralRequest {
    pub organization_id: OrganizationId,
    pub caller: workspace_model::Caller,
    pub credential_hash: String,
    pub request: workspace_model::WorkspaceRequest,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CentralMailRequest {
    pub organization_id: OrganizationId,
    pub caller: workspace_model::Caller,
    pub credential_hash: String,
    pub request: collaboration_protocol::central_mail::CentralMailRequest,
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
    #[cfg(feature = "test-support")]
    controls: workspace_cases::Controls,
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
            alarm_batch_failure: self.controls.alarm_batch_failure.clone(),
            connection: SendWrapper::new(DoConnection::new(self.state.storage())),
            state: self.state.clone(),
        })
    }
    async fn migrate(&self) -> std::result::Result<(), StoreFailure> {
        statement_store::migrate(
            &self.connection,
            Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
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
        let clock: Arc<dyn runtime_primitives::Clock> = Arc::new(self.clock.clone());
        let runtime = WorkspaceRuntime::new(
            organization.clone(),
            host,
            WorkspaceTransport {
                env: Arc::new(SendWrapper::new(self.env.clone())),
                #[cfg(feature = "test-support")]
                lose_read: self.controls.lose_read.clone(),
            },
            clock.clone(),
            Arc::new(ClockIds::new(self.clock.clone())),
        );
        let runtime = match crate::organization_signin_link::trust_settings(&self.env) {
            Ok(Some(settings)) => match CloudflareSignInTrust::new(settings).for_organization(organization, clock) {
                Ok(check) => runtime.with_sign_in_verification(check),
                Err(collaboration_central_core::organization_authority::FleetOrganizationStatementRefusal::InvalidConfiguration) => runtime,
                Err(_) => return Err(StoreFailure::Unavailable),
            },
            // Invalid/missing trust never permits sign-in. The default checker
            // returns the closed InvalidConfiguration refusal for core to observe.
            Ok(None) | Err(_) => runtime,
        };
        #[cfg(feature = "test-support")]
        let runtime = runtime.with_mail_policy(self.controls.mail_policy.get().unwrap_or_default());
        let runtime = Arc::new(
            runtime.with_sign_in_observer(Arc::new(crate::access_verification::observe_signin)),
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
            #[cfg(feature = "test-support")]
            controls: workspace_cases::Controls::default(),
        }
    }
    async fn fetch(&self, mut request: Request) -> Result<Response> {
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
            let response = collaboration_central_core::organization_signin_router(runtime)
                .call(http::Request::from_parts(parts, body))
                .await?;
            return response.try_into();
        }
        if request.path() == "/mail" {
            let call: CentralMailRequest = request.json().await?;
            if call.organization_id != organization {
                return Response::error("wrong organization", 403);
            }
            let runtime = self.runtime(organization).await?;
            let caller = WorkspaceCaller {
                organization_id: call.organization_id,
                caller: call.caller,
                credential_hash: call.credential_hash,
            };
            return Response::from_json(&runtime.request_central(&caller, call.request).await);
        }
        let call: CentralRequest = request.json().await?;
        if call.organization_id != organization {
            return Response::error("wrong organization", 403);
        }
        let runtime = self.runtime(organization).await?;
        let caller = WorkspaceCaller {
            organization_id: call.organization_id,
            caller: call.caller,
            credential_hash: call.credential_hash,
        };
        Response::from_json(&runtime.request(&caller, call.request).await)
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
            runtime
                .on_alarm()
                .await
                .map_err(|error| worker::Error::RustError(error.to_string()))?;
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
