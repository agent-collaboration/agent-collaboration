//! Org-scoped central: shared core over a statement-only DO adapter, no Fleet engine.
use crate::{
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
    connection: SendWrapper<DoConnection>,
    state: Arc<SendWrapper<State>>,
}
impl ObjectHost for CentralStatements {
    fn run_batch(
        &self,
        batch: StatementBatch,
    ) -> impl std::future::Future<Output = std::result::Result<BatchOutcome, StoreFailure>> + Send
    {
        SendFuture::new(statement_store::run_batch(
            &self.connection,
            batch.into(),
            AlarmPolicy::Apply(self.state.storage()),
        ))
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
type OrgRuntime = WorkspaceRuntime<CentralStatements, WorkspaceTransport>;
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
    async fn runtime(&self, organization: OrganizationId) -> Result<Arc<OrgRuntime>> {
        let mut initialized = self.runtime.lock().await;
        if let Some(runtime) = initialized.as_ref() {
            return Ok(runtime.clone());
        }
        statement_store::migrate(
            &self.connection,
            Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
        )
        .await
        .map_err(|failure| worker::Error::RustError(failure.to_string()))?;
        let host = Arc::new(CentralStatements {
            connection: SendWrapper::new(DoConnection::new(self.state.storage())),
            state: self.state.clone(),
        });
        let runtime = Arc::new(WorkspaceRuntime::new(
            organization,
            host,
            WorkspaceTransport {
                env: Arc::new(SendWrapper::new(self.env.clone())),
                #[cfg(feature = "test-support")]
                lose_read: self.controls.lose_read.clone(),
            },
            Arc::new(self.clock.clone()),
            Arc::new(ClockIds::new(self.clock.clone())),
        ));
        *initialized = Some(runtime.clone());
        Ok(runtime)
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
        let namespace = self.env.durable_object("CENTRAL")?;
        if namespace.id_from_name(organization.as_str())?.to_string() != self.state.id().to_string()
        {
            return Response::error("wrong physical org", 403);
        }
        #[cfg(feature = "test-support")]
        if request.path().starts_with("/test/") {
            return workspace_cases::control(self, organization, request).await;
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
}

#[cfg(feature = "test-support")]
#[path = "workspace_cases.rs"]
pub(crate) mod workspace_cases;
