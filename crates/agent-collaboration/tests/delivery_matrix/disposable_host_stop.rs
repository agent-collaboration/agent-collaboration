//! End the disposable matrix producer through its existing owned Host controller.
use super::proof_context::{ProofContext, ProofResult};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

pub(super) async fn stop(proof: &mut ProofContext) -> ProofResult<()> {
    let marker: Value =
        serde_json::from_slice(&std::fs::read(proof.root.join("debug-host-context.json"))?)?;
    let run_directory = marker
        .get("runDirectory")
        .and_then(Value::as_str)
        .ok_or("disposable Host run-directory identity missing")?;
    let router_root = marker
        .get("routerRoot")
        .and_then(Value::as_str)
        .ok_or("disposable matrix requires an explicit private router root")?;
    if Path::new(run_directory).canonicalize()? != proof.root
        || Path::new(router_root).canonicalize()? != proof.root
    {
        return Err("disposable matrix Host/root binding mismatch; no stop requested".into());
    }
    let process_id = marker
        .get("hostPid")
        .and_then(Value::as_u64)
        .filter(|process_id| *process_id > 0)
        .ok_or("disposable Host PID missing")?;
    let inspected = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("ps")
            .args(["-ww", "-p", &process_id.to_string(), "-o", "command="])
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    let command = String::from_utf8(inspected.stdout)?;
    if !inspected.status.success()
        || !command.contains("automation-debug-host")
        || !command.contains(&format!("--run-directory {run_directory}"))
        || !command.contains(&format!("--router-root {router_root}"))
    {
        return Err("PID is not this disposable Host; no stop requested".into());
    }
    let binary =
        Path::new(env!("CARGO_BIN_EXE_agent-collaboration")).with_file_name("codex-router");
    if !binary.is_file() {
        return Err("adjacent compiled debug Router CLI missing".into());
    }
    proof.record(
        "disposableMatrixHostStopRequested",
        json!({"hostPid":process_id}),
    )?;
    // The public controller owns its original exit deadline, same-owner peer
    // validation, kernel exit observation and final released-lock assertion.
    let output = tokio::process::Command::new(binary)
        .args(["host", "stop", "--require-debug-isolation", "--router-root"])
        .arg(&proof.root)
        .env_remove("CODEX_ROUTER_USE_HOME_DEFAULT")
        .kill_on_drop(true)
        .output()
        .await?;
    let stopped = output.status.success()
        && String::from_utf8_lossy(&output.stdout).trim() == "Host stopped.";
    proof.record(
        "disposableMatrixHostStopped",
        json!({
            "hostPid":process_id,"exitCode":output.status.code(),"stopped":stopped,
        }),
    )?;
    if !stopped {
        return Err("public controller did not prove owned Host exit and lock release".into());
    }
    Ok(())
}
