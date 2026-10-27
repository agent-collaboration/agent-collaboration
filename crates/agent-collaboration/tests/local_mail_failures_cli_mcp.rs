//! Real CLI/MCP Local mail observations, sharing only the isolated surface fixture.
use collaboration_service as mail_service;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/local_mail_surfaces.rs"]
#[expect(
    dead_code,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod local_mail_surfaces;
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "This fixture source belongs to the split lane, which fixes its allowances at integration."
)]
#[path = "../../collaboration-service/tests/support/native_mail_owner.rs"]
#[expect(
    dead_code,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod native_mail_owner;
use collaboration_protocol::MailItemId;
use local_mail_surfaces::{SERVICE, Surfaces, TestResult};
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};
macro_rules! ensure {
    ($condition:expr $(,)?) => {
        if !$condition {
            return Err(format!("assertion failed: {}", stringify!($condition)).into());
        }
    };
}
macro_rules! ensure_eq {
    ($left:expr,$right:expr $(,)?) => {{
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(format!("left={left:?}, right={right:?}").into());
        }
    }};
}
#[path = "support/local_mail_capacity.rs"]
mod local_mail_capacity;
#[path = "support/local_mail_native_sql.rs"]
// Companion replay tests use the shared compaction helpers.
mod native_sql;
#[tokio::test]
async fn full_inbox_refuses_cli_and_mcp_sends_and_replies_without_effect_or_target() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let caller = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"fixture-sender"});
    let original = surfaces.api.call("message_send", json!({"itemId":MailItemId::generate(),"target":caller,"message":{"kind":"agent","sender":Surfaces::session("fixture-target"),"text":"Reply to this accepted message"},"delivery":"auto"})).await?;
    let reference = original
        .pointer("/result/pushId")
        .and_then(Value::as_str)
        .ok_or("original push")?;
    let filled = tokio::time::timeout(
        Duration::from_secs(600),
        local_mail_capacity::fill_local_inbox(&surfaces),
    )
    .await;
    let full = match filled {
        Ok(Ok(full)) => full,
        Ok(Err(error)) => {
            surfaces.stop().await?;
            return Err(error);
        }
        Err(error) => {
            eprintln!(
                "local quota timeout: native answer/push rows={:?}",
                native_sql::counts(surfaces.interactions.clone()).await?
            );
            surfaces.stop().await?;
            return Err(error.into());
        }
    };
    ensure!(full.admitted > 0);
    let before = native_sql::counts(surfaces.interactions.clone()).await?;
    let id = MailItemId::generate();
    let (code, cli, _) = surfaces
        .cli("over budget", &["--human-user", "--item-id", id.as_str()])
        .await?;
    ensure_eq!(code, 4);
    let mcp = surfaces
        .api
        .call_structured(
            "message_send",
            Surfaces::input(Some(id.as_str()), "over budget"),
        )
        .await?;
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .kill_on_drop(true)
            .env("CODEX_THREAD_ID", "fixture-sender")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CURSOR_CONVERSATION_ID")
            .args([
                "message",
                "reply",
                reference,
                "over budget",
                "--json",
                "--service-directory",
            ])
            .arg(surfaces.root.path())
            .output(),
    )
    .await??;
    ensure_eq!(output.status.code(), Some(4));
    let cli_reply: Value = serde_json::from_slice(&output.stdout)?;
    let mcp_reply = surfaces
        .api
        .call_structured(
            "message_reply",
            json!({"caller":caller,"reference":reference,"text":"over budget"}),
        )
        .await?;
    for (cli, mcp, field) in [(cli, mcp, "target"), (cli_reply, mcp_reply, "caller")] {
        ensure_eq!(
            cli.pointer("/error/serviceKind"),
            Some(&json!("mailboxFull"))
        );
        ensure_eq!(
            mcp.pointer("/error/serviceKind"),
            Some(&json!("mailboxFull"))
        );
        ensure_eq!(mcp.pointer("/error/effect"), Some(&json!("none")));
        ensure_eq!(cli.pointer("/error/effect"), mcp.pointer("/error/effect"));
        ensure_eq!(cli.get(field), Some(&Value::Null));
        ensure_eq!(mcp["error"].get(field), Some(&Value::Null));
    }
    let after = native_sql::counts(surfaces.interactions.clone()).await?;
    ensure_eq!(before, after);
    ensure_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 1);
    ensure_eq!(
        surfaces
            .interactions
            .get_push_record(&full.first)
            .await?
            .ok_or("held carrier")?
            .body,
        Some(full.body.clone())
    );
    ensure_eq!(
        surfaces
            .interactions
            .get_push_record(&full.last)
            .await?
            .ok_or("last held projection")?
            .delivery_state,
        collaboration_protocol::PushDeliveryState::Held
    );
    let known = Surfaces::held_input(Some(full.last.as_str()), &full.body);
    let replay = surfaces.api.call("message_send", known).await?;
    ensure_eq!(replay.pointer("/result/pushId"), Some(&json!(full.last)));
    ensure_eq!(
        replay.pointer("/result/deliveryState"),
        Some(&json!("held"))
    );
    ensure_eq!(
        native_sql::counts(surfaces.interactions.clone()).await?,
        before
    );
    surfaces.stop().await
}

#[tokio::test]
async fn malformed_identity_has_no_effect_on_cli_and_mcp() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let (code, cli, _) = surfaces
        .cli(
            "never admitted",
            &["--human-user", "--item-id", "not-a-uuid"],
        )
        .await?;
    let mcp = surfaces
        .api
        .call_structured(
            "message_send",
            Surfaces::input(Some("not-a-uuid"), "never admitted"),
        )
        .await?;
    ensure_eq!(code, 2);
    ensure_eq!(cli.pointer("/error/kind"), Some(&json!("invalidShape")));
    ensure_eq!(
        mcp.pointer("/error/data/kind"),
        Some(&json!("invalidShape"))
    );
    ensure_eq!(mcp.pointer("/error/effect"), Some(&json!("none")));
    ensure_eq!(cli.pointer("/error/effect"), mcp.pointer("/error/effect"));
    ensure_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 0);
    surfaces.stop().await
}

#[tokio::test]
async fn committed_delivery_followed_by_answer_read_failure_keeps_unknown_effect_and_identity()
-> TestResult {
    let surfaces = Surfaces::start().await?;
    // Corrupt only the recorded RPC answer after its real outcome transaction writes it.
    // Both the SDK outcome and the native push stay at their actual owner.
    native_sql::execute(surfaces.interactions.clone(), "CREATE TABLE final_read_fixture(item_id TEXT PRIMARY KEY, result_json TEXT NOT NULL); CREATE TRIGGER fail_final_answer_read AFTER UPDATE OF result ON message_send_answers WHEN NEW.result IS NOT NULL AND OLD.result IS NULL BEGIN INSERT INTO final_read_fixture VALUES(NEW.item_id,NEW.result); UPDATE message_send_answers SET result='{' WHERE item_id=NEW.item_id; END;").await?;
    for cli_first in [false, true] {
        let id = MailItemId::generate();
        let response = if cli_first {
            let (code, value, _) = surfaces
                .cli(
                    "committed before unreadable answer",
                    &["--human-user", "--item-id", id.as_str()],
                )
                .await?;
            ensure_eq!(code, 5);
            value
        } else {
            surfaces
                .api
                .call_structured(
                    "message_send",
                    Surfaces::input(Some(id.as_str()), "committed before unreadable answer"),
                )
                .await?
        };
        let failure = response.get("error").ok_or("error response")?;
        ensure_eq!(failure.get("effect"), Some(&json!("unknown")));
        ensure_eq!(failure.get("serviceKind"), Some(&json!("outcomeUnknown")));
        ensure!(
            failure["message"]
                .as_str()
                .ok_or("message")?
                .contains(&format!("router://{SERVICE}/push/{}", id.as_str()))
        );
        let target = if cli_first {
            response.get("target")
        } else {
            failure.get("target")
        };
        ensure_eq!(target, Some(&Surfaces::session("fixture-target")));
        let record = surfaces
            .interactions
            .get_push_record(&id.clone().try_into()?)
            .await?
            .ok_or("committed push")?;
        ensure_eq!(
            record.delivery_state,
            collaboration_protocol::PushDeliveryState::Delivered
        );
        let original =
            native_sql::saved_answer(surfaces.interactions.clone(), id.as_str().to_owned()).await?;
        native_sql::restore_answer(
            surfaces.interactions.clone(),
            id.as_str().to_owned(),
            original.clone(),
        )
        .await?;
        let replay = surfaces
            .api
            .call(
                "message_send",
                Surfaces::input(Some(id.as_str()), "committed before unreadable answer"),
            )
            .await?;
        ensure_eq!(
            replay.get("result"),
            Some(&serde_json::from_str::<Value>(&original)?)
        );
        let refusal = surfaces
            .api
            .call_structured(
                "message_send",
                Surfaces::input(Some(id.as_str()), "changed after committed delivery"),
            )
            .await?;
        ensure_eq!(
            refusal.pointer("/error/serviceKind"),
            Some(&json!("itemConflict"))
        );
        ensure_eq!(refusal.pointer("/error/effect"), Some(&json!("none")));
        ensure_eq!(refusal.pointer("/error/target"), Some(&Value::Null));
        ensure_eq!(
            surfaces.carrier.injections.load(Ordering::SeqCst),
            if cli_first { 2 } else { 1 }
        );
    }
    surfaces.stop().await
}
