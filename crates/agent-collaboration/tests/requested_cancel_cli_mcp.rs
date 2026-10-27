//! Requested local controls expose canonical outcomes through the actual CLI and MCP.
use collaboration_service as mail_service;

#[path = "support/local_mail_surfaces.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
mod local_mail_surfaces;

#[path = "../../collaboration-service/tests/support/native_mail_owner.rs"]
#[expect(
    dead_code,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod native_mail_owner;
use collaboration_protocol::MailItemId;
use local_mail_surfaces::{Surfaces, TestResult};
use serde_json::{Value, json};
use std::{path::Path, sync::atomic::Ordering, time::Duration};

async fn cli(root: &Path, args: &[&str]) -> TestResult<(i32, Value, String)> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .kill_on_drop(true)
            .env("CODEX_THREAD_ID", "cancel-cli-sender")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CURSOR_CONVERSATION_ID")
            .args(args)
            .args(["--json", "--service-directory"])
            .arg(root)
            .output(),
    )
    .await??;
    let stderr = String::from_utf8(output.stderr)?;
    let value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cancel CLI JSON {error}; stderr={stderr}"))?;
    Ok((output.status.code().unwrap_or(-1), value, stderr))
}
async fn terminal_control(
    surfaces: &Surfaces,
    target: &MailItemId,
    cancel: &MailItemId,
    expected: &str,
) -> TestResult<Value> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let reply = surfaces
                .api
                .call(
                    "message_cancel",
                    json!({
                        "cancelId":cancel,"target":target,"sender":{"kind":"humanUser"},
                    }),
                )
                .await?;
            let kind = reply
                .pointer("/result/outcome/kind")
                .and_then(Value::as_str);
            if kind == Some(expected) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(reply);
            }
            if kind != Some("pending") {
                return Err(format!("unexpected canonical control: {reply}").into());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn cancel_after_local_delivery_marks_the_original_without_another_native_wake() -> TestResult
{
    let surfaces = Surfaces::start().await?;
    let item = MailItemId::generate();
    let (code, sent, _) = surfaces
        .cli(
            "already delivered",
            &["--human-user", "--item-id", item.as_str()],
        )
        .await?;
    assert_eq!(code, 0, "{sent}");
    assert_eq!(
        sent.pointer("/result/record/deliveryState"),
        Some(&json!("delivered"))
    );
    let before = surfaces
        .interactions
        .get_push_record(&item.clone().try_into()?)
        .await?
        .ok_or("delivered original")?;
    assert!(before.cancelled_after_delivery.is_none());
    let cancel = MailItemId::generate();
    let result = terminal_control(&surfaces, &item, &cancel, "tooLate").await?;
    assert_eq!(
        result.pointer("/result/outcome/at"),
        Some(&json!("delivered"))
    );
    let (code, replay, printed) = cli(
        surfaces.root.path(),
        &[
            "message",
            "cancel",
            "--item",
            item.as_str(),
            "--cancel-id",
            cancel.as_str(),
            "--human-user",
        ],
    )
    .await?;
    assert_eq!(code, 0, "{replay}; {printed}");
    assert!(printed.contains(cancel.as_str()));
    assert_eq!(replay.pointer("/result/record"), result.get("result"));
    let after = surfaces
        .interactions
        .get_push_record(&item.clone().try_into()?)
        .await?
        .ok_or("retained original")?;
    assert_eq!(after.delivery_state, before.delivery_state);
    assert_eq!(
        serde_json::to_value(after.last_outcome)?,
        serde_json::to_value(before.last_outcome)?
    );
    let mark = after
        .cancelled_after_delivery
        .ok_or("cancelled-after-delivery mark")?;
    assert_eq!(mark.cancel.as_str(), cancel.as_str());
    assert_eq!(serde_json::to_value(mark.too_late_at)?, json!("delivered"));
    assert_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 1);

    let second = MailItemId::generate();
    let (code, sent, _) = surfaces
        .cli(
            "second delivered original",
            &["--human-user", "--item-id", second.as_str()],
        )
        .await?;
    assert_eq!(code, 0, "{sent}");
    let (code, conflict, _) = cli(
        surfaces.root.path(),
        &[
            "message",
            "cancel",
            "--item",
            second.as_str(),
            "--cancel-id",
            cancel.as_str(),
            "--human-user",
        ],
    )
    .await?;
    assert_eq!(code, 4, "{conflict}");
    assert_eq!(
        conflict.pointer("/error/serviceKind"),
        Some(&json!("itemConflict"))
    );
    assert_eq!(conflict.pointer("/error/effect"), Some(&json!("none")));
    assert!(
        surfaces
            .interactions
            .get_push_record(&second.try_into()?)
            .await?
            .ok_or("second retained original")?
            .cancelled_after_delivery
            .is_none()
    );
    let saved = surfaces
        .interactions
        .local_cancel_delivery(&cancel.as_str().to_owned().try_into()?)
        .await?
        .ok_or("canonical sender cause")?;
    assert_eq!(saved.target_item.as_str(), item.as_str());
    assert_eq!(
        serde_json::to_value(saved.attribution)?,
        json!({"kind":"bySender"})
    );
    assert_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 2);
    surfaces.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn held_local_mail_is_prevented_through_public_control_before_native_delivery() -> TestResult
{
    let surfaces = Surfaces::start().await?;
    let item = MailItemId::generate();
    let input = Surfaces::held_input(Some(item.as_str()), "held before delivery");
    let held = surfaces.api.call("message_send", input).await?;
    assert_eq!(
        held.pointer("/result/deliveryState"),
        Some(&json!("held")),
        "{held}"
    );
    let cancel = MailItemId::generate();
    let result = terminal_control(&surfaces, &item, &cancel, "prevented").await?;
    let (code, replay, _) = cli(
        surfaces.root.path(),
        &[
            "message",
            "cancel",
            "--item",
            item.as_str(),
            "--cancel-id",
            cancel.as_str(),
            "--human-user",
        ],
    )
    .await?;
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay.pointer("/result/record"), result.get("result"));
    assert!(
        surfaces
            .interactions
            .get_push_record(&item.try_into()?)
            .await?
            .ok_or("prevented push")?
            .cancelled_after_delivery
            .is_none()
    );
    assert_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 0);
    surfaces.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn omitted_cancel_identity_is_printed_even_when_connection_fails_without_storage()
-> TestResult {
    let root = tempfile::tempdir()?;
    let missing = root.path().join("missing-service");
    let target = MailItemId::generate();
    let (code, failure, printed) = cli(
        &missing,
        &[
            "message",
            "cancel",
            "--item",
            target.as_str(),
            "--human-user",
        ],
    )
    .await?;
    assert_eq!(code, 3, "{failure}; {printed}");
    assert_eq!(failure.pointer("/error/effect"), Some(&json!("none")));
    let printed = printed
        .lines()
        .find_map(|line| line.strip_prefix("cancelId: "))
        .ok_or("pre-dispatch cancel identity")?;
    let _identity = MailItemId::try_from(printed.to_owned())?;
    assert!(!missing.exists());
    Ok(())
}
