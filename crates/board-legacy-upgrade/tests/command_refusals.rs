#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
#[expect(
    dead_code,
    reason = "The shared CLI owner is also used by the IB and BC05 integration targets."
)]
#[path = "upload_support/actual_cli.rs"]
mod actual_cli;
mod support;
use actual_cli::{ImporterCliInvocation, run_importer_cli};
use codex_router_test_support::owned_cli_process::run_cli_output;
use std::{
    path::{Path, PathBuf},
    process::{Output, Stdio},
    time::Duration,
};
use support::*;

#[tokio::test]
async fn version_is_the_released_executable_name_and_workspace_version() -> TestResult {
    // Arrange
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    // Act
    let mut command = tokio::process::Command::new(runner);
    command
        .arg(env!("CARGO_BIN_EXE_codex-router-board-upgrade"))
        .arg("--version");
    let output = run_cli_output(command, Duration::from_secs(30))
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
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
    let cli_binary = PathBuf::from(env!("CARGO_BIN_EXE_codex-router-board-upgrade"));
    let signing_runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    let output = run_importer_cli(ImporterCliInvocation {
        cli_binary: &cli_binary,
        signing_runner: &signing_runner,
        source_copies: sources,
        router_directory: root,
        report_path: report,
        central_url: (!dry_run).then_some("http://127.0.0.1:1"),
        access_assertion: None,
        owner_human_id: "legacy-owner",
        machine_label: "fixture machine",
    })
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(output)
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
    let source_before = std::fs::read(&source)?;
    let source_modified = source.metadata()?.modified()?;
    let collision_before = std::fs::read(&collision)?;
    let collision_modified = collision.metadata()?.modified()?;
    let root = identity(directory.path())?;
    let report = directory.path().join("report.json");
    let output = run_many(&[&source, &collision], &root, &report, false).await?;
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["kind"], "refused");
    assert_eq!(report["reason"], "name collisions refuse publication");
    assert_collision_ids(&report)?;
    assert_eq!(std::fs::read(&source)?, source_before);
    assert_eq!(source.metadata()?.modified()?, source_modified);
    assert_eq!(std::fs::read(&collision)?, collision_before);
    assert_eq!(collision.metadata()?.modified()?, collision_modified);
    Ok(())
}

#[tokio::test]
async fn collision_refusal_precedes_credentials_and_any_central_connection() -> TestResult {
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let (_collision_directory, collision) = fixture_copy("collision.sqlite")?;
    let source_before = std::fs::read(&source)?;
    let source_modified = source.metadata()?.modified()?;
    let collision_before = std::fs::read(&collision)?;
    let collision_modified = collision.metadata()?.modified()?;
    let root = identity(directory.path())?;
    let report = directory.path().join("refused.json");
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    let mut command = tokio::process::Command::new(runner);
    command
        .arg(env!("CARGO_BIN_EXE_codex-router-board-upgrade"))
        .arg("--source")
        .arg(&source)
        .arg("--source")
        .arg(&collision)
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
        .kill_on_drop(true);
    let output = run_cli_output(command, Duration::from_secs(30))
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["kind"], "refused");
    assert_eq!(report["reason"], "name collisions refuse publication");
    assert_collision_ids(&report)?;
    assert_eq!(std::fs::read(&source)?, source_before);
    assert_eq!(source.metadata()?.modified()?, source_modified);
    assert_eq!(std::fs::read(&collision)?, collision_before);
    assert_eq!(collision.metadata()?.modified()?, collision_modified);
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
    let source_bytes = std::fs::read(&source)?;
    let source_modified = source.metadata()?.modified()?;
    let holder = SourceFileHolder {
        child: tokio::process::Command::new("tail")
            .arg("-f")
            .arg(&source)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?,
        source: source.clone(),
    };

    codex_router_test_support::scenario_finalization::run_scenario(
        holder,
        async |holder| {
            wait_until_lsof_reports(&holder.child, &holder.source).await?;
            let output = run(&holder.source, &root, &report, true)
                .await
                .map_err(promote_test_error)?;
            assert_eq!(output.status.code(), Some(2));
            let report: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
            assert!(
                report["reason"]
                    .as_str()
                    .ok_or("reason missing")?
                    .contains("open in another process")
            );
            assert_eq!(std::fs::read(&holder.source)?, source_bytes);
            assert_eq!(holder.source.metadata()?.modified()?, source_modified);
            Ok(())
        },
        async |mut holder| {
            if holder.child.try_wait()?.is_none() {
                holder.child.kill().await?;
            }
            holder.child.wait().await?;
            let output = tokio::process::Command::new("lsof")
                .args(["-t", "--"])
                .arg(holder.source)
                .output()
                .await?;
            if !output.status.success() && output.status.code() != Some(1) {
                return Err(std::io::Error::other(
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
                .into());
            }
            if !output.stdout.is_empty() {
                return Err(std::io::Error::other("source file still has an open process").into());
            }
            Ok(())
        },
        || Ok(()),
    )
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(())
}

struct SourceFileHolder {
    child: tokio::process::Child,
    source: PathBuf,
}

async fn wait_until_lsof_reports(
    holder: &tokio::process::Child,
    source: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let holder_pid = holder.id().ok_or("holder process id missing")?.to_string();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let output = tokio::process::Command::new("lsof")
                .args(["-t", "--"])
                .arg(source)
                .output()
                .await?;
            if !output.status.success() && output.status.code() != Some(1) {
                return Err(std::io::Error::other(
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
                .into());
            }
            if String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|pid| pid == holder_pid)
            {
                return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

fn assert_collision_ids(report: &serde_json::Value) -> TestResult {
    let collisions = report
        .get("report")
        .and_then(|report| report.get("collisions"))
        .and_then(serde_json::Value::as_array)
        .ok_or("collision report missing")?;
    let workspace = collisions
        .iter()
        .find(|collision| {
            collision.get("kind").and_then(serde_json::Value::as_str) == Some("workspace")
        })
        .ok_or("workspace collision missing")?;
    let board = collisions
        .iter()
        .find(|collision| {
            collision.get("kind").and_then(serde_json::Value::as_str) == Some("board")
        })
        .ok_or("board collision missing")?;
    let workspace_ids = workspace
        .get("ids")
        .ok_or("workspace collision ids missing")?;
    let board_ids = board.get("ids").ok_or("board collision ids missing")?;
    assert_eq!(
        workspace_ids,
        &serde_json::json!([
            "019a0000-0000-7000-8000-000000000002",
            "019a0000-0000-7000-8000-000000000009"
        ])
    );
    assert_eq!(
        board_ids,
        &serde_json::json!([
            "019a0000-0000-7000-8000-00000000000b",
            "019a0000-0000-7000-8000-000000000013"
        ])
    );
    Ok(())
}

fn promote_test_error(error: Box<dyn std::error::Error>) -> std::io::Error {
    std::io::Error::other(error.to_string())
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
