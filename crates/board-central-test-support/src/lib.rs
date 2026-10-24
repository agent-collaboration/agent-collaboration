//! A real Access-verified central and machine session for board-side tests.
//! This crate is consumed only by dev-dependencies; it owns no board policy.
use board_fleet_sync::BoardSessionInputs;
use collaboration_central_conformance::{JwksFixtureServer, fixture_policy, owner_assertion};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use collaboration_service::{FleetClient, ProjectRegistration};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::{PeerId, ReceiverId};
use message_board::ActorOrigin;
use std::{path::Path, sync::Arc, time::Duration};

mod access_edge;
mod enrollment_edge;
mod enrollment_fixture;
pub use enrollment_fixture::EnrollmentTestCentral;
#[cfg(feature = "conformance")]
mod piece_integrity_refusal;
#[cfg(feature = "conformance")]
pub use piece_integrity_refusal::PieceIntegrityObservation;
mod claim_grant_drop;
mod lease_notice_gate;
pub use claim_grant_drop::{ClaimGrantDropObservation, DroppedClaimGrant};
pub use lease_notice_gate::LeaseNoticeObservation;
mod fleet_observer;
mod machine_enrollment;
mod machine_session;
mod owner_transfers;
pub use machine_enrollment::EnrolledTestMachine;
#[cfg(feature = "conformance")]
mod payload_input_observer;
pub use collaboration_central_conformance::{DispatchFaultObservation, RuntimeControl};
pub use collaboration_central_core::{ConformanceBoardState, ObjectAddress};
pub use machine_session::FleetTestRuntime;
#[cfg(feature = "conformance")]
pub use payload_input_observer::{PayloadInputObservation, RouterPayloadInputBodies};
pub type FixtureResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(20);
const INSTALLATION_PEER: &str = "019f0000-0000-7000-8000-000000001f15";

pub struct TestCentral {
    server: TokioServer,
    jwks: JwksFixtureServer,
    machine: EnrolledTestMachine,
}

impl TestCentral {
    /// The first fixture installation's actual established session count.
    pub fn session_connections(&self) -> usize {
        self.machine.session_connections()
    }
    /// Owner-route tests use the same real server as the enrolled session.
    pub fn base_url(&self) -> &str {
        self.server.base_url()
    }
    pub fn hold_granted_notice(
        &self,
        project: message_board::ProjectId,
        epoch: fleet_sync_protocol::Epoch,
    ) -> FixtureResult<LeaseNoticeObservation> {
        self.machine.hold_granted_notice(project, epoch)
    }
    pub fn hold_superseded_notice(
        &self,
        project: message_board::ProjectId,
        epoch: fleet_sync_protocol::Epoch,
    ) -> FixtureResult<LeaseNoticeObservation> {
        self.machine.edge.hold_superseded_notice(project, epoch)
    }
    pub fn drop_claim_grant_once(
        &self,
        attempt: fleet_sync_protocol::RequestId,
    ) -> FixtureResult<ClaimGrantDropObservation> {
        self.machine.edge.drop_claim_grant_once(attempt)
    }
    #[cfg(feature = "conformance")]
    pub fn refuse_piece_integrity(
        &self,
        database: fleet_sync_protocol::DatabaseId,
        table: fleet_sync_protocol::TableName,
        references: fleet_sync_protocol::TableName,
    ) -> FixtureResult<PieceIntegrityObservation> {
        self.machine
            .edge
            .refuse_piece_integrity(database, table, references)
    }
    /// Reuse the real composed object's existing conformance controls.
    pub async fn runtime_control(
        &self,
        address: collaboration_central_core::ObjectAddress,
        command: collaboration_central_conformance::RuntimeControl,
    ) -> FixtureResult<collaboration_central_conformance::RuntimeFaultObservation> {
        Ok(reqwest::Client::builder()
            .no_proxy()
            .timeout(BOUND)
            .build()?
            .post(format!(
                "{}/admin/v1/conformance/runtime",
                self.server.base_url()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&collaboration_central_conformance::RuntimeControlRequest { address, command })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// The real owning ImportTask and admin seed consume captured board pages.
    pub async fn import_board_snapshot(
        &self,
        pages: Vec<fleet_sync_protocol::SnapshotPage>,
        manifest: fleet_sync_protocol::SnapshotManifest,
    ) -> FixtureResult<fleet_sync_protocol::ReplicationPosition> {
        self.import_board_snapshot_in_state(ConformanceBoardState::Open, pages, manifest)
            .await
    }

    pub async fn import_board_snapshot_in_state(
        &self,
        state: ConformanceBoardState,
        pages: Vec<fleet_sync_protocol::SnapshotPage>,
        manifest: fleet_sync_protocol::SnapshotManifest,
    ) -> FixtureResult<fleet_sync_protocol::ReplicationPosition> {
        let owner = collaboration_central_core::OwnerIdentity::from_access(
            collaboration_central_core::AccessCaller::Owner {
                email: "owner@example.test".into(),
            },
        )?;
        Ok(self
            .server
            .conformance_platform()
            .conformance_import_board_snapshot(owner, state, pages, manifest)
            .await?)
    }

    /// Exercise the normal owner-authenticated retirement endpoint.
    pub async fn retire_project(
        &self,
        database: fleet_sync_protocol::DatabaseId,
        request: fleet_sync_protocol::RequestId,
        fence: fleet_sync_protocol::Epoch,
        acked: fleet_sync_protocol::ReplicationPosition,
    ) -> FixtureResult<serde_json::Value> {
        Ok(reqwest::Client::builder()
            .no_proxy()
            .timeout(BOUND)
            .build()?
            .post(format!(
                "{}/admin/v1/projects/{database}/retire",
                self.server.base_url()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&serde_json::json!({"requestId":request,"expectFence":fence,"expectAcked":acked}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    #[cfg(feature = "conformance")]
    fn observe_payload_input_bodies(&self) -> FixtureResult<PayloadInputObservation> {
        self.machine.edge.observe_payload_input_bodies()
    }

    /// The supplied origin becomes the actual registry enrollment, never a frame field.
    pub async fn start(root: &Path, origin: &ActorOrigin) -> FixtureResult<Self> {
        Self::start_with_peer(
            root,
            origin,
            PeerId::try_from(INSTALLATION_PEER.to_owned())?,
        )
        .await
    }

    /// Enroll the Host's persisted installation identity on the real central.
    pub async fn start_with_peer(
        root: &Path,
        origin: &ActorOrigin,
        peer: PeerId,
    ) -> FixtureResult<Self> {
        let jwks = JwksFixtureServer::start().await?;
        // Stable fixture deployment configuration across an owning-store reopen.
        let receiver = ReceiverId::try_from("019f0000-0000-7000-8000-000000000010".to_owned())?;
        let server = TokioServer::start(TokioSettings {
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
            root: root.into(),
            access_policy: fixture_policy(),
            jwks_url: jwks.url.clone(),
            registry: RegistryConfig {
                lease: LeasePolicy {
                    lease_duration: Duration::from_secs(14_400),
                    renew_interval: Duration::from_secs(900),
                },
                authoritative_receiver: receiver,
                release_unsent_for: Duration::ZERO,
                move_needs_renewal_within: Some(Duration::from_secs(1800)),
            },
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        })
        .await?;
        let machine = EnrolledTestMachine::enroll(
            server.base_url(),
            origin,
            peer,
            receiver,
            "board-central-native-fixture".to_owned(),
        )
        .await?;
        Ok(Self {
            server,
            jwks,
            machine,
        })
    }

    pub fn session_inputs(&self) -> FixtureResult<BoardSessionInputs> {
        self.machine.session_inputs()
    }

    /// Give the owned Host child the exact enrollment used by this fixture.
    pub fn write_host_session(&self, directory: &Path) -> FixtureResult<std::path::PathBuf> {
        self.machine.write_host_session(directory)
    }

    /// A second real enrollment shares this central's Registry, Copy and receiver.
    pub async fn additional_machine(
        &self,
        origin: &ActorOrigin,
        peer: PeerId,
    ) -> FixtureResult<EnrolledTestMachine> {
        EnrolledTestMachine::enroll(
            self.server.base_url(),
            origin,
            peer,
            self.machine.receiver,
            format!("board-central-native-fixture-{peer}"),
        )
        .await
    }

    #[cfg(feature = "conformance")]
    async fn prune_fleet_feed(&self) -> FixtureResult<(u64, i64)> {
        Ok(self
            .server
            .conformance_platform()
            .conformance_prune_fleet_feed()
            .await?)
    }

    pub async fn shutdown(self) -> FixtureResult {
        self.machine.shutdown().await?;
        self.server.shutdown().await?;
        self.jwks.shutdown().await?;
        Ok(())
    }
}

/// The caller keeps the runtime guard beside the client and closes it after admission.
pub async fn fleet_client(
    directory: &Path,
    registration: Arc<dyn ProjectRegistration>,
    origin: &ActorOrigin,
) -> FixtureResult<(FleetClient, FleetTestRuntime)> {
    let (requests, runtime) = machine_session(directory, origin).await?;
    let client = FleetClient::open(directory, registration, Some(requests)).await?;
    client.synchronize().await?;
    Ok((client, runtime))
}

/// Unit-test crates construct their own product type with this same session capability.
pub async fn machine_session(
    directory: &Path,
    origin: &ActorOrigin,
) -> FixtureResult<(
    fleet_sync_tokio_runner::SessionRequests<board_sync_protocol::BoardConsumerProtocol>,
    FleetTestRuntime,
)> {
    let central = TestCentral::start(&directory.join("test-central"), origin).await?;
    machine_session::connect(directory, central).await
}
