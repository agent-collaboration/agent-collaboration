//! A test-only DO fixture for the portable bootstrap and real library SQL.

use crate::conformance_core_store::{ControlIo, ReceiptStoreHost, new_host};
use collaboration_central_core::{
    AccessCaller, AccessClientId, BootstrapCommand, EnrollMachine, ObjectBootstrap, ObjectHost,
    ObjectRuntime, OwnerIdentity, RegistryKind,
};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::ReceiverId;
use serde::Deserialize;
use std::time::Duration;
use std::{cell::OnceCell, sync::Arc};
use worker::{
    DurableObject, Env, Error, Method, Request, RequestInit, Response, Result, State,
    durable_object, send::SendWrapper, wasm_bindgen,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestBody {
    fixture: Fixture,
    command: Command,
    fault: Option<Fault>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Fixture {
    Registry,
    Copy,
    Session,
    Mailbox,
    LateConflict,
    Checksum,
    Noncanonical,
    Unknown,
    Damaged,
    Uncertain,
    Enrollment,
    ImportedUnheld,
}
impl Fixture {
    fn name(&self) -> &'static str {
        match self {
            Self::Registry => "registry",
            Self::Copy => "copy",
            Self::Session => "session",
            Self::Mailbox => "mailbox",
            Self::LateConflict => "lateConflict",
            Self::Checksum => "checksum",
            Self::Noncanonical => "noncanonical",
            Self::Unknown => "unknown",
            Self::Damaged => "damaged",
            Self::Uncertain => "uncertain",
            Self::Enrollment => "enrollment",
            Self::ImportedUnheld => "importedUnheld",
        }
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum Command {
    Bootstrap(BootstrapCommand),
    ImportedUnheld {
        database: fleet_sync_protocol::DatabaseId,
        #[serde(rename = "boardState")]
        board_state: collaboration_central_core::ConformanceBoardState,
    },
    Enroll(EnrollMachine),
    Authenticate {
        access_client_id: AccessClientId,
        credential_hash: String,
    },
}
fn registry(
    host: Arc<ReceiptStoreHost>,
) -> Result<ObjectRuntime<RegistryKind, ReceiptStoreHost, crate::conformance_core_store::ControlIo>>
{
    let receiver = ReceiverId::try_from("019f0000-0000-7000-8000-000000000010".to_owned())
        .map_err(|e| Error::RustError(e.to_string()))?;
    let io = crate::conformance_core_store::ControlIo::new(host.wake().clone());
    ObjectRuntime::registry(
        host,
        io,
        RegistryConfig {
            lease: LeasePolicy {
                lease_duration: Duration::from_secs(60),
                renew_interval: Duration::from_secs(20),
            },
            authoritative_receiver: receiver,
            release_unsent_for: Duration::from_secs(10),
            move_needs_renewal_within: None,
        },
        collaboration_central_core::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )
            .map_err(|error| worker::Error::RustError(error.to_string()))?,
        },
    )
    .map_err(|e| Error::RustError(e.to_string()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Fault {
    LoseRollbackReply,
}

pub(crate) const IMPORTED_UNHELD_PATH: &str = "/test/imported-unheld-fixture";
pub(crate) const CONTROL_PATH: &str = "/admin/v1/conformance/bootstrap";
pub(crate) async fn forward(env: &Env, mut request: Request) -> Result<Response> {
    let labels = crate::access_verification::InvocationLabels::for_request(&request)?;
    let assertion = request.headers().get("Cf-Access-Jwt-Assertion")?;
    let caller = crate::access_verification::verify(
        env,
        assertion.as_deref(),
        CONTROL_PATH,
        &labels,
        crate::access_verification::VerificationLayer::Edge,
    )
    .await;
    let owner = match caller {
        Ok(caller @ AccessCaller::Owner { .. }) => OwnerIdentity::from_access(caller)
            .map_err(|error| Error::RustError(error.to_string()))?,
        _ => return Response::error("owner required", 401),
    };
    let text = request.text().await?;
    let body: RequestBody = serde_json::from_str(&text)?;
    if matches!(body.fixture, Fixture::ImportedUnheld) {
        let Command::ImportedUnheld {
            database,
            board_state,
        } = body.command
        else {
            return Response::error("importedUnheld fixture command required", 400);
        };
        return crate::composed_control::forward(
            env,
            &collaboration_central_core::ObjectAddress::Registry,
            IMPORTED_UNHELD_PATH,
            Some(serde_json::to_string(&(database, owner, board_state))?),
        )
        .await;
    }

    let stub = env
        .durable_object("SCHEMA_PROBES")?
        .id_from_name(body.fixture.name())?
        .get_stub()?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post).with_body(Some(text.into()));
    stub.fetch_with_request(Request::new_with_init(
        "https://schema-probe/execute",
        &init,
    )?)
    .await
}

#[durable_object]
pub struct BootstrapObject {
    state: Arc<SendWrapper<State>>,
    host: OnceCell<Arc<ReceiptStoreHost>>,
}
impl DurableObject for BootstrapObject {
    fn new(state: State, _env: Env) -> Self {
        Self {
            state: Arc::new(SendWrapper::new(state)),
            host: OnceCell::new(),
        }
    }
    async fn fetch(&self, mut request: Request) -> Result<Response> {
        let body: RequestBody = serde_json::from_str(&request.text().await?)?;
        let host = match self.host.get() {
            Some(host) => host.clone(),
            None => {
                let host = new_host(self.state.clone())?;
                self.host
                    .set(host.clone())
                    .map_err(|_| Error::RustError("bootstrap fixture initialized twice".into()))?;
                host
            }
        };
        host.migrate_store()
            .await
            .map_err(|error| Error::RustError(error.to_string()))?;
        if let Some(Fault::LoseRollbackReply) = body.fault {
            #[cfg(feature = "conformance-faults")]
            host.store().inner().lose_next_rollback_reply();
            #[cfg(not(feature = "conformance-faults"))]
            return Response::error("store faults unavailable", 404);
        }
        let now = runtime_primitives::Clock::now(&ControlIo::new(host.wake().clone()));
        let result = match body.command {
            Command::Bootstrap(command) => ObjectBootstrap::new(host.clone(), now)
                .execute(command)
                .await
                .and_then(|reply| {
                    serde_json::to_value(reply).map_err(durable_steps::StoreError::backend)
                }),
            Command::ImportedUnheld { .. } => Err(durable_steps::StoreError::backend(
                "import fixture must use the composed Registry",
            )),
            Command::Enroll(request) => {
                let runtime = registry(host.clone())?;
                let owner = OwnerIdentity::from_access(AccessCaller::Owner {
                    email: "fixture@example.test".into(),
                })
                .map_err(|e| Error::RustError(e.to_string()))?;
                runtime
                    .enroll_machine(&owner, request)
                    .await
                    .and_then(|reply| {
                        serde_json::to_value(reply).map_err(durable_steps::StoreError::backend)
                    })
            }
            Command::Authenticate {
                access_client_id,
                credential_hash,
            } => registry(host.clone())?
                .authenticate_machine(access_client_id, credential_hash)
                .await
                .and_then(|reply| {
                    serde_json::to_value(reply).map_err(durable_steps::StoreError::backend)
                }),
        };
        #[cfg(feature = "conformance-faults")]
        let observed = host.store().inner().reply_loss_observed();
        #[cfg(not(feature = "conformance-faults"))]
        let observed = false;
        Response::from_json(&serde_json::json!({"result":result,"replyLossObserved":observed}))
    }
}
