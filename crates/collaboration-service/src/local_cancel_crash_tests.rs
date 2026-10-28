//! A real process dies with the actual Local producer transaction open.
use super::*;
use durable_steps::AtomicStore;
use fleet_sync_delivery::{Inbox, delivery_rollback};
use fleet_sync_protocol::PeerId;
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use serde::{Deserialize, Serialize};
use std::{os::unix::process::ExitStatusExt as _, path::PathBuf, process::Stdio, sync::Arc};
use tokio::{io::AsyncReadExt as _, net::UnixListener};
use tracing::instrument::WithSubscriber;

const CHILD_ROOT: &str = "CODEX_ROUTER_TEST_CANCEL_CRASH_ROOT";
const CHILD_TEST: &str =
    "mail_push_delivery::tests::local_runtime::crash::local_cancel_crash_child_entry";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CrashState {
    path: PathBuf,
    owner: PeerId,
    item: ItemId,
    target: ItemId,
}

#[tokio::test]
async fn crash_between_attribution_and_sdk_admission_leaves_neither_after_reopen() {
    // Arrange a child with private data and an operation-owned readiness endpoint.
    let root = tempfile::Builder::new()
        .prefix("crcc-")
        .tempdir_in("/tmp")
        .unwrap();
    let listener = UnixListener::bind(root.path().join("ready.sock")).unwrap();
    let log = std::fs::File::create(root.path().join("child.log")).unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD_TEST, "--nocapture"])
        .env(CHILD_ROOT, root.path())
        .env("TMPDIR", root.path())
        .stdin(Stdio::null())
        .stderr(log.try_clone().unwrap())
        .stdout(log)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let readiness = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let (mut connection, _) = listener.accept().await?;
        let mut ready = [0_u8; 1];
        connection.read_exact(&mut ready).await?;
        std::io::Result::Ok(ready)
    })
    .await;
    // Act: SIGKILL while the scope contains the real uncommitted attribution row.
    child.kill().await.unwrap();
    let status = child.wait().await.unwrap();
    assert_eq!(
        status.signal(),
        Some(9),
        "child was not killed while blocked: {status}"
    );
    assert_eq!(
        readiness
            .expect("child never reached its actual open transaction")
            .unwrap(),
        *b"R",
        "{}",
        std::fs::read_to_string(root.path().join("child.log")).unwrap()
    );
    let state: CrashState =
        serde_json::from_slice(&std::fs::read(root.path().join("state.json")).unwrap()).unwrap();
    assert!(
        state.path.starts_with(root.path()),
        "child data escaped its owning test directory"
    );
    // Assert through the native owner after the killed process has fully exited.
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
            .local_cancel_delivery(&mail_id(state.item))
            .await
            .unwrap()
            .is_none(),
        "uncommitted requester association survived the crash"
    );
    // Use the public SDK with the same ID and changed target. A committed earlier
    // custody row would refuse ItemConflict instead of accepting this fresh control.
    let fixture = crate::mail_delivery_projection_tests::fixture::Fixture::new().await;
    let store = InteractionAtomicStore::new(owner.clone(), fixture.host.clone());
    let mut config = fixture.host.config.local;
    config.owner = state.owner;
    let consumer = fixture.host.consumer.clone();
    let template = fixture.item(&fixture.draft());
    let target = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    assert_ne!(target, state.target);
    let mut recipient = template.recipient().clone();
    recipient.peer = state.owner;
    let replacement = DeliveryCancel::<RouterDeliveryTypes>::new(
        state.item,
        template.sender().clone(),
        recipient,
        None,
        CancelBody { target },
    )
    .unwrap();
    let admitted = store
        .transaction(move |scope| {
            Box::pin(async move {
                Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                    .enqueue_local_cancel(
                        replacement,
                        &*consumer,
                        runtime_primitives::SyncTime::from_unix_millis(
                            recorded_at().timestamp_millis(),
                        ),
                    )
                    .await
                    .map_err(delivery_rollback)
            })
        })
        .await
        .unwrap();
    assert_eq!(admitted.reference.item, state.item);
    assert!(admitted.effects.attempts.is_empty());
    assert!(admitted.effects.reports.is_empty());
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("crash observation owner retained"))
        .close()
        .await
        .unwrap();
    drop(fixture.close().await);
}

#[tokio::test]
async fn local_cancel_crash_child_entry() {
    let Some(root) = std::env::var_os(CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    let runtime = Runtime::new().await;
    let mut mail = runtime.local_mail_runtime();
    mail.cancel_admission_probe = Some(root.join("ready.sock"));
    let draft = runtime.fixture.draft();
    let item = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let state = CrashState {
        path: runtime.fixture.database_path(),
        owner: runtime.fixture.host.config.local.owner,
        item,
        target: runtime.fixture.item(&draft).id(),
    };
    std::fs::write(root.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
    let result = mail
        .cancel_local(
            cancel_for(&runtime, &draft, item),
            requester("uncommitted requester"),
        )
        .await;
    // The parent must kill this process; ordinary error/return is not crash proof.
    panic!("cancel crash child unexpectedly returned: {result:?}");
}
