//! Real CLI/MCP Local mail observations, sharing only the isolated surface fixture.
use collaboration_service as mail_service;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/local_mail_surfaces.rs"]
mod local_mail_surfaces;
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "This fixture source belongs to the split lane, which fixes its allowances at integration."
)]
#[path = "../../collaboration-service/tests/support/native_mail_owner.rs"]
mod native_mail_owner;
#[path = "support/local_mail_native_sql.rs"]
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)] // Companion failure tests use the shared fault helpers.
mod native_sql;
use collaboration_protocol::MailItemId;
use local_mail_surfaces::{Surfaces, TestResult};
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
#[tokio::test]
async fn supplied_identity_replays_across_cli_and_mcp_and_conflicts_without_another_injection()
-> TestResult {
    let surfaces = Surfaces::start().await?;
    let id = MailItemId::generate();
    let (code, first, printed) = surfaces
        .cli("same content", &["--human-user", "--item-id", id.as_str()])
        .await?;
    ensure_eq!(code, 0);
    ensure!(printed.contains(id.as_str()));
    let original = first
        .pointer("/result/record")
        .ok_or_else(|| format!("first CLI result: {first}"))?;
    ensure_eq!(original.get("pushId"), Some(&json!(id.as_str())));
    let replay = surfaces
        .api
        .call(
            "message_send",
            Surfaces::input(Some(id.as_str()), "same content"),
        )
        .await?;
    ensure_eq!(replay.get("result"), Some(original));
    let (code, again, _) = surfaces
        .cli("same content", &["--human-user", "--item-id", id.as_str()])
        .await?;
    ensure_eq!(code, 0);
    ensure_eq!(again.pointer("/result/record"), Some(original));
    let conflict = surfaces
        .api
        .call(
            "message_send",
            Surfaces::input(Some(id.as_str()), "changed content"),
        )
        .await?;
    ensure_eq!(
        conflict.pointer("/error/data/kind"),
        Some(&json!("itemConflict"))
    );
    let (code, conflict_cli, _) = surfaces
        .cli(
            "changed content",
            &["--human-user", "--item-id", id.as_str()],
        )
        .await?;
    ensure_eq!(code, 4);
    ensure_eq!(
        conflict_cli.pointer("/error/serviceKind"),
        Some(&json!("itemConflict"))
    );
    let structured_conflict = surfaces
        .api
        .call_structured(
            "message_send",
            Surfaces::input(Some(id.as_str()), "changed content"),
        )
        .await?;
    ensure_eq!(
        structured_conflict.pointer("/error/effect"),
        Some(&json!("none"))
    );
    ensure_eq!(
        conflict_cli.pointer("/error/effect"),
        structured_conflict.pointer("/error/effect")
    );
    ensure_eq!(conflict_cli.get("target"), Some(&Value::Null));
    ensure_eq!(
        structured_conflict.pointer("/error/target"),
        Some(&Value::Null)
    );
    ensure_eq!(
        conflict_cli.pointer("/error/data/existing"),
        structured_conflict.pointer("/error/data/existing")
    );
    let malformed = surfaces
        .api
        .call(
            "message_send",
            Surfaces::input(Some("not-a-uuid"), "never admitted"),
        )
        .await?;
    ensure_eq!(malformed.pointer("/error/code"), Some(&json!(-32602)));
    ensure_eq!(
        malformed.pointer("/error/data/kind"),
        Some(&json!("invalidShape"))
    );
    let (code, bad, _) = surfaces
        .cli(
            "never admitted",
            &["--human-user", "--item-id", "not-a-uuid"],
        )
        .await?;
    ensure_eq!(code, 2);
    ensure_eq!(bad.pointer("/error/kind"), Some(&json!("invalidShape")));
    let structured_malformed = surfaces
        .api
        .call_structured(
            "message_send",
            Surfaces::input(Some("not-a-uuid"), "never admitted"),
        )
        .await?;
    ensure_eq!(
        structured_malformed.pointer("/error/effect"),
        Some(&json!("none"))
    );
    ensure_eq!(
        bad.pointer("/error/effect"),
        structured_malformed.pointer("/error/effect")
    );
    ensure_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 1);
    let reference = surfaces.delivery_reference(&id).await?;
    ensure!(
        native_sql::local_is_compacted(surfaces.interactions.clone(), surfaces.peer, reference)
            .await?
    );
    let recorded = surfaces
        .interactions
        .message_send_answer(&id)
        .await?
        .ok_or("recorded answer missing")?
        .result
        .ok_or("recorded result")?;
    ensure_eq!(serde_json::to_value(recorded)?["pushId"], json!(id));
    surfaces.stop().await
}

#[tokio::test]
async fn omitted_ids_are_fresh_and_cli_prints_its_identity_before_dispatch() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let input = Surfaces::input(None, "intentional repeat");
    let first = surfaces.api.call("message_send", input.clone()).await?;
    let second = surfaces.api.call("message_send", input).await?;
    let first_id = first
        .pointer("/result/pushId")
        .and_then(Value::as_str)
        .ok_or("first minted ID")?;
    let second_id = second
        .pointer("/result/pushId")
        .and_then(Value::as_str)
        .ok_or("second minted ID")?;
    ensure!(first_id != second_id);
    let replay = surfaces
        .api
        .call(
            "message_send",
            Surfaces::input(Some(first_id), "intentional repeat"),
        )
        .await?;
    ensure_eq!(replay.get("result"), first.get("result"));
    let (code, cli, stderr) = surfaces.cli("CLI mint", &["--human-user"]).await?;
    ensure_eq!(code, 0);
    let cli_id = cli
        .pointer("/result/record/pushId")
        .and_then(Value::as_str)
        .ok_or("CLI minted ID")?;
    ensure_eq!(stderr.trim(), format!("itemId: {cli_id}"));
    ensure!(MailItemId::try_from(cli_id.to_owned()).is_ok());
    ensure_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 3);
    surfaces.stop().await
}

#[tokio::test]
async fn every_m2c_mail_preparation_path_writes_terminal_age_retention() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let sender = Surfaces::session("fixture-agent-sender").to_string();
    let agent_id = MailItemId::generate();
    let (code, agent, _) = surfaces
        .cli(
            "agent message",
            &["--from", &sender, "--item-id", agent_id.as_str()],
        )
        .await?;
    ensure_eq!(code, 0);
    let human_id = MailItemId::generate();
    let human = surfaces
        .api
        .call(
            "message_send",
            Surfaces::input(Some(human_id.as_str()), "human message"),
        )
        .await?;
    ensure!(human.get("result").is_some());
    let reply = surfaces.api.call("message_reply",json!({"caller":Surfaces::session("fixture-target"),"reference":agent.pointer("/result/record/link").ok_or("agent link")?,"text":"reply path"})).await?;
    let reply_id = reply
        .pointer("/result/pushId")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("reply result: {reply}"))?;
    for id in [agent_id.as_str(), human_id.as_str(), reply_id] {
        ensure_eq!(
            native_sql::retention(surfaces.interactions.clone(), id.to_owned()).await?,
            "mailTerminalAge"
        );
        let item = id.to_owned().try_into()?;
        let reference = surfaces.delivery_reference(&item).await?;
        ensure!(
            native_sql::local_is_compacted(surfaces.interactions.clone(), surfaces.peer, reference)
                .await?
        );
    }
    ensure_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 3);
    surfaces.stop().await
}

#[tokio::test]
async fn cli_mints_before_a_failing_connection_without_creating_storage() -> TestResult {
    let root = tempfile::Builder::new()
        .prefix("m2c-no-router-")
        .tempdir()?;
    let target = Surfaces::session("fixture-offline-target").to_string();
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .kill_on_drop(true)
            .args([
                "message",
                "send",
                "--to",
                &target,
                "--human-user",
                "--text",
                "pre-dispatch identity",
                "--json",
                "--service-directory",
            ])
            .arg(root.path())
            .output(),
    )
    .await??;
    ensure!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    let printed = stderr
        .lines()
        .find_map(|line| line.strip_prefix("itemId: "))
        .ok_or("identity was not printed before the connection failed")?;
    ensure!(MailItemId::try_from(printed.to_owned()).is_ok());
    ensure_eq!(std::fs::read_dir(root.path())?.count(), 0);
    Ok(())
}
