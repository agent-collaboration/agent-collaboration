use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::protocol::OperationId;
use collaboration_runtime::test_support::remove_store_directory_tolerating_sqlite_sidecars;
use std::os::unix::fs::DirBuilderExt;

#[tokio::test]
async fn cli_creates_wakeup_with_harness_sender_and_reads_through_host()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a real owned collaboration API and CLI subprocess; no native process/model.
    let operation_id = OperationId::generate();
    let suffix = operation_id
        .as_str()
        .get(24..)
        .expect("generated operation id has an ASCII UUID suffix");
    let root = std::env::temp_dir().join(format!("wake-cli-{suffix}"));
    with_wakeup_test_io_context(
        &format!("create fixture directory {}", root.display()),
        std::fs::DirBuilder::new().mode(0o700).create(&root),
    )?;
    let runtime = with_wakeup_test_io_context(
        &format!("start collaboration runtime in {}", root.display()),
        Box::pin(CollaborationRuntime::start(CollaborationRuntimeInputs {
            board_session: None,
            directory: root.clone(),
            codex_home: root.clone(),
            backend_socket: root.join("absent.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: None,
            remote_control_server_name: None,
            owner_human_id: None,
        }))
        .await,
    )?;
    // Act: the documented CLI operation must reach Host-created persistent state.
    let target=serde_json::json!({"endpoint":{"serviceId":runtime.service_id(),"endpointId":"codex-local"},"sessionId":"fixture-only-new-thread"}).to_string();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .env("CODEX_THREAD_ID", "wake-cli-sender")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CURSOR_CONVERSATION_ID")
        .args([
            "wake",
            "send",
            "--to",
            &target,
            "--every",
            "10m",
            "--for",
            "2h",
            "--text",
            "Check repository",
            "--json",
            "--service-directory",
        ])
        .arg(&root)
        .output()
        .await;
    let output = with_wakeup_test_io_context("spawn initial wake send CLI", output)?;
    if !output.status.success() {
        return Err(format!(
            "CLI create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let record: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let id = record
        .pointer("/result/record/definition/wakeupId")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing created instruction identity")?;
    let read = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "wake",
            "show",
            "--wakeup-id",
            id,
            "--json",
            "--service-directory",
        ])
        .arg(&root)
        .output()
        .await;
    let read = with_wakeup_test_io_context("spawn initial wake show CLI", read)?;
    // Assert: machine-readable read returns the exact text, with no native backend involved.
    if !read.status.success() {
        return Err("CLI show failed".into());
    }
    let record: serde_json::Value = serde_json::from_slice(&read.stdout)?;
    if record
        .pointer("/result/record/definition/message/content/text")
        .and_then(serde_json::Value::as_str)
        != Some("Check repository")
    {
        return Err("CLI wake text was not persisted".into());
    }
    if record
        .pointer("/result/record/definition/message/content/kind")
        .and_then(serde_json::Value::as_str)
        != Some("agent")
        || record
            .pointer("/result/record/definition/message/content/sender/sessionId")
            .and_then(serde_json::Value::as_str)
            != Some("wake-cli-sender")
    {
        return Err("CLI wake did not persist the harness agent identity".into());
    }
    if record.pointer("/result/record/firstFire") != Some(&serde_json::Value::Null) {
        return Err("CLI creation invented a firing".into());
    }
    for (action, state) in [
        ("pause", "paused"),
        ("resume", "active"),
        ("cancel", "cancelled"),
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([
                "wake",
                action,
                "--wakeup-id",
                id,
                "--json",
                "--service-directory",
            ])
            .arg(&root)
            .output()
            .await;
        let output = with_wakeup_test_io_context(&format!("spawn wake {action} CLI"), output)?;
        if !output.status.success() {
            return Err(format!(
                "wake {action} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        if response
            .pointer("/result/record/wakeup/state")
            .and_then(serde_json::Value::as_str)
            != Some(state)
        {
            return Err(format!("wake {action} returned wrong state").into());
        }
    }
    let fire = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .env("CODEX_THREAD_ID", "wake-cli-sender")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CURSOR_CONVERSATION_ID")
        .args([
            "wake",
            "send",
            "--to",
            &target,
            "--text",
            "Check after timer",
            "--after",
            "1s",
            "--wait-until-first-fire",
            "--json",
            "--service-directory",
        ])
        .arg(&root)
        .output()
        .await;
    let fire = with_wakeup_test_io_context("spawn first-fire wake CLI", fire)?;
    if !fire.status.success() {
        return Err(format!(
            "first-fire CLI failed: {}",
            String::from_utf8_lossy(&fire.stderr)
        )
        .into());
    }
    let fired: serde_json::Value = serde_json::from_slice(&fire.stdout)?;
    if fired
        .pointer("/result/record/firstFire/kind")
        .and_then(serde_json::Value::as_str)
        != Some("wakeFired")
    {
        return Err("one-result wait returned before firing receipt".into());
    }
    pin_first_fire_receipt(&fired, &root).await?;
    let listing = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "wake",
            "list",
            "--limit",
            "1",
            "--json",
            "--service-directory",
        ])
        .arg(&root)
        .output()
        .await;
    let listing = with_wakeup_test_io_context("spawn wake listing CLI", listing)?;
    if !listing.status.success() {
        return Err(format!(
            "wake list failed: {}",
            String::from_utf8_lossy(&listing.stderr)
        )
        .into());
    }
    let listing: serde_json::Value = serde_json::from_slice(&listing.stdout)?;
    if listing
        .pointer("/result/page/records")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        != Some(1)
        || listing
            .pointer("/result/page/nextCursor")
            .is_none_or(serde_json::Value::is_null)
    {
        return Err("CLI bounded listing missing page or cursor".into());
    }
    let fired_id = fired
        .pointer("/result/record/firstFire/wakeupId")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing fired wake identity")?;
    let deliveries = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "delivery",
            "list",
            "--wakeup-id",
            fired_id,
            "--json",
            "--service-directory",
        ])
        .arg(&root)
        .output()
        .await;
    let deliveries = with_wakeup_test_io_context("spawn delivery listing CLI", deliveries)?;
    if !deliveries.status.success() {
        return Err("delivery list CLI failed".into());
    }
    let deliveries: serde_json::Value = serde_json::from_slice(&deliveries.stdout)?;
    let delivery_id = deliveries
        .pointer("/result/page/records/0/deliveryId")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing fired delivery")?;
    for action in ["show", "attempts", "reconcile"] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([
                "delivery",
                action,
                "--delivery-id",
                delivery_id,
                "--json",
                "--service-directory",
            ])
            .arg(&root)
            .output()
            .await;
        let output = with_wakeup_test_io_context(&format!("spawn delivery {action} CLI"), output)?;
        if !output.status.success() {
            return Err(format!("delivery {action} CLI failed").into());
        }
        let output: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        if action == "attempts"
            && output
                .pointer("/result/page/coverage/earlierAttempts")
                .and_then(serde_json::Value::as_str)
                != Some("mayBeUnavailable")
        {
            return Err("delivery attempts CLI omitted retention coverage".into());
        }
    }
    runtime.shutdown().await?;
    with_wakeup_test_io_context(
        "remove fixture store directory",
        remove_store_directory_tolerating_sqlite_sidecars(&root),
    )?;
    Ok(())
}

fn with_wakeup_test_io_context<T>(
    operation: &str,
    result: std::io::Result<T>,
) -> std::io::Result<T> {
    result.map_err(|error| std::io::Error::new(error.kind(), format!("{operation}: {error}")))
}

/// `wake send --wait-until-first-fire --json` keeps today's output: `result.record` is the
/// created wake with `firstFire` set to the FireReceipt the wait observed (the tool's
/// `outcome.fired.fire`), field for field, and it is the receipt the wake stores.
async fn pin_first_fire_receipt(
    fired: &serde_json::Value,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let first_fire = fired
        .pointer("/result/record/firstFire")
        .ok_or("wait output omitted result.record.firstFire")?;
    let mut fields: Vec<&str> = first_fire
        .as_object()
        .ok_or("firstFire is not an object")?
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    if fields != ["dueAt", "firedAt", "kind", "occurrenceId", "wakeupId"] {
        return Err(format!("firstFire is not today's FireReceipt shape: {first_fire}").into());
    }
    let receipt: collaboration_client::protocol::FireReceipt =
        serde_json::from_value(first_fire.clone())?;
    if serde_json::to_value(&receipt)? != *first_fire {
        return Err("firstFire does not round-trip as a FireReceipt".into());
    }
    let wakeup_id = fired
        .pointer("/result/record/definition/wakeupId")
        .and_then(serde_json::Value::as_str)
        .ok_or("wait output omitted the created wake")?;
    if first_fire
        .pointer("/wakeupId")
        .and_then(serde_json::Value::as_str)
        != Some(wakeup_id)
    {
        return Err("firstFire names another wake".into());
    }
    let shown = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "wake",
            "show",
            "--wakeup-id",
            wakeup_id,
            "--json",
            "--service-directory",
        ])
        .arg(root)
        .output()
        .await;
    let shown = with_wakeup_test_io_context("spawn wake show after first fire CLI", shown)?;
    if !shown.status.success() {
        return Err("wake show after the first fire failed".into());
    }
    let shown: serde_json::Value = serde_json::from_slice(&shown.stdout)?;
    if shown.pointer("/result/record/firstFire") != Some(first_fire) {
        return Err(format!(
            "the waited FireReceipt differs from the stored one: {first_fire} vs {shown}"
        )
        .into());
    }
    Ok(())
}
