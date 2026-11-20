//! A Host process killed while it delivers a direct message keeps the message's single
//! native submission: after restart the message is settled from what the store knows and is
//! never submitted again.
//!
//! The real delivery service, target actor and interaction store run in a child process;
//! only the native client stands in. It writes one witness line per submission it receives.
use crate::BoardAvailability;
use crate::SubscriptionDeliveryService;
use crate::SubscriptionDeliveryServiceProps;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_contracts::layer_zero::DeliveryRequest;
use collaboration_protocol::{
    DeliveryClientReceipt, DeliveryOutcome, DeliveryReceipt, MessageDelivery, OperationId,
    PushHeaderFacts, PushId, PushKind, PushOrigin, PushRecord, PushRecordDraft,
    SessionReachability, SessionRef,
};
use interaction_storage::InteractionStore;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{UnixListener, UnixStream};

const CHILD_ROOT: &str = "CODEX_ROUTER_TEST_DM_PROCESS_LOSS_ROOT";
const CHILD_STAGE: &str = "CODEX_ROUTER_TEST_DM_PROCESS_LOSS_STAGE";
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const FIXED_MILLIS: i64 = 1_792_432_800_000;

/// Where the child is killed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LossPoint {
    /// The native client accepted the submission; the receipt was never stored.
    AfterNativeAcceptance,
    /// The receipt was stored and the delivery call returned.
    AfterSettlement,
}

impl LossPoint {
    fn name(self) -> &'static str {
        match self {
            Self::AfterNativeAcceptance => "after-native-acceptance",
            Self::AfterSettlement => "after-settlement",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Self::AfterNativeAcceptance, Self::AfterSettlement]
            .into_iter()
            .find(|point| point.name() == name)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildState {
    database: PathBuf,
    target: SessionRef,
    push_id: PushId,
}

/// One submission the native client received.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NativeSubmission {
    push_id: PushId,
    correlation: String,
}

fn child_selector() -> String {
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, path)| path);
    format!("{module}::direct_message_process_loss_child_entry")
}

fn fixed_clock() -> Arc<router_clock::ManualClock> {
    Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            FIXED_MILLIS,
        ))
        .expect("fixed delivery clock"),
    )
}

fn session(session_id: &str) -> SessionRef {
    serde_json::from_value(serde_json::json!({
        "endpoint": {"serviceId": SERVICE, "endpointId": "codex-local"},
        "sessionId": session_id
    }))
    .expect("session reference")
}

/// A started turn whose receipt carries evidence beyond its outcome tag: a reachability and a
/// client receipt, so losing either field after restart changes the stored record.
fn started() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::Started,
        reachability: Some(SessionReachability::ProviderAcp),
        client: Some(DeliveryClientReceipt::ProviderAcp {
            operation_id: OperationId::try_from("019f0000-0000-7000-8000-0000000000b1".to_owned())
                .expect("receipt operation id"),
        }),
    }
}

fn service_over(
    store: Arc<InteractionStore>,
    delivery: Arc<dyn SessionMessageDelivery>,
) -> SubscriptionDeliveryService {
    SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: store,
        delivery,
        presence: Arc::new(RunningPresence),
        machine_identity: MachineIdentity::new(
            SERVICE.to_owned().try_into().expect("service id"),
            Some("Process loss proof"),
        )
        .expect("machine identity"),
        clock: fixed_clock(),
    })
}

struct RunningPresence;
impl TargetPresenceProbe for RunningPresence {
    fn presence(&self, _: &SessionRef) -> DeliveryFuture<'_, TargetPresence> {
        Box::pin(async { Ok(TargetPresence::Running) })
    }
}

/// The child's native client: it records each submission, then either holds the call open
/// for the kill or answers that the turn started.
struct WitnessingClient {
    witness: PathBuf,
    ready: PathBuf,
    loss_point: LossPoint,
}

impl SessionMessageDelivery for WitnessingClient {
    fn supports_delivery_mode(&self, _: &SessionRef, _: MessageDelivery) -> bool {
        true
    }
    fn deliver<'a>(
        &'a self,
        request: DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        Box::pin(async move {
            let line = serde_json::to_string(&NativeSubmission {
                push_id: request.payload.push_id.clone(),
                correlation: String::from(request.correlation),
            })
            .map_err(|_| DeliveryContractError::ClientOperation)?;
            let mut witness = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.witness)
                .map_err(|_| DeliveryContractError::ClientOperation)?;
            writeln!(witness, "{line}").map_err(|_| DeliveryContractError::ClientOperation)?;
            witness
                .sync_all()
                .map_err(|_| DeliveryContractError::ClientOperation)?;
            if self.loss_point == LossPoint::AfterNativeAcceptance {
                signal_ready(&self.ready).await;
                std::future::pending::<()>().await;
            }
            Ok(collaboration_contracts::scripted_delivery_result(started()))
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}

/// The restarted process's native client: any submission it receives is a resend.
#[derive(Default)]
struct ResendDetector {
    submissions: Mutex<Vec<PushId>>,
}

impl SessionMessageDelivery for ResendDetector {
    fn supports_delivery_mode(&self, _: &SessionRef, _: MessageDelivery) -> bool {
        true
    }
    fn deliver<'a>(
        &'a self,
        request: DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        Box::pin(async move {
            self.submissions
                .lock()
                .map_err(|_| DeliveryContractError::ClientOperation)?
                .push(request.payload.push_id);
            Ok(collaboration_contracts::scripted_delivery_result(started()))
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}

async fn signal_ready(ready: &Path) {
    let mut connection = UnixStream::connect(ready)
        .await
        .expect("parent readiness listener");
    connection.write_all(b"R").await.expect("readiness signal");
    connection.flush().await.expect("readiness flush");
}

fn direct_message(target: &SessionRef) -> PushRecordDraft {
    serde_json::from_value(serde_json::json!({
        "pushId": PushId::try_from(uuid::Uuid::now_v7().to_string()).expect("push id"),
        "kind": PushKind::DirectMessage,
        "origin": PushOrigin::Session(session("sender")),
        "originRouterRef": null,
        "target": target,
        "mode": MessageDelivery::Auto,
        "guard": null,
        "replyToPushId": null,
        "headerFacts": PushHeaderFacts::DirectMessage { sender_display_name: None },
        "body": "deliver this exactly once",
        "activity": null,
        "createdAt": chrono::DateTime::<chrono::Utc>::from_timestamp_millis(FIXED_MILLIS)
            .expect("fixed creation time"),
    }))
    .expect("direct message draft")
}

/// Runs the child until `loss_point`, kills it, and returns its state and witness lines.
async fn deliver_then_kill(
    loss_point: LossPoint,
) -> (tempfile::TempDir, ChildState, Vec<NativeSubmission>) {
    let root = tempfile::Builder::new()
        .prefix("dmpl-")
        .tempdir_in("/tmp")
        .expect("private child directory");
    let listener = UnixListener::bind(root.path().join("ready.sock")).expect("readiness socket");
    let log = std::fs::File::create(root.path().join("child.log")).expect("child log");
    let mut child = tokio::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", &child_selector(), "--ignored", "--nocapture"])
        .env(CHILD_ROOT, root.path())
        .env(CHILD_STAGE, loss_point.name())
        .stdin(Stdio::null())
        .stderr(log.try_clone().expect("child log"))
        .stdout(log)
        .kill_on_drop(true)
        .spawn()
        .expect("child process");
    let readiness = tokio::time::timeout(Duration::from_secs(15), async {
        let (mut connection, _) = listener.accept().await?;
        let mut ready = [0_u8; 1];
        connection.read_exact(&mut ready).await?;
        std::io::Result::Ok(ready)
    })
    .await;
    child.kill().await.expect("kill child");
    let status = child.wait().await.expect("child exit");
    let child_log = std::fs::read_to_string(root.path().join("child.log")).unwrap_or_default();
    // The child's success signal is its readiness byte: a selector that matched nothing
    // exits without ever sending it.
    assert_eq!(
        readiness
            .unwrap_or_else(|_| panic!(
                "{}: child never reached its kill point: {child_log}",
                loss_point.name()
            ))
            .expect("readiness read"),
        *b"R",
        "{child_log}"
    );
    assert_eq!(
        status.signal(),
        Some(9),
        "{}: child was not killed at its kill point: {status}; {child_log}",
        loss_point.name()
    );
    let state: ChildState = serde_json::from_slice(
        &std::fs::read(root.path().join("state.json")).expect("child state"),
    )
    .expect("child state json");
    assert!(
        state.database.starts_with(root.path()),
        "child data escaped its owning test directory"
    );
    let submissions = std::fs::read_to_string(root.path().join("native-submissions.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).expect("witness line"))
        .collect();
    (root, state, submissions)
}

/// What the restarted process shows for the message.
struct AfterRestart {
    /// The record the restarted delivery answered when asked for the message again.
    answered: PushRecord,
    /// The record read back from the store after that answer.
    stored: PushRecord,
    /// Every submission the restarted process made; any is a resend.
    resent: Vec<PushId>,
}

/// Restarts delivery over the child's store, asks for the same message again, and returns what
/// the restarted process answered, stored and submitted.
async fn restart_and_ask_again(state: &ChildState) -> AfterRestart {
    let store = Arc::new(
        InteractionStore::open(&state.database)
            .await
            .expect("cold reopen after the kill"),
    );
    let detector = Arc::new(ResendDetector::default());
    let service = service_over(Arc::clone(&store), detector.clone());
    service.start().await.expect("restarted delivery starts");
    let answered = tokio::time::timeout(
        Duration::from_secs(15),
        service.deliver_direct_message(state.target.clone(), state.push_id.clone()),
    )
    .await
    .expect("delivery answer deadline")
    .expect("stored direct message");
    service.shutdown().await;
    let stored = store
        .get_push_record(&state.push_id)
        .await
        .expect("stored record read")
        .expect("stored record present");
    drop(service);
    Arc::try_unwrap(store)
        .unwrap_or_else(|_| panic!("restarted store retained"))
        .close()
        .await
        .expect("store closes");
    let resent = detector
        .submissions
        .lock()
        .expect("detector submissions")
        .clone();
    AfterRestart {
        answered,
        stored,
        resent,
    }
}

#[tokio::test]
async fn a_direct_message_whose_process_dies_after_native_acceptance_is_unknown_and_never_resent() {
    // Arrange and act: the child's native client takes the submission; the child dies
    // before the receipt is stored.
    let (_root, state, submissions) = deliver_then_kill(LossPoint::AfterNativeAcceptance).await;

    // Assert: exactly one submission, correlated to the message's push id.
    assert_eq!(
        submissions,
        vec![NativeSubmission {
            push_id: state.push_id.clone(),
            correlation: state.push_id.as_str().to_owned(),
        }]
    );
    // After restart the store cannot know whether the turn ran: the message is Unknown,
    // and asking for it again neither resubmits nor invents an acceptance.
    let after = restart_and_ask_again(&state).await;
    let outcome = after
        .answered
        .last_outcome
        .expect("a settled direct message has an outcome")
        .outcome;
    assert_eq!(outcome, DeliveryOutcome::Unknown);
    let resent = after.resent;
    assert!(
        resent.is_empty(),
        "the restarted Host resubmitted {resent:?}"
    );
}

#[tokio::test]
async fn a_direct_message_whose_process_dies_after_settlement_keeps_its_receipt_and_is_never_resent()
 {
    // Arrange and act: the child stores the started receipt, records the settled message it
    // was answered with, then dies.
    let (root, state, submissions) = deliver_then_kill(LossPoint::AfterSettlement).await;
    let settled: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.path().join("settled-record.json")).expect("settled record"),
    )
    .expect("settled record json");
    // The receipt carries evidence beyond its tag, so a lost field would show.
    assert_eq!(
        settled["lastOutcome"]["outcome"]["kind"], "started",
        "{settled}"
    );
    assert!(
        !settled["lastOutcome"]["reachability"].is_null(),
        "{settled}"
    );
    assert!(!settled["lastOutcome"]["client"].is_null(), "{settled}");
    assert!(!settled["settledAt"].is_null(), "{settled}");

    // Assert: one submission; after restart both the replayed answer and the stored message equal
    // the settled message field for field, and nothing is submitted again.
    assert_eq!(submissions.len(), 1, "{submissions:?}");
    assert_eq!(submissions[0].push_id, state.push_id);
    let after = restart_and_ask_again(&state).await;
    assert_eq!(
        serde_json::to_value(&after.answered).expect("answered json"),
        settled,
        "the replayed answer differs from the settled message"
    );
    assert_eq!(
        serde_json::to_value(&after.stored).expect("stored json"),
        settled,
        "recovery rewrote the settled message"
    );
    let resent = after.resent;
    assert!(
        resent.is_empty(),
        "the restarted Host resubmitted {resent:?}"
    );
}

#[tokio::test]
#[ignore = "the process-loss child; its parent re-executes it with --ignored"]
async fn direct_message_process_loss_child_entry() {
    let (Some(root), Some(stage)) = (
        std::env::var_os(CHILD_ROOT).map(PathBuf::from),
        std::env::var(CHILD_STAGE).ok(),
    ) else {
        return;
    };
    let loss_point = LossPoint::parse(&stage).expect("known loss point");
    let database = root.join("interaction.sqlite");
    let store = Arc::new(
        InteractionStore::open(&database)
            .await
            .expect("child interaction store"),
    );
    let target = session("dm-target");
    let draft = direct_message(&target);
    let push_id = draft.push_id.clone();
    store
        .insert_push_record(draft)
        .await
        .expect("stored direct message");
    std::fs::write(
        root.join("state.json"),
        serde_json::to_vec(&ChildState {
            database,
            target: target.clone(),
            push_id: push_id.clone(),
        })
        .expect("state json"),
    )
    .expect("child state");
    let ready = root.join("ready.sock");
    let service = service_over(
        store,
        Arc::new(WitnessingClient {
            witness: root.join("native-submissions.jsonl"),
            ready: ready.clone(),
            loss_point,
        }),
    );
    service.start().await.expect("child delivery starts");
    let delivered = service.deliver_direct_message(target, push_id).await;
    if loss_point == LossPoint::AfterSettlement {
        let settled = delivered.as_ref().expect("settled direct message");
        std::fs::write(
            root.join("settled-record.json"),
            serde_json::to_vec(settled).expect("settled record json"),
        )
        .expect("settled record");
        signal_ready(&ready).await;
        std::future::pending::<()>().await;
    }
    // The parent must kill this process; returning is not process loss.
    panic!("process loss child returned at {stage}: {delivered:?}");
}
