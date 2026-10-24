#![allow(clippy::panic_in_result_fn)]
mod support;
use std::{
    path::{Path, PathBuf},
    process::{Output, Stdio},
    time::Duration,
};
use support::*;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::test]
async fn version_is_the_released_executable_name_and_workspace_version() -> TestResult {
    // Arrange
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    // Act
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(runner)
            .arg(env!("CARGO_BIN_EXE_codex-router-board-upgrade"))
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    // Assert
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("codex-router-board-upgrade {}\n", env!("CARGO_PKG_VERSION"))
    );
    Ok(())
}

fn identity(directory: &Path) -> TestResult<PathBuf> {
    let root = directory.join("router");
    std::fs::create_dir(&root)?;
    create_host_identity(&root)?;
    Ok(root)
}

async fn run(source: &Path, root: &Path, report: &Path, dry_run: bool) -> TestResult<Output> {
    run_many(&[source], root, report, dry_run).await
}

async fn run_many(
    sources: &[&Path],
    root: &Path,
    report: &Path,
    dry_run: bool,
) -> TestResult<Output> {
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    let mut command = tokio::process::Command::new(runner);
    command.arg(env!("CARGO_BIN_EXE_codex-router-board-upgrade"));
    for source in sources {
        command.arg("--source").arg(source);
    }
    command
        .arg("--router-directory")
        .arg(root)
        .arg("--report")
        .arg(report)
        .args([
            "--owner-human-id",
            "legacy-owner",
            "--machine-label",
            "fixture machine",
        ])
        .kill_on_drop(true);
    if dry_run {
        command.arg("--dry-run");
    } else {
        command.args(["--central-url", "http://127.0.0.1:1"]);
    }
    Ok(tokio::time::timeout(Duration::from_secs(30), command.output()).await??)
}

#[tokio::test]
async fn dry_run_drives_the_binary_and_writes_only_the_report_with_resource_counts() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let root = identity(directory.path())?;
    let config_before = std::fs::read(
        codex_router_host::router_collaboration_directory(&root).join("service-identity.json"),
    )?;
    let source_before = std::fs::read(&source)?;
    let report = directory.path().join("report.json");
    let output = run(&source, &root, &report, true).await?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut progress_phases = Vec::new();
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        let event: serde_json::Value = serde_json::from_str(line)?;
        assert_eq!(event["level"], "INFO");
        let object = event.as_object().ok_or("progress event")?;
        assert!(
            object
                .keys()
                .all(|key| matches!(key.as_str(), "level" | "fields"))
        );
        let fields = event["fields"].as_object().ok_or("progress fields")?;
        assert!(fields.keys().all(|key| matches!(
            key.as_str(),
            "phase" | "rows" | "posts" | "activity_rows" | "projects" | "elapsed_ms"
        )));
        for (key, value) in fields {
            if key != "phase" {
                assert!(value.is_number(), "progress count/timing must be numeric");
            }
        }
        let phase = fields["phase"].as_str().ok_or("progress phase")?;
        assert!(matches!(
            phase,
            "read" | "transformed" | "digest" | "opened" | "staging" | "staged" | "verified"
        ));
        progress_phases.push(phase.to_owned());
    }
    assert!(progress_phases.iter().any(|phase| phase == "read"));
    assert!(progress_phases.iter().any(|phase| phase == "verified"));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["kind"], "dryRun");
    let content = &report["report"];
    assert_eq!(
        content["projects"]
            .as_array()
            .ok_or("projects missing")?
            .len(),
        3
    );
    assert_eq!(
        content["workspaceCounts"]
            .as_array()
            .ok_or("workspace counts missing")?
            .len(),
        2
    );
    assert_eq!(
        content["boardCounts"]
            .as_array()
            .ok_or("board counts missing")?
            .len(),
        2
    );
    let posts = content["projects"]
        .as_array()
        .ok_or("projects missing")?
        .iter()
        .map(|project| project["posts"].as_u64().ok_or("post count missing"))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<u64>();
    assert_eq!(posts, 11);
    for record in content["fleet"]["records"]
        .as_array()
        .ok_or("fleet records missing")?
    {
        assert_eq!(record["time"]["kind"], "legacyUnknown");
    }
    assert_eq!(report["sources"]["owner"], "explicitOverride");
    assert_eq!(report["sources"]["machineLabel"], "explicitOverride");
    assert_eq!(std::fs::read(&source)?, source_before);
    assert_eq!(
        std::fs::read(
            codex_router_host::router_collaboration_directory(&root).join("service-identity.json")
        )?,
        config_before
    );
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 3);
    Ok(())
}

#[tokio::test]
async fn a_collision_refuses_real_publication_and_reports_both_ids() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let (_collision_directory, collision) = fixture_copy("collision.sqlite")?;
    let root = identity(directory.path())?;
    let report = directory.path().join("report.json");
    let output = run_many(&[&source, &collision], &root, &report, false).await?;
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["kind"], "refused");
    assert_eq!(report["reason"], "name collisions refuse publication");
    let collisions = report["report"]["collisions"]
        .as_array()
        .ok_or("collision report missing")?;
    assert!(collisions.iter().any(|collision| {
        collision["kind"] == "workspace"
            && collision["ids"]
                .as_array()
                .is_some_and(|ids| ids.len() == 2)
    }));
    assert!(collisions.iter().any(|collision| {
        collision["kind"] == "board"
            && collision["ids"]
                .as_array()
                .is_some_and(|ids| ids.len() == 2)
    }));
    Ok(())
}

#[tokio::test]
async fn collision_refusal_precedes_credentials_and_any_central_connection() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let (_collision_directory, collision) = fixture_copy("collision.sqlite")?;
    let root = identity(directory.path())?;
    let report = directory.path().join("refused.json");
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(runner)
            .arg(env!("CARGO_BIN_EXE_codex-router-board-upgrade"))
            .arg("--source")
            .arg(source)
            .arg("--source")
            .arg(collision)
            .arg("--router-directory")
            .arg(root)
            .arg("--report")
            .arg(&report)
            .args([
                "--central-url",
                &endpoint,
                "--owner-human-id",
                "legacy-owner",
                "--machine-label",
                "fixture",
            ])
            // This would refuse immediately if credentials were read first.
            .env(
                "CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE",
                "relative.assertion",
            )
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["kind"], "refused");
    assert_eq!(report["reason"], "name collisions refuse publication");
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        _ => return Err("collision-bearing run contacted central".into()),
    }
    Ok(())
}

#[tokio::test]
async fn offline_owner_and_machine_derivation_need_no_running_router() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let root = identity(directory.path())?;
    let report = directory.path().join("report.json");
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    let mut command = tokio::process::Command::new(runner);
    command
        .arg(env!("CARGO_BIN_EXE_codex-router-board-upgrade"))
        .arg("--source")
        .arg(&source)
        .arg("--router-directory")
        .arg(&root)
        .arg("--report")
        .arg(&report)
        .arg("--dry-run")
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output()).await??;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["sources"]["owner"], "osAccount");
    assert_eq!(report["sources"]["machineLabel"], "offlineRouterDerivation");
    assert_eq!(std::fs::read_dir(root)?.count(), 1);
    Ok(())
}

#[tokio::test]
async fn absent_router_identity_is_reported_without_minting_one() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let root = directory.path().join("router");
    std::fs::create_dir(&root)?;
    let report = directory.path().join("report.json");
    let output = run(&source, &root, &report, true).await?;
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["kind"], "refused");
    assert!(
        report["reason"]
            .as_str()
            .ok_or("refusal reason missing")?
            .contains("identity is absent")
    );
    assert_eq!(std::fs::read_dir(root)?.count(), 0);
    Ok(())
}

#[tokio::test]
async fn corrupt_router_identity_is_refused_without_replacing_its_bytes() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let root = identity(directory.path())?;
    let path =
        codex_router_host::router_collaboration_directory(&root).join("service-identity.json");
    let report_path = directory.path().join("report.json");
    for bytes in [
        b"invalid".as_slice(),
        br#"{"version":2,"serviceId":"550e8400-e29b-41d4-a716-446655440000"}"#,
    ] {
        std::fs::write(&path, bytes)?;
        let modified = path.metadata()?.modified()?;
        let output = run(&source, &root, &report_path, true).await?;
        assert_eq!(output.status.code(), Some(2));
        let report: serde_json::Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
        assert_eq!(report["kind"], "refused");
        assert!(
            report["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("identity"))
        );
        assert_eq!(std::fs::read(&path)?, bytes);
        assert_eq!(modified, path.metadata()?.modified()?);
        assert_eq!(std::fs::read_dir(&root)?.count(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn report_output_cannot_overwrite_or_hard_link_a_source_store() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let root = identity(directory.path())?;
    let bytes = std::fs::read(&source)?;
    let alias = directory.path().join("alias.json");
    std::fs::hard_link(&source, &alias)?;
    for output_path in [&source, &alias] {
        let output = run(&source, &root, output_path, true).await?;
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("would overwrite"));
        assert_eq!(bytes, std::fs::read(&source)?);
    }
    Ok(())
}

#[tokio::test]
async fn another_process_holding_the_legacy_file_refuses_the_run() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let root = identity(directory.path())?;
    let report = directory.path().join("report.json");
    let mut holder=tokio::process::Command::new("python3")
        .args(["-c","import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('SELECT COUNT(*) FROM sqlite_schema').fetchone(); print('ready',flush=True); sys.stdin.read(); c.close()"])
        .arg(&source).stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true).spawn()?;
    let stdout = holder.stdout.take().ok_or("holder stdout missing")?;
    let mut reader = BufReader::new(stdout);
    let mut ready = String::new();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut ready)).await??;
    assert_eq!(ready.trim(), "ready");
    let output = run(&source, &root, &report, true).await?;
    holder.kill().await?;
    holder.wait().await?;
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert!(
        report["reason"]
            .as_str()
            .ok_or("reason missing")?
            .contains("open in another process")
    );
    Ok(())
}

#[tokio::test]
async fn invalid_legacy_description_reports_its_topic_through_the_real_cli() -> TestResult {
    let (directory, source) = fixture_copy("role-history.sqlite")?;
    let topic = "019a0000-0000-7000-8000-000000000016";
    let connection = rusqlite::Connection::open(&source)?;
    connection.execute(
        "UPDATE board_topics SET description=? WHERE topic_id=?",
        [&"x".repeat(message_board::MAX_DESCRIPTION_BYTES + 1), topic],
    )?;
    connection.close().map_err(|(_, error)| error)?;
    let bytes = std::fs::read(&source)?;
    let root = identity(directory.path())?;
    let report = directory.path().join("refusal.json");
    let output = run(&source, &root, &report, true).await?;
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["kind"], "refused");
    let reason = report["reason"].as_str().ok_or("refusal reason missing")?;
    assert!(
        reason.contains(topic),
        "topic missing from refusal: {reason}"
    );
    assert!(reason.contains("description"));
    assert_eq!(bytes, std::fs::read(source)?);
    Ok(())
}
