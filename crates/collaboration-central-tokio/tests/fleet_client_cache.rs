//! Router FleetClient rebuilds through the production transport and real central.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)] // Permanent scenario assertions; fixture failures propagate.
use board_fleet_sync::{AccessServiceToken, MachineSessionStore, ReqwestSessionTransport};
use board_sync_protocol::{BoardConsumerProtocol, BoardConsumerRequest};
use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use collaboration_service::{
    FleetClient, ProjectRegistration, ProjectRegistrationFuture, ProjectRegistrationState,
};
use fleet_admission::{FleetOperation, FleetRequest};
use fleet_sync_protocol::{ClientFrame, OperationRequest, SchemaSupport};
use fleet_sync_session::{ClientEvent, Endpoint, PeerCredential, SessionClient};
use fleet_sync_tokio_runner::SessionPump;
use futures_util::{SinkExt, StreamExt};
use message_board::*;
use reqwest_websocket::Message;
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::watch;

#[path = "fleet_client_cache/archive.rs"]
mod archive;
#[path = "native_http/support.rs"]
mod configuration;
#[path = "fleet_client_cache/fixture.rs"]
mod fixture;
#[path = "fleet_client_cache/ingress.rs"]
mod ingress;
#[path = "fleet_client_cache/outbox.rs"]
mod outbox;
#[path = "fleet_client_cache/rebuild.rs"]
mod rebuild;
#[path = "fleet_client_cache/resync.rs"]
mod resync;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(20);
const READER_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440ca1";
const WRITER_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440ca2";
const WORKSPACE: &str = "ffffffff-ffff-7fff-8fff-fffffffffca1";
struct NoRegistration;
impl ProjectRegistration for NoRegistration {
    fn wait_ready<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(ProjectRegistrationState::Pending) })
    }

    fn start<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(ProjectRegistrationState::Pending) })
    }
}
