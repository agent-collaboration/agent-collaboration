//! Real CLI/MCP Local mail observations, sharing only the isolated surface fixture.
#[path = "support/local_mail_surfaces.rs"]
mod local_mail_surfaces;
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
#[path = "../../automation-storage/tests/support/local_mail_capacity.rs"]
mod local_mail_capacity;
#[tokio::test]
async fn full_inbox_refuses_cli_and_mcp_sends_and_replies_without_effect_or_target() -> TestResult {
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    let surfaces = Surfaces::start().await?;
    let caller = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"fixture-sender"});
    let original = surfaces.api.call("message_send", json!({"itemId":MailItemId::generate(),"target":caller,"message":{"kind":"agent","sender":Surfaces::session("fixture-target"),"text":"Reply to this accepted message"},"delivery":"auto"})).await?;
    let reference = original
        .pointer("/result/pushId")
        .and_then(Value::as_str)
        .ok_or("original push")?;
    let mut observer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(surfaces.root.path().join("automation.sqlite")),
    )
    .await?;
    let template = automation_storage::LocalMessageRequest {
        draft: collaboration_protocol::PushRecordDraft {
            push_id: MailItemId::generate().try_into()?,
            kind: collaboration_protocol::PushKind::DirectMessage,
            origin: collaboration_protocol::PushOrigin::OwnerUnverified,
            origin_router_ref: None,
            target: serde_json::from_value(Surfaces::session("capacity-unloaded"))?,
            mode: Some(collaboration_protocol::MessageDelivery::Auto),
            guard: None,
            reply_to_push_id: None,
            header_facts: collaboration_protocol::PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some("x".into()),
            activity: None,
            created_at: chrono::Utc::now(),
        },
        target_identity: "unloaded capacity fixture".into(),
    };
    let full = local_mail_capacity::fill_local_inbox(
        &mut *surfaces.store.lock().await,
        &mut observer,
        template,
    )
    .await?;
    let before: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM mail_items),(SELECT COUNT(*) FROM router_pushes)",
    )
    .fetch_one(&mut observer)
    .await?;
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
    let after: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM mail_items),(SELECT COUNT(*) FROM router_pushes)",
    )
    .fetch_one(&mut observer)
    .await?;
    ensure_eq!(before, after);
    ensure_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 1);
    ensure_eq!(
        surfaces
            .store
            .lock()
            .await
            .get_push_record(&full.held.draft.push_id)
            .await?
            .ok_or("held carrier")?
            .body,
        full.held.draft.body
    );
    ensure!(
        surfaces
            .store
            .lock()
            .await
            .read_local_mail(&full.last.draft.push_id.try_into()?)
            .await?
            .is_some()
    );
    observer.close().await?;
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
async fn committed_delivery_followed_by_custody_read_failure_keeps_unknown_effect_and_identity()
-> TestResult {
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    let surfaces = Surfaces::start().await?;
    let mut observer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(surfaces.root.path().join("automation.sqlite")),
    )
    .await?;
    // The trigger runs in settlement's transaction, after the correct answer is
    // written. It damages only the final read's input; carrier acceptance stays real.
    sqlx::raw_sql("CREATE TABLE final_read_fixture(item_id TEXT PRIMARY KEY, result_json TEXT NOT NULL); CREATE TRIGGER fail_final_custody_read AFTER UPDATE ON mail_items WHEN json_extract(NEW.status_json,'$.kind')='delivered' AND json_extract(OLD.status_json,'$.kind')!='delivered' BEGIN INSERT INTO final_read_fixture VALUES(NEW.item_id,NEW.result_json); UPDATE mail_items SET result_json='{' WHERE item_id=NEW.item_id; END;").execute(&mut observer).await?;
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
            .store
            .lock()
            .await
            .get_push_record(&id.clone().try_into()?)
            .await?
            .ok_or("committed push")?;
        ensure_eq!(
            record.delivery_state,
            collaboration_protocol::PushDeliveryState::Delivered
        );
        let status: String =
            sqlx::query_scalar("SELECT status_json FROM mail_items WHERE item_id=?")
                .bind(id.as_str())
                .fetch_one(&mut observer)
                .await?;
        ensure_eq!(
            serde_json::from_str::<Value>(&status)?["kind"],
            json!("delivered")
        );
        let original: String =
            sqlx::query_scalar("SELECT result_json FROM final_read_fixture WHERE item_id=?")
                .bind(id.as_str())
                .fetch_one(&mut observer)
                .await?;
        sqlx::query("UPDATE mail_items SET result_json=? WHERE item_id=?")
            .bind(&original)
            .bind(id.as_str())
            .execute(&mut observer)
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
    observer.close().await?;
    surfaces.stop().await
}
