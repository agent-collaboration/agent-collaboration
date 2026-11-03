//! Real pump/event/inbox proofs. Fleet producer policy is observed at the C4+C5 gate.
use super::*;
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{CarrierAcceptance, MailDeliveryMode, MailSender, MessageBody};
use fleet_sync_protocol::{
    CallId, DeliveryEnvelope, DeliveryItem, DeliveryStatus, FrameId, ItemId, PeerId, PushResult,
    Recipient, ServerCall, ServerFrame, ServerRequest,
};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::{fs::PermissionsExt, process::ExitStatusExt},
    process::Stdio,
};
use tokio::io::AsyncReadExt;
use transport::WireEvent;

const CHILD_ROOT: &str = "CODEX_ROUTER_TEST_PUSH_CRASH_ROOT";
const CHILD_TEST: &str = "collaboration_runtime::board_composition::machine_session::mail_notice_events::pushed_mail::pushed_mail_crash_child";

fn message(
    id: ItemId,
    peer: PeerId,
    text: &str,
) -> Result<DeliveryEnvelope<BoardDeliveryTypes>, TestError> {
    let address: message_board::SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"pushed-recipient"
    }))?;
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        id,
        MailSender::HumanUser {
            installation: address.endpoint.service_id.clone(),
        },
        Recipient { peer, address },
        None,
        BoardDeliveryBody::Message(MessageBody {
            text: text.to_owned().try_into()?,
            delivery: MailDeliveryMode::Queue,
            guard: None,
            sender_display_name: None,
            reply_to: None,
        }),
    )?))
}

pub(super) fn push_frame(
    item: DeliveryEnvelope<BoardDeliveryTypes>,
) -> Result<(FrameId, Vec<u8>), TestError> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let call = CallId {
        operation: runtime_primitives::OperationId::new(durable_steps::Uuid::now_v7())?,
        step: runtime_primitives::StepName::new("push")?,
    };
    Ok((
        frame,
        serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Request {
            frame,
            body: ServerRequest::Crate(ServerCall::Push { call, item }),
        })?,
    ))
}

pub(super) async fn push(
    harness: &MailHarness,
    item: DeliveryEnvelope<BoardDeliveryTypes>,
) -> Result<PushResult<CarrierAcceptance>, TestError> {
    let (frame, bytes) = push_frame(item)?;
    harness.transport.replay(vec![bytes.into()]).await?;
    let events = harness.transport.observer.wait_for(
        |events| events.iter().any(|event| matches!(event, WireEvent::Pushed { frame: actual, .. } if *actual == frame)),
        "Push did not produce its correlated receipt",
    ).await?;
    events
        .into_iter()
        .find_map(|event| match event {
            WireEvent::Pushed {
                frame: actual,
                result,
            } if actual == frame => Some(result),
            _ => None,
        })
        .ok_or_else(|| "Push receipt disappeared".into())
}

pub(super) async fn welcome(harness: &mut MailHarness) -> Result<(), TestError> {
    harness.release_welcome()?;
    let mut welcomed = harness.welcomed.subscribe();
    tokio::time::timeout(BOUND, welcomed.wait_for(|ready| *ready)).await??;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn welcome_and_legacy_mail_hints_do_not_admit_recipient_fetch_work() -> Result<(), TestError>
{
    // Arrange a real pre-Welcome legacy hint; the runtime must receive custody only by Push.
    let mut harness = MailHarness::open(true).await?;
    assert!(harness.mail.state().await?.is_none());
    // Act through the actual pump and Host's deferred-notice path.
    welcome(&mut harness).await?;
    harness.wait_ack(NoticePosition::new(1)).await?;
    // Assert the hint's handling commits its cursor without a Fetch admission.
    assert!(
        harness.mail.state().await?.is_none(),
        "Welcome or MailWaiting admitted obsolete receive cycles"
    );
    assert!(
        !harness
            .transport
            .observer
            .events()
            .iter()
            .any(|event| matches!(event, WireEvent::Fetch { .. } | WireEvent::Received(_)))
    );
    harness.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn push_receipts_follow_committed_custody_and_replays_preserve_the_original()
-> Result<(), TestError> {
    // Arrange a live actual session/event owner; leave only the native delivery worker inactive.
    let mut harness = MailHarness::open(false).await?;
    welcome(&mut harness).await?;
    let item = ItemId::new(durable_steps::Uuid::now_v7())?;
    let envelope = message(item, harness.peer(), "one pushed item")?;
    let digest = envelope.digest().clone();
    // Act through a server frame and the real pump, then inspect the committed physical owner.
    assert!(matches!(
        push(&harness, envelope).await?,
        PushResult::Accepted { .. }
    ));
    let push_id: collaboration_protocol::PushId = item.as_uuid().to_string().try_into()?;
    assert_eq!(
        harness
            .mail
            .owner
            .get_push_record(&push_id)
            .await?
            .ok_or("Push projection missing at receipt")?
            .body
            .as_deref(),
        Some("one pushed item")
    );
    assert_eq!(
        harness
            .mail
            .rows()
            .await?
            .iter()
            .filter(|row| row.category == "inbox")
            .count(),
        1
    );
    // Assert same identity/digest replays current status and changed content cannot replace it.
    assert!(matches!(
        push(&harness, message(item, harness.peer(), "one pushed item")?).await?,
        PushResult::Replayed {
            status: DeliveryStatus::AtRecipient,
            cancel: None
        }
    ));
    assert!(
        matches!(push(&harness, message(item, harness.peer(), "different content")?).await?, PushResult::Conflict { existing } if existing == digest)
    );
    assert_eq!(
        harness
            .mail
            .rows()
            .await?
            .iter()
            .filter(|row| row.category == "inbox")
            .count(),
        1
    );
    assert_eq!(
        harness
            .mail
            .owner
            .get_push_record(&push_id)
            .await?
            .ok_or("original projection missing")?
            .body
            .as_deref(),
        Some("one pushed item")
    );
    harness.close().await
}

#[derive(Serialize, Deserialize)]
struct CrashInput {
    peer: PeerId,
    item: ItemId,
}

#[tokio::test]
#[ignore = "Owned child entry, invoked only by the Push crash parent"]
async fn pushed_mail_crash_child() -> Result<(), TestError> {
    let root =
        std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).ok_or("Push child root missing")?);
    let mut harness = MailHarness::open_configured(&root, None, false, false, None, None).await?;
    welcome(&mut harness).await?;
    let input = CrashInput {
        peer: harness.peer(),
        item: ItemId::new(durable_steps::Uuid::now_v7())?,
    };
    std::fs::write(root.join("input.json"), serde_json::to_vec(&input)?)?;
    harness
        .transport
        .observer
        .record_push_receipts_to(root.join("receipt-sent"));
    let (_, bytes) = push_frame(message(input.item, input.peer, "survives inbox crash")?)?;
    harness.transport.replay(vec![bytes.into()]).await?;
    // The parent kills this process only after the real post-commit barrier signal.
    std::future::pending::<Result<(), TestError>>().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn push_commit_survives_sigkill_before_receipt_and_replays_without_second_intake()
-> Result<(), TestError> {
    // Arrange a child-owned real session and a private debug barrier at the receipt boundary.
    let directory = tempfile::Builder::new()
        .prefix("c5push-")
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let socket = directory.path().join("accepted.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let output = std::fs::File::create(directory.path().join("child.log"))?;
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
        .env(CHILD_ROOT, directory.path())
        .env("CODEX_ROUTER_DEBUG_INBOX_ACCEPTED_BARRIER_SOCKET", &socket)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output)
        .kill_on_drop(true)
        .spawn()?;
    let accepted = tokio::time::timeout(BOUND, async {
        let (mut barrier, _) = listener.accept().await?;
        let mut marker = vec![0; b"inbox-accepted\n".len()];
        barrier.read_exact(&mut marker).await?;
        Ok::<_, std::io::Error>((barrier, marker))
    })
    .await;
    // Act: no receipt can pass the held production barrier before SIGKILL.
    child.kill().await?;
    let status = child.wait().await?;
    let accepted = accepted.map_err(|error| {
        format!(
            "Push did not reach inbox commit: {error}; {}",
            std::fs::read_to_string(directory.path().join("child.log")).unwrap_or_default()
        )
    })??;
    assert_eq!(accepted.1, b"inbox-accepted\n");
    assert_eq!(status.signal(), Some(9));
    assert!(
        !directory.path().join("receipt-sent").exists(),
        "receipt escaped before the crash barrier"
    );
    drop(accepted);
    let input: CrashInput =
        serde_json::from_slice(&std::fs::read(directory.path().join("input.json"))?)?;
    // Assert via owning reopen, then send the same frozen item through a new real pump.
    let owner = interaction_storage::InteractionStore::open(
        directory.path().join("router/interaction.sqlite"),
    )
    .await
    .map_err(|error| format!("Push owning inbox reopen: {error:?}"))?;
    let push_id: collaboration_protocol::PushId = input.item.as_uuid().to_string().try_into()?;
    assert_eq!(
        owner
            .get_push_record(&push_id)
            .await?
            .ok_or("committed Push lost after SIGKILL")?
            .body
            .as_deref(),
        Some("survives inbox crash")
    );
    owner.close().await?;
    let mut recovered =
        MailHarness::open_configured(directory.path(), None, false, false, Some(input.peer), None)
            .await
            .map_err(|error| format!("Push recovered machine assembly: {error}"))?;
    welcome(&mut recovered).await?;
    assert!(matches!(
        push(
            &recovered,
            message(input.item, input.peer, "survives inbox crash")?
        )
        .await?,
        PushResult::Replayed {
            status: DeliveryStatus::AtRecipient,
            cancel: None
        }
    ));
    assert_eq!(
        recovered
            .mail
            .rows()
            .await?
            .iter()
            .filter(|row| row.category == "inbox")
            .count(),
        1
    );
    recovered.close().await
}
