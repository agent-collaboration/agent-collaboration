//! SIGKILL interrupts the actual project producer between SDK and epoch writes.
use super::*;
use durable_steps::AtomicStore;
use durable_steps::storage::JournalStorage as _;
use fleet_sync_delivery::{Outbox, OutboxStatusState};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use serde::{Deserialize, Serialize};
use std::{os::unix::process::ExitStatusExt as _, path::PathBuf, process::Stdio, sync::Arc};
use tokio::{io::AsyncReadExt as _, net::UnixListener};
use tracing::instrument::WithSubscriber;

const CHILD_ROOT: &str = "CODEX_ROUTER_TEST_PROJECT_OUTBOX_CRASH_ROOT";
const CHILD_TEST: &str = "mail_push_delivery::tests::remote_assembly::project_admission::crash::project_outbox_crash_child_entry";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CrashState {
    path: PathBuf,
    item: ItemId,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProjectCrashKind {
    Original,
    Cancel,
}

#[tokio::test]
async fn sigkill_between_sdk_and_epoch_writes_preserves_neither_after_owning_reopen() {
    assert_atomic_project_admission(ProjectCrashKind::Original).await;
}
#[tokio::test]
async fn sigkill_between_cancel_custody_and_epoch_writes_preserves_neither_after_reopen() {
    assert_atomic_project_admission(ProjectCrashKind::Cancel).await;
}
async fn assert_atomic_project_admission(kind: ProjectCrashKind) {
    let root = tempfile::Builder::new()
        .prefix("crpe-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(
        root.path().join("kind.json"),
        serde_json::to_vec(&kind).unwrap(),
    )
    .unwrap();
    let listener = UnixListener::bind(root.path().join("ready.sock")).unwrap();
    let log = std::fs::File::create(root.path().join("child.log")).unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD_TEST, "--nocapture"])
        .env(CHILD_ROOT, root.path())
        .env("TMPDIR", root.path())
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(15), async {
        let (mut connection, _) = listener.accept().await?;
        let mut byte = [0_u8; 1];
        connection.read_exact(&mut byte).await?;
        std::io::Result::Ok(byte)
    })
    .await;
    child.kill().await.unwrap();
    let status = child.wait().await.unwrap();
    assert_eq!(
        status.signal(),
        Some(9),
        "child did not reach the owned transaction: {status}"
    );
    assert_eq!(
        ready
            .expect("actual project transaction did not signal readiness")
            .unwrap(),
        *b"R",
        "{}",
        std::fs::read_to_string(root.path().join("child.log")).unwrap()
    );
    let state: CrashState =
        serde_json::from_slice(&std::fs::read(root.path().join("state.json")).unwrap()).unwrap();
    assert!(state.path.starts_with(root.path()));

    let diagnostics = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .finish();
    let owner = Arc::new(
        InteractionStore::open(&state.path)
            .with_subscriber(diagnostics)
            .await
            .expect("cold reopen after owned crash; startup phase and driver code are logged"),
    );
    assert!(
        owner
            .mail_submission_epoch(&mail_id(state.item))
            .await
            .unwrap()
            .is_none(),
        "uncommitted epoch survived SIGKILL"
    );
    let fixture = crate::mail_delivery_projection_tests::fixture::Fixture::new().await;
    let limits = fixture.host.config.local.limits;
    let binding = fixture.host.binding.clone();
    let store = InteractionAtomicStore::new(owner.clone(), fixture.host.clone());
    let (status, pending) = store
        .transaction(move |scope| {
            Box::pin(async move {
                let status = Outbox::<_, collaboration_contracts::RouterDeliveryTypes, _>::new(
                    scope,
                    limits,
                    binding.as_ref(),
                )
                .status(state.item)
                .await
                .map_err(durable_steps::StoreError::backend)?;
                let pending = scope
                    .due(runtime_primitives::SyncTime::from_unix_millis(i64::MAX))
                    .await?;
                Ok((status, pending))
            })
        })
        .await
        .unwrap();
    assert!(
        matches!(status.state, OutboxStatusState::Unknown),
        "SDK custody committed before its epoch association"
    );
    assert!(
        pending.is_empty(),
        "uncommitted project admission left a runnable SDK operation"
    );
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("crash owner retained"))
        .close()
        .await
        .unwrap();
    drop(fixture.close().await);
}
