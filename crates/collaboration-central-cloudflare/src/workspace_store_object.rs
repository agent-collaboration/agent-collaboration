//! One workspace's SQLite and its typed procedures. Legacy operations still
//! use the statement route until their procedures move.

use crate::object_location::ObjectAddress;
use crate::statement_store::{self, AlarmPolicy};
use crate::store_wire::{OwnedStoreCall, StoreAnswer};
use central_storage::{StoreFailure, StoreReply};
use fleet_contracts::{WorkspaceRequest, WorkspaceServiceProtocol, WorkspaceStatusFailure};
use fleet_rpc::{Envelope, dispatch};
use fleet_workspace_service::WorkspaceService;
use sqlx_durable_object::DoConnection;
use std::cell::{Cell, OnceCell};
use worker::{DurableObject, Env, Request, Response, Result, State, durable_object, wasm_bindgen};

#[durable_object]
pub struct WorkspaceStoreObject {
    state: State,
    env: Env,
    connection: OnceCell<DoConnection>,
    migrated: Cell<bool>,
    #[cfg(feature = "test-support")]
    typed_dispatches: Cell<u32>,
}

impl WorkspaceStoreObject {
    /// Refuses a call addressed to another object than this one.
    fn check_address(&self, request: &Request) -> Result<workspace_ids::WorkspaceId> {
        let requested = request
            .headers()
            .get("x-central-object-address")?
            .ok_or_else(|| worker::Error::RustError("object address missing".into()))?;
        let address: ObjectAddress = serde_json::from_str(&requested)?;
        let ObjectAddress::WorkspaceStore(workspace) = &address else {
            return Err(worker::Error::RustError(
                "not a workspace store address".into(),
            ));
        };
        let (binding, name) = crate::object_location::location(&address);
        if self
            .env
            .durable_object(binding)?
            .id_from_name(&name)?
            .to_string()
            != self.state.id().to_string()
        {
            return Err(worker::Error::RustError(
                "object address does not match physical id".into(),
            ));
        }
        Ok(workspace.clone())
    }

    async fn open_store(
        &self,
        migrator: std::sync::Arc<sqlx_core::migrate::Migrator>,
    ) -> std::result::Result<&DoConnection, StoreFailure> {
        let connection = self
            .connection
            .get_or_init(|| DoConnection::new(self.state.storage()));
        if !self.migrated.get() {
            statement_store::migrate(connection, migrator)
                .await
                .map_err(|failure| {
                    worker::console_warn!("workspace store migration failed: {failure}");
                    StoreFailure::Unavailable
                })?;
            self.migrated.set(true);
        }
        Ok(connection)
    }

    async fn answer(
        &self,
        call: OwnedStoreCall,
        migrator: std::sync::Arc<sqlx_core::migrate::Migrator>,
    ) -> StoreAnswer {
        let connection = self.open_store(migrator).await?;
        match call {
            OwnedStoreCall::Batch(batch) => {
                statement_store::run_batch(connection, batch, AlarmPolicy::Ignore)
                    .await
                    .map(StoreReply::Batch)
            }
            OwnedStoreCall::Read(statement) => statement_store::read(connection, statement)
                .await
                .map(StoreReply::Read),
        }
    }
}

impl DurableObject for WorkspaceStoreObject {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            env,
            connection: OnceCell::new(),
            migrated: Cell::new(false),
            #[cfg(feature = "test-support")]
            typed_dispatches: Cell::new(0),
        }
    }

    async fn fetch(&self, mut request: Request) -> Result<Response> {
        let workspace = self.check_address(&request)?;
        #[cfg(feature = "test-support")]
        if request.path() == "/test/storage-state" {
            return Response::from_json(&crate::workspace_rpc_fixture::StorageState {
                opened: self.connection.get().is_some(),
                migrated: self.migrated.get(),
                typed_dispatches: self.typed_dispatches.get(),
            });
        }
        if request.path() == "/workspace-rpc" {
            if request.method() != worker::Method::Post {
                return Response::error("expected POST", 405);
            }
            let envelope: Envelope<workspace_ids::WorkspaceId, WorkspaceRequest> =
                request.json().await?;
            let reply =
                dispatch::<WorkspaceServiceProtocol, _, _>(&workspace, envelope, |call| async {
                    #[cfg(feature = "test-support")]
                    self.typed_dispatches
                        .set(self.typed_dispatches.get().saturating_add(1));
                    match self
                        .open_store(crate::object_migrations::workspace_store())
                        .await
                    {
                        Ok(connection) => {
                            fleet_contracts::dispatch_workspace_request(
                                &WorkspaceService::new(connection),
                                call,
                            )
                            .await
                        }
                        Err(_) => call.unavailable_reply(WorkspaceStatusFailure::Unavailable),
                    }
                })
                .await
                .map_err(|_| {
                    worker::Error::RustError(
                        "workspace envelope target does not match address".into(),
                    )
                })?;
            return Response::from_json(&reply);
        }
        #[cfg(feature = "test-support")]
        if request.path() == "/test/crash" {
            self.state
                .block_concurrency_while(async {
                    Err::<(), worker::Error>(worker::Error::RustError(
                        "workspace store fixture crash".to_owned(),
                    ))
                })
                .await?;
            return Response::error("object did not reset", 500);
        }
        if request.path() != "/store" {
            return Response::error("unknown store route", 404);
        }
        let call: OwnedStoreCall = request.json().await?;
        let migrator = crate::object_migrations::workspace_store();
        #[cfg(feature = "test-support")]
        let migrator =
            if request.headers().get("x-workspace-fixture")?.as_deref() == Some("widgets") {
                crate::object_migrations::widgets_store()
            } else if request.headers().get("x-workspace-fixture")?.as_deref()
                == Some("failingBusiness")
            {
                std::sync::Arc::new(fleet_storage_schema::workspace_migrations::failing_upgrade(
                    fleet_storage_schema::workspace_migrations::workspace_migrator(),
                ))
            } else {
                migrator
            };
        Response::from_json(&self.answer(call, migrator).await)
    }
}
