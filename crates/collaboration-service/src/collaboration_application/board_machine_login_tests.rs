//! The public family boundary gates cached reads and owner capabilities before dispatch.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use collaboration_contracts::{
    MachineLoginGate, MachineLoginState, ProjectClaim, ProjectClaimFuture,
};
use collaboration_protocol::ThreadSubscriptionsRequest;
use message_board::{
    ActorOrigin, BoardError, BoardErrorDetails, BoardFailureKind, BoardFailureStage,
    BoardNextAction, FleetFeedEntry, Identity, ProtocolVersion, Record, RecordEnvelope,
    RecordPlacement, RecordScope, RecordTime, RecordedActor, Workspace, WorkspaceId,
    WorkspaceShowRequest, WorkspaceState,
};
use runtime_primitives::{Clock, SyncTime};
use tokio::sync::watch;

use super::BoardOperations;

const SERVICE: &str = "019f0000-0000-7000-8000-000000000001";
const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000011";
const PROJECT: &str = "019f0000-0000-7000-8000-000000000012";
const REQUEST: &str = "019f0000-0000-7000-8000-000000000013";

#[path = "board_machine_login_writes.rs"]
mod writes;

struct ControlledClock;
impl Clock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_000)
    }
}
impl crate::SubscriptionClock for ControlledClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(1_000).unwrap()
    }
    fn monotonic_now(&self) -> tokio::time::Instant {
        panic!("subscription snapshot must not read monotonic time")
    }
    fn sleep_until(
        &self,
        _: tokio::time::Instant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        panic!("subscription snapshot must not sleep")
    }
}
struct Gate(watch::Receiver<MachineLoginState>);
impl MachineLoginGate for Gate {
    fn machine_login_state(&self) -> MachineLoginState {
        *self.0.borrow()
    }
}
struct RecordingClaim(AtomicUsize);
impl ProjectClaim for RecordingClaim {
    fn claim<'runtime>(
        &'runtime self,
        request: &'runtime board_sync_protocol::ProjectClaimRequest,
    ) -> ProjectClaimFuture<'runtime> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(board_sync_protocol::ProjectClaimReply::Pending {
                attempt: request.request_id.clone(),
            })
        })
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    board: BoardOperations,
    state: watch::Sender<MachineLoginState>,
    claim: Arc<RecordingClaim>,
    workspace: Workspace,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let service: collaboration_protocol::UuidIdentity = SERVICE.to_owned().try_into().unwrap();
        let epoch = "019f0000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap();
        let peer =
            serde_json::from_value(serde_json::json!("019f0000-0000-7000-8000-000000000003"))
                .unwrap();
        let machine =
            collaboration_contracts::MachineIdentity::new(service.clone(), Some("Fixture machine"))
                .unwrap();
        let identity = collaboration_contracts::RouterIdentity::new(
            service.clone(),
            epoch,
            Some(peer),
            machine.clone(),
        )
        .unwrap();
        let registry = Arc::new(collaboration_runtime::ProjectStoreRegistry::new(
            root.path(),
            Some(peer),
            Arc::new(ControlledClock),
        ));
        let workspace_id: WorkspaceId = WORKSPACE.to_owned().try_into().unwrap();
        let workspace = Workspace {
            envelope: RecordEnvelope {
                id: workspace_id.clone().into(),
                scope: RecordScope {
                    workspace: workspace_id.clone(),
                    board: None,
                    project: None,
                },
                actor: RecordedActor::declared(actor()),
                request: None,
                position: RecordPlacement::Fleet(FleetFeedEntry {
                    position: 1.try_into().unwrap(),
                    origin: ActorOrigin {
                        installation: SERVICE.to_owned().try_into().unwrap(),
                        machine: "Fixture machine".to_owned().try_into().unwrap(),
                    },
                }),
                time: RecordTime::Router(chrono::DateTime::from_timestamp_millis(1_000).unwrap()),
            },
            workspace_id,
            name: "Cached workspace".to_owned().try_into().unwrap(),
            description: "Fixture".to_owned().try_into().unwrap(),
            state: WorkspaceState::Active,
            repositories: Vec::new(),
        };
        let cache =
            message_board_storage::FleetCache::open(&root.path().join("fleet-cache.sqlite"))
                .await
                .unwrap();
        cache
            .replace_snapshot(&message_board_storage::FleetRecordSnapshot {
                through: 1.try_into().unwrap(),
                records: vec![Record::Workspace(workspace.clone())],
            })
            .await
            .unwrap();
        let fleet = Arc::new(
            collaboration_runtime::FleetClient::open(
                root.path(),
                Arc::new(crate::test_project_runtime::PendingRegistration),
                None,
            )
            .await
            .unwrap(),
        );
        let admission = Arc::new(
            crate::ProjectAdmission::new(crate::ProjectAdmissionProps {
                registry: registry.clone(),
                fleet,
                identity: machine,
            })
            .unwrap(),
        );
        let interactions = Arc::new(
            interaction_storage::InteractionStore::open(root.path().join("interactions.sqlite"))
                .await
                .unwrap(),
        );
        let native = crate::collaboration_application::NativeSessionControl::new(
            identity.clone(),
            collaboration_runtime::EndpointDirectory::new(service),
            None,
            None,
            collaboration_runtime::SessionDisplayNameCache::default(),
        );
        let context = crate::collaboration_application::SessionContextReader::new(
            identity.clone(),
            native,
            None,
            None,
        );
        let claim = Arc::new(RecordingClaim(AtomicUsize::new(0)));
        let board = BoardOperations::new(
            identity,
            Some(admission),
            Some(claim.clone()),
            None,
            None,
            None,
            Arc::new(ControlledClock),
            Some(interactions),
            Arc::new(collaboration_contracts::UnacknowledgedProjectReplication),
            context,
            Some(registry),
        );
        let (state, observed) = watch::channel(MachineLoginState::NotLoggedIn);
        Self {
            _root: root,
            board: board.with_machine_login_gate(Arc::new(Gate(observed))),
            state,
            claim,
            workspace,
        }
    }
}

fn actor() -> Identity {
    Identity::Human {
        human_id: "fixture-owner".to_owned().try_into().unwrap(),
    }
}
fn assert_not_logged_in(error: BoardError) {
    assert_eq!(error.kind, BoardFailureKind::NotLoggedIn);
    assert_eq!(error.stage, BoardFailureStage::Admission);
    assert_eq!(error.next_action, BoardNextAction::RunFleetLogin);
    assert_eq!(error.details, BoardErrorDetails::None);
    assert!(
        error
            .message
            .contains("codex-router fleet login --organization <id> --label <label>")
    );
    let wire = serde_json::to_value(error).unwrap();
    assert_eq!(wire["kind"], "notLoggedIn");
    assert_eq!(wire["nextAction"], "runFleetLogin");
}

#[tokio::test]
async fn machine_login_gate_precedes_cached_workspace_read_and_allows_it_after_login() {
    let fixture = Fixture::new().await;
    let request = WorkspaceShowRequest {
        workspace_id: fixture.workspace.workspace_id.clone(),
    };

    assert_not_logged_in(
        fixture
            .board
            .workspace_show(request.clone())
            .await
            .unwrap_err(),
    );
    fixture.state.send_replace(MachineLoginState::LoggedIn);

    assert_eq!(
        fixture
            .board
            .workspace_show(request.clone())
            .await
            .unwrap()
            .workspace,
        fixture.workspace
    );
    fixture.state.send_replace(MachineLoginState::NotLoggedIn);
    assert_not_logged_in(fixture.board.workspace_show(request).await.unwrap_err());
}

#[tokio::test]
async fn machine_login_gate_precedes_claim_dispatch_and_allows_the_unchanged_claim_reply_after_login()
 {
    let fixture = Fixture::new().await;
    let request = board_sync_protocol::ProjectClaimRequest {
        request_id: REQUEST.to_owned().try_into().unwrap(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: PROJECT.to_owned().try_into().unwrap(),
    };

    assert_not_logged_in(
        fixture
            .board
            .project_claim(request.clone())
            .await
            .unwrap_err(),
    );
    assert_eq!(fixture.claim.0.load(Ordering::SeqCst), 0);
    fixture.state.send_replace(MachineLoginState::LoggedIn);

    assert_eq!(
        fixture.board.project_claim(request.clone()).await.unwrap(),
        board_sync_protocol::ProjectClaimReply::Pending {
            attempt: request.request_id
        }
    );
    assert_eq!(fixture.claim.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn machine_login_gate_precedes_subscription_snapshot_and_allows_real_storage_after_login() {
    let fixture = Fixture::new().await;
    let request = ThreadSubscriptionsRequest { actor: actor() };

    assert_not_logged_in(
        fixture
            .board
            .thread_subscriptions(request.clone())
            .await
            .unwrap_err(),
    );
    fixture.state.send_replace(MachineLoginState::LoggedIn);

    let result = fixture.board.thread_subscriptions(request).await.unwrap();
    assert!(result.subscriptions.is_empty());
    assert!(result.unavailable.is_empty());
}
