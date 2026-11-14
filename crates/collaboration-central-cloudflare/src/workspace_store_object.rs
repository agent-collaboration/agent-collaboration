//! One workspace's store: it holds the tables and runs the statements it is given,
//! atomically. No rules, decisions or notifications live here (owner decision 29);
//! central sends every call.

use crate::statement_store::{self, AlarmPolicy};
use crate::store_wire::{OwnedStoreCall, StoreAnswer};
use central_storage::{StoreFailure, StoreReply};
use collaboration_central_core::ObjectAddress;
use sqlx_durable_object::DoConnection;
use std::cell::Cell;
use worker::{DurableObject, Env, Request, Response, Result, State, durable_object, wasm_bindgen};

#[durable_object]
pub struct WorkspaceStoreObject {
    state: State,
    env: Env,
    connection: DoConnection,
    migrated: Cell<bool>,
}

impl WorkspaceStoreObject {
    /// Refuses a call addressed to another object than this one.
    fn check_address(&self, request: &Request) -> Result<()> {
        let requested = request
            .headers()
            .get("x-central-object-address")?
            .ok_or_else(|| worker::Error::RustError("object address missing".into()))?;
        let address: ObjectAddress = serde_json::from_str(&requested)?;
        if !matches!(address, ObjectAddress::WorkspaceStore(_)) {
            return Err(worker::Error::RustError(
                "not a workspace store address".into(),
            ));
        }
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
        Ok(())
    }

    async fn answer(&self, call: OwnedStoreCall) -> StoreAnswer {
        if !self.migrated.get() {
            statement_store::migrate(
                &self.connection,
                crate::object_migrations::workspace_store(),
            )
            .await
            .map_err(|failure| {
                worker::console_warn!("workspace store migration failed: {failure}");
                StoreFailure::Unavailable
            })?;
            self.migrated.set(true);
        }
        match call {
            OwnedStoreCall::Batch(batch) => {
                statement_store::run_batch(&self.connection, batch, AlarmPolicy::Ignore)
                    .await
                    .map(StoreReply::Batch)
            }
            OwnedStoreCall::Read(statement) => statement_store::read(&self.connection, statement)
                .await
                .map(StoreReply::Read),
        }
    }
}

impl DurableObject for WorkspaceStoreObject {
    fn new(state: State, env: Env) -> Self {
        let connection = DoConnection::new(state.storage());
        Self {
            state,
            env,
            connection,
            migrated: Cell::new(false),
        }
    }

    async fn fetch(&self, mut request: Request) -> Result<Response> {
        self.check_address(&request)?;
        if request.path() != "/store" {
            return Response::error("unknown store route", 404);
        }
        let call: OwnedStoreCall = request.json().await?;
        Response::from_json(&self.answer(call).await)
    }
}
