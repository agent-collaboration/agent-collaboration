//! Kill the process with the native owner held; inspect before separately exercising recovery.
use super::*;
use durable_steps::{AtomicStore, CallFailure, StoreError};
use fleet_sync_protocol::{DeliveryEnvelope, ItemId, PeerId, ServerCall, ServerRequest};
use fleet_sync_tokio_runner::{
    CallEnvelope, CallRouter, ObjectKey, ObjectOptions, ObjectStore, TokioHost,
};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use runtime_primitives::SystemClock;
use serde::{Deserialize, Serialize};
use std::{os::unix::process::ExitStatusExt as _, process::Stdio, task::Poll};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{UnixListener, UnixStream},
};
use transport::WireEvent;

const CHILD_ROOT: &str = "CODEX_ROUTER_TEST_REMOTE_MAIL_CRASH_ROOT";
const CHILD_KIND: &str = "CODEX_ROUTER_TEST_REMOTE_MAIL_CRASH_KIND";
const CHILD_TEST: &str = "collaboration_runtime::board_composition::machine_session::mail_notice_events::harness::crash::remote_mail_crash_child_entry";

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum CrashKind {
    Notice,
    Publication,
    StatusBatch,
    FrontierRetirement,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CrashState {
    peer: PeerId,
    original_frames: Vec<Vec<u8>>,
    published_applied: bool,
    item: Option<ItemId>,
    status_acknowledged: u64,
}

struct NoReadbackCalls;
impl CallRouter for NoReadbackCalls {
    async fn dispatch(&self, _: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        Err(CallFailure::AnswerLost)
    }
}
type ReadbackHost = TokioHost<NoReadbackCalls, SystemClock>;
fn readback_host() -> Arc<ReadbackHost> {
    Arc::new(TokioHost::new(Arc::new(NoReadbackCalls), SystemClock))
}

async fn native_rows(
    root: &Path,
) -> Result<Vec<fleet_sync_delivery::test_support::DeliveryObservation>, TestError> {
    let owner = Arc::new(
        InteractionStore::open(root.join("interaction.sqlite"))
            .await
            .map_err(|error| io::Error::other(format!("native crash owning reopen: {error:?}")))?,
    );
    let store = InteractionAtomicStore::new(owner.clone(), readback_host());
    let rows = store
        .transaction(|scope| {
            Box::pin(async move {
                fleet_sync_delivery::test_support::snapshot(scope)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    drop(store);
    Arc::try_unwrap(owner)
        .map_err(|_| io::Error::other("native crash readback retained"))?
        .close()
        .await?;
    Ok(rows)
}

async fn origin_rows(
    base: &Path,
    peer: PeerId,
) -> Result<Vec<fleet_sync_delivery::test_support::DeliveryObservation>, TestError> {
    // Only after SIGKILL and wait: this is the owning reopen, never a parallel connection.
    let owners = ObjectStore::open(base.join("central")).await?;
    let object = owners
        .object(
            ObjectKey::new(format!("delivery-status-{}", peer.as_uuid()))?,
            ObjectOptions::default(),
            readback_host(),
        )
        .await?;
    let rows = object
        .transaction(|scope| {
            Box::pin(async move {
                fleet_sync_delivery::test_support::snapshot(scope)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    owners.close().await?;
    Ok(rows)
}

fn header(
    rows: &[fleet_sync_delivery::test_support::DeliveryObservation],
    item: ItemId,
) -> Result<serde_json::Value, TestError> {
    let row = rows
        .iter()
        .find(|row| row.category == "outbox" && row.identity == item.as_uuid().to_string())
        .ok_or("sender header missing")?;
    Ok(serde_json::from_str(
        row.content
            .as_deref()
            .ok_or("sender header content missing")?,
    )?)
}

async fn kill_child(kind: CrashKind) -> Result<(tempfile::TempDir, CrashState), TestError> {
    let directory = tempfile::Builder::new()
        .prefix("crmc-")
        .tempdir_in("/tmp")?;
    let listener = UnixListener::bind(directory.path().join("ready.sock"))?;
    let log = std::fs::File::create(directory.path().join("child.log"))?;
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(["--exact", CHILD_TEST, "--nocapture"])
        .env(CHILD_ROOT, directory.path())
        .env(CHILD_KIND, serde_json::to_string(&kind)?)
        .env("TMPDIR", directory.path())
        .stdin(Stdio::null())
        .stderr(log.try_clone()?)
        .stdout(log)
        .kill_on_drop(true)
        .spawn()?;
    let ready = tokio::time::timeout(BOUND, async {
        let (mut connection, _) = listener.accept().await?;
        let mut ready = [0];
        connection.read_exact(&mut ready).await?;
        Ok::<_, io::Error>(ready)
    })
    .await;
    child.kill().await?;
    let status = child.wait().await?;
    let log = std::fs::read_to_string(directory.path().join("child.log"))?;
    assert_eq!(
        status.signal(),
        Some(9),
        "child did not die at SIGKILL: {status}; {log}"
    );
    let ready = match ready {
        Ok(result) => result?,
        Err(error) => {
            let retained = directory.keep();
            return Err(io::Error::other(format!(
                "child never reached held native transaction: {error}; retained={}; {log}",
                retained.display()
            ))
            .into());
        }
    };
    assert_eq!(ready, *b"R");
    let state = serde_json::from_slice(&std::fs::read(directory.path().join("state.json"))?)?;
    Ok((directory, state))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn notice_ack_never_precedes_native_admission_at_sigkill_and_welcome_recovers()
-> Result<(), TestError> {
    let (directory, state) = kill_child(CrashKind::Notice).await?;
    let root = directory.path().join("router");
    let rows = native_rows(&root).await?;
    let inbox: serde_json::Value = serde_json::from_str(
        rows.iter()
            .find(|row| row.category == "inboxState")
            .and_then(|row| row.content.as_deref())
            .ok_or("baseline receive state missing")?,
    )?;
    let admitted_at_kill = inbox["items"]["rerunRequested"] == true;
    let acknowledged_at_kill = MachineSessionStore::open(&root)
        .await?
        .load()
        .await?
        .ok_or("durable client resume missing")?
        .notice_cursor
        .value();
    assert!(acknowledged_at_kill <= 1);
    assert!(
        !admitted_at_kill,
        "the held native writer did not hold the hint transaction"
    );

    // Replay the ORIGINAL SDK-generated bytes into the actual resumed client.
    // A prematurely committed transport ACK suppresses this hint in the SDK.
    let mut harness = MailHarness::open_manual(directory.path(), Some(state.peer)).await?;
    harness
        .transport
        .replay(state.original_frames.into_iter().map(Bytes::from).collect())
        .await?;
    harness
        .transport
        .observer
        .wait_for(
            |events| {
                events
                    .iter()
                    .any(|event| matches!(event, WireEvent::Notice(1)))
            },
            "original SDK frame was not replayed",
        )
        .await?;
    harness.release_welcome()?;
    let welcome = harness.next_manual_event().await?;
    assert!(matches!(welcome, ClientEvent::Welcome { .. }));
    harness.query(Vec::new()).await?; // Same pump command barrier after the replayed frame.
    let mut events = vec![welcome];
    let mut replayed_hint = false;
    while let Ok(event) = harness
        .manual_events
        .as_mut()
        .ok_or("manual events missing")?
        .try_recv()
    {
        replayed_hint |=
            matches!(&event, ClientEvent::Notice { position, .. } if position.value() == 1);
        events.push(event);
    }
    assert_eq!(
        replayed_hint,
        acknowledged_at_kill == 0,
        "SDK replay must follow the durable ACK prefix"
    );
    // Recovery is a separate proof: pass the real Welcome and any original hint
    // to the ordinary Host event loop, even if the hint was already suppressed.
    harness.resume_events(events).await?;
    harness
        .wait_state(
            |inbox| {
                inbox["items"]["rerunRequested"] == true
                    && inbox["controls"]["rerunRequested"] == true
            },
            "normal Welcome did not recover both receive streams",
        )
        .await?;
    harness.wait_ack(NoticePosition::new(1)).await?;
    harness.close().await?;
    assert!(
        acknowledged_at_kill == 0 || admitted_at_kill,
        "durable notice ACK advanced before native admission at SIGKILL"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn canonical_publication_stays_owed_until_native_commit_at_sigkill_then_catch_up_repairs()
-> Result<(), TestError> {
    let (directory, state) = kill_child(CrashKind::Publication).await?;
    let item = state.item.ok_or("crash publication item missing")?;
    let rows = native_rows(&directory.path().join("router")).await?;
    assert_eq!(header(&rows, item)?["value"]["status"]["kind"], "queued");
    let origin = origin_rows(directory.path(), state.peer).await?;
    let owed_at_kill = origin.iter().any(|row| row.category == "publication" && row.identity == item.as_uuid().to_string()
        && row.metadata.as_deref().and_then(|target| serde_json::from_str::<fleet_sync_delivery::PublicationTarget<message_board::ServiceId>>(target).ok()).is_some_and(|target| matches!(target, fleet_sync_delivery::PublicationTarget::Peer { peer, .. } if peer == state.peer)));
    // The held native writer prevents both the original live application and
    // its cursor from committing. After owning readback, exercise repair alone.
    let mut harness = MailHarness::open_manual(directory.path(), Some(state.peer)).await?;
    harness.mail.mail.catch_up_status().await?;
    assert_eq!(harness.mail.status_cursor().await?, 1);
    let repaired = header(&harness.mail.rows().await?, item)?;
    assert_eq!(repaired["value"]["status"]["kind"], "sent");
    let events = harness.transport.observer.events();
    assert!(events.iter().any(|event| matches!(
        event,
        WireEvent::StatusFetched {
            through: 1,
            events: 1
        }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WireEvent::StatusAcked { floor: 1 }))
    );
    harness.release_welcome()?;
    harness.close().await?;
    assert!(
        !state.published_applied,
        "Published(Applied) reached the real wire before native commit at SIGKILL"
    );
    assert!(
        owed_at_kill,
        "central retired the final peer publication while native status was uncommitted at SIGKILL"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn missing_native_publication_owner_never_answers_applied_or_retires_the_owed_status()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut harness = MailHarness::open_manual(directory.path(), None).await?;
    let peer = harness.peer();
    let item = relay_items::ordinary_message(peer);
    let item_id = item.id();
    let envelope = serde_json::from_value(serde_json::to_value(&item)?)?;
    harness.mail.enqueue(item, harness.relay).await?;
    assert!(matches!(
        harness
            .submit_generic(DeliveryEnvelope::Item(envelope))
            .await?,
        fleet_sync_protocol::SubmitResult::Accepted { .. }
    ));
    harness.release_welcome()?;
    assert!(matches!(
        harness.next_manual_event().await?,
        ClientEvent::Welcome { .. }
    ));
    let ClientEvent::Request {
        frame,
        body: body @ ServerRequest::Crate(ServerCall::PublishStatus { .. }),
    } = harness.next_manual_event().await?
    else {
        return Err("expected actual central canonical publication".into());
    };
    let result = answer_server_call(
        harness.projects.clone(),
        Weak::new(),
        Weak::new(),
        Weak::new(),
        harness.requests.clone(),
        frame,
        body,
    )
    .await;
    harness.query(Vec::new()).await?;
    let applied = harness
        .transport
        .observer
        .events()
        .iter()
        .any(|event| matches!(event, WireEvent::Published { applied: true, .. }));
    harness.close().await?;
    let rows = native_rows(&directory.path().join("router")).await?;
    assert_eq!(header(&rows, item_id)?["value"]["status"]["kind"], "queued");
    let origin = origin_rows(directory.path(), peer).await?;
    assert!(
        result.is_err(),
        "missing native publication owner was reported as success"
    );
    assert!(
        !applied,
        "missing native publication owner answered Applied"
    );
    assert!(
        origin
            .iter()
            .any(|row| row.category == "publication"
                && row.identity == item_id.as_uuid().to_string()),
        "missing owner retired the owed publication"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn remote_mail_crash_child_entry() -> Result<(), TestError> {
    let Some(base) = std::env::var_os(CHILD_ROOT).map(PathBuf::from) else {
        return Ok(());
    };
    let kind: CrashKind = serde_json::from_str(&std::env::var(CHILD_KIND)?)?;
    if matches!(kind, CrashKind::FrontierRetirement) {
        return Box::pin(retirement::retirement_checkpoint_child(&base)).await;
    }
    if matches!(kind, CrashKind::StatusBatch) {
        return Box::pin(status_atomicity::status_batch_child(&base)).await;
    }
    let mut harness = MailHarness::open_manual(&base, None).await?;
    let mut state = CrashState {
        peer: harness.peer(),
        original_frames: Vec::new(),
        published_applied: false,
        item: None,
        status_acknowledged: 0,
    };
    if matches!(kind, CrashKind::Publication) {
        let item = relay_items::ordinary_message(harness.peer());
        let envelope = serde_json::from_value(serde_json::to_value(&item)?)?;
        state.item = Some(item.id());
        harness.mail.enqueue(item, harness.relay).await?;
        assert!(matches!(
            harness
                .submit_generic(DeliveryEnvelope::Item(envelope))
                .await?,
            fleet_sync_protocol::SubmitResult::Accepted { .. }
        ));
    }
    harness.release_welcome()?;
    assert!(matches!(
        harness.next_manual_event().await?,
        ClientEvent::Welcome { .. }
    ));
    harness.welcomed.send_replace(true);
    if matches!(kind, CrashKind::Notice) {
        harness.mail.mail.prompt_receive_all().await?;
        let frames = harness.ledger.publish(1, DeliveryStream::Items).await?;
        state.original_frames = frames.iter().map(|frame| frame.to_vec()).collect();
        assert_eq!(
            state.original_frames.len(),
            1,
            "expected one actual SDK notice frame"
        );
        harness.transport.replay(frames).await?;
    }
    let event = harness.next_manual_event().await?;
    let _held_writer = harness.mail.hold_writer().await?;
    let mut notices = MachineNoticeReactions::open(&harness.root).await?;
    let owners = MachineSessionEventOwners {
        projects: harness.projects.clone(),
        admission: Weak::new(),
        fleet: Weak::new(),
        publisher: Weak::new(),
        mail: Arc::downgrade(&harness.mail.mail),
    };
    let mut receipts = NoticeReceipts::default();
    let handling = async {
        match (kind, event) {
            (CrashKind::Notice, ClientEvent::Notice { position, body }) => {
                assert!(receipts.begin(position));
                let outcome = notice_dispatch::handle_notice(
                    &mut notices,
                    &owners.projects,
                    &owners.admission,
                    &owners.fleet,
                    &owners.mail,
                    &harness.requests,
                    body,
                )
                .await;
                receipts.complete(position, outcome.is_ok());
                acknowledge_handled_prefix(&harness.requests, &mut receipts).await;
                outcome
            }
            (
                CrashKind::Publication,
                ClientEvent::Request {
                    frame,
                    body: body @ ServerRequest::Crate(ServerCall::PublishStatus { .. }),
                },
            ) => {
                answer_server_call(
                    harness.projects.clone(),
                    Weak::new(),
                    Arc::downgrade(&harness.mail.mail),
                    Weak::new(),
                    harness.requests.clone(),
                    frame,
                    body,
                )
                .await
            }
            _ => Err(io::Error::other("unexpected actual crash event")),
        }
    };
    tokio::pin!(handling);
    // Poll the REAL handler at its first pending boundary, then flush commands
    // already enqueued by it through the same real SessionRequests pump.
    std::future::poll_fn(|cx| {
        assert!(
            handling.as_mut().poll(cx).is_pending(),
            "handler escaped the held native writer"
        );
        Poll::Ready(())
    })
    .await;
    harness.query(Vec::new()).await?;
    std::future::poll_fn(|cx| {
        assert!(
            handling.as_mut().poll(cx).is_pending(),
            "handler completed before native commit"
        );
        Poll::Ready(())
    })
    .await;
    state.published_applied = harness
        .transport
        .observer
        .events()
        .iter()
        .any(|event| matches!(event, WireEvent::Published { applied: true, .. }));
    std::fs::write(base.join("state.json"), serde_json::to_vec(&state)?)?;
    let mut ready = UnixStream::connect(base.join("ready.sock")).await?;
    ready.write_all(b"R").await?;
    std::future::pending::<Result<(), TestError>>().await
}

#[path = "mail_status_atomicity.rs"]
mod status_atomicity;

#[path = "notification_retirement_crash.rs"]
mod retirement;
