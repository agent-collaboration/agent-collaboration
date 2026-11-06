//! Production-shaped roots are always temporary and opt in through the debug-only seam.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::{NativeFixture, native_fixture},
    offline_cli_fixture::{RunFixture, TestResult},
};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

const TEST_HOME: &str = "CODEX_ROUTER_CUTOVER_TEST_HOME";

pub(super) async fn production_fixture() -> TestResult<(CutoverHostFixture, NativeFixture)> {
    let area = tempfile::Builder::new()
        .prefix(super::host_entrypoints::HOST_BASE_PREFIX)
        .tempdir_in(std::fs::canonicalize("/tmp")?)?;
    let mut host = CutoverHostFixture::with_area(area, None)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut source = native_fixture(&host)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    std::fs::set_permissions(&host.home, std::fs::Permissions::from_mode(0o700))?;
    let root = host.home.join(".codex-router");
    let relative_source = source.path.strip_prefix(&host.root)?.to_owned();
    std::fs::rename(&host.root, &root)?;
    host.root = root;
    source.path = host.root.join(relative_source);
    Ok((host, source))
}

pub(super) fn command(
    run: &RunFixture,
    host: &CutoverHostFixture,
    phase: &str,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh"),
    );
    command
        .arg(env!("CARGO_BIN_EXE_codex-router-board-staging-cutover"))
        .arg(phase)
        .arg("--plan")
        .arg(&run.plan)
        .env(TEST_HOME, &host.home)
        .env("HOME", &host.home)
        .kill_on_drop(true);
    command
}

pub(super) async fn preview(
    run: &RunFixture,
    host: &CutoverHostFixture,
    target: &str,
) -> TestResult<serde_json::Value> {
    let output = command(run, host, "offline")
        .args(["--production", "--target", target, "--dry-run"])
        .output()
        .await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(super) async fn execute(
    run: &RunFixture,
    host: &CutoverHostFixture,
    phase: &str,
    target: &str,
) -> TestResult<serde_json::Value> {
    let plan = preview(run, host, target).await?;
    let digest = plan
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .ok_or("preview digest missing")?;
    let mut command = command(run, host, phase);
    command.args([
        "--production",
        "--target",
        target,
        "--confirm-production",
        digest,
    ]);
    if matches!(phase, "offline" | "claim") {
        command.arg("--owner-credential-file").arg(&run.owner);
    }
    let output = command.output().await?;
    if !output.status.success() {
        return Err(format!(
            "{phase} failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[derive(Debug, PartialEq)]
pub(super) struct TreeEntry {
    path: PathBuf,
    device: u64,
    inode: u64,
    mode: u32,
    bytes: Option<Vec<u8>>,
}

pub(super) fn snapshot(root: &Path) -> TestResult<Vec<TreeEntry>> {
    fn collect(root: &Path, path: &Path, entries: &mut Vec<TreeEntry>) -> TestResult {
        let metadata = std::fs::symlink_metadata(path)?;
        entries.push(TreeEntry {
            path: path.strip_prefix(root)?.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            bytes: metadata
                .is_file()
                .then(|| std::fs::read(path))
                .transpose()?,
        });
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                collect(root, &entry?.path(), entries)?;
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    collect(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert exact read-only production preview."
)]
async fn production_dry_run_preserves_every_root_byte_and_identity_with_a_live_host_lock()
-> TestResult {
    // Arrange only a fake home-default root, real native source and existing artifacts.
    let (host, source) = production_fixture().await?;
    let area = tempfile::tempdir()?;
    let run = RunFixture::new(&host, &source, "https://central.example/", area.path())?;
    std::fs::create_dir(&run.bundle)?;
    std::fs::write(
        run.bundle.join("retained-artifact"),
        b"existing bundle bytes",
    )?;
    let backup = host.root.join("staging-cutover-backups");
    std::fs::create_dir(&backup)?;
    std::fs::write(backup.join("retained-backup"), b"existing backup bytes")?;
    let lock = codex_router_host::HostInstanceLock::acquire(
        &codex_router_host::HostCoordinationPaths::for_router_root(&host.root),
    )?;
    let root_before = snapshot(&host.root)?;
    let artifacts_before = snapshot(area.path())?;
    // Act through the compiled operator with the debug-only injected home.
    let plan = preview(&run, &host, "https://central.example/").await?;
    // Assert every root/artifact byte, inode and permission, plus unchanged authority.
    assert_eq!(plan["production"], true);
    assert_eq!(
        plan["projects"][0]["project"],
        source.selected.project.as_str()
    );
    assert_eq!(snapshot(&host.root)?, root_before);
    assert_eq!(snapshot(area.path())?, artifacts_before);
    assert!(matches!(
        codex_router_host::HostInstanceLock::acquire(
            &codex_router_host::HostCoordinationPaths::for_router_root(&host.root)
        ),
        Err(codex_router_host::InstanceAcquireError::AlreadyRunning)
    ));
    drop(lock);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert fail-closed production guards before all effects."
)]
async fn production_guards_refuse_missing_wrong_stale_and_locked_confirmation_without_effects()
-> TestResult {
    // Arrange a real source, private owner handoff, and a listener detecting any HTTP attempt.
    let (host, source) = production_fixture().await?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let target = format!("http://{}/", listener.local_addr()?);
    let area = tempfile::tempdir()?;
    let run = RunFixture::new(&host, &source, &target, area.path())?;
    let plan = preview(&run, &host, &target).await?;
    let digest = plan["digest"].as_str().ok_or("digest missing")?;
    for (flags, expected) in [
        (vec![], "cannot acquire the declared offline Host"),
        (
            vec!["--target", target.as_str(), "--confirm-production", digest],
            "invalid cutover arguments",
        ),
        (
            vec!["--production", "--target", target.as_str()],
            "invalid cutover arguments",
        ),
        (
            vec!["--production", "--confirm-production", digest],
            "invalid cutover arguments",
        ),
        (
            vec![
                "--production",
                "--target",
                target.as_str(),
                "--confirm-production",
                "0000000000000000000000000000000000000000000000000000000000000000",
            ],
            "cannot acquire the declared offline Host",
        ),
        (
            vec![
                "--production",
                "--target",
                "https://different.example/",
                "--confirm-production",
                digest,
            ],
            "explicit production target differs",
        ),
    ] {
        let root_before = snapshot(&host.root)?;
        let artifacts_before = snapshot(area.path())?;
        // Act through each compiled refusal route, with valid authentication available.
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            command(&run, &host, "offline")
                .args(flags)
                .arg("--owner-credential-file")
                .arg(&run.owner)
                .output(),
        )
        .await??;
        // Assert before lock creation, source/artifact writes, credential consumption or TCP.
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(snapshot(&host.root)?, root_before);
        assert_eq!(snapshot(area.path())?, artifacts_before);
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }
    // A content change after preview makes the old confirmation stale.
    let source_directory = source.path.parent().ok_or("source parent absent")?;
    std::fs::write(
        source_directory.join("changed-since-preview"),
        b"new content",
    )?;
    require_stale_refusal(&run, &host, &target, digest).await?;
    // Full-plan identity changes are stale even when all physical source bytes stay equal.
    let current = preview(&run, &host, &target).await?;
    let current_digest = current["digest"].as_str().ok_or("digest missing")?;
    let mut changed: serde_json::Value = serde_json::from_slice(&std::fs::read(&run.plan)?)?;
    changed["projects"][0]["canaryRequest"] =
        serde_json::to_value(message_board::RequestId::generate())?;
    std::fs::write(&run.plan, serde_json::to_vec(&changed)?)?;
    require_stale_refusal(&run, &host, &target, current_digest).await?;
    // A fresh confirmation still grants no authority over a running Host's singleton lock.
    let current = preview(&run, &host, &target).await?;
    let lock = codex_router_host::HostInstanceLock::acquire(
        &codex_router_host::HostCoordinationPaths::for_router_root(&host.root),
    )?;
    let root_before = snapshot(&host.root)?;
    let artifacts_before = snapshot(area.path())?;
    let output = command(&run, &host, "offline")
        .args([
            "--production",
            "--target",
            &target,
            "--confirm-production",
            current["digest"].as_str().ok_or("digest missing")?,
        ])
        .arg("--owner-credential-file")
        .arg(&run.owner)
        .output()
        .await?;
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot acquire the declared offline Host")
    );
    assert_eq!(snapshot(&host.root)?, root_before);
    assert_eq!(snapshot(area.path())?, artifacts_before);
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    drop(lock);
    Ok(())
}

async fn require_stale_refusal(
    run: &RunFixture,
    host: &CutoverHostFixture,
    target: &str,
    digest: &str,
) -> TestResult {
    let root_before = snapshot(&host.root)?;
    let parent = run.plan.parent().ok_or("plan parent missing")?;
    let artifacts_before = snapshot(parent)?;
    let output = command(run, host, "offline")
        .args([
            "--production",
            "--target",
            target,
            "--confirm-production",
            digest,
        ])
        .arg("--owner-credential-file")
        .arg(&run.owner)
        .output()
        .await?;
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot acquire the declared offline Host"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(snapshot(&host.root)?, root_before);
    assert_eq!(snapshot(parent)?, artifacts_before);
    Ok(())
}
