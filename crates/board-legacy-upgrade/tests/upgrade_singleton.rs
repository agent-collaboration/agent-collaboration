//! Offline upgrade and the running Host share the shipped singleton artifact.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
mod support;

use codex_router_host::{HostCoordinationPaths, HostInstance, router_collaboration_directory};
use std::{
    fs::OpenOptions,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    process::Output,
    time::Duration,
};
use support::*;

fn coordination(root: &Path) -> HostCoordinationPaths {
    HostCoordinationPaths::for_router_root(root)
}

fn write_identity(root: &Path) -> TestResult {
    create_host_identity(root)?;
    assert!(!root.join("service-identity.json").exists());
    Ok(())
}

async fn upgrade(source: &Path, root: &Path, report: &Path, dry_run: bool) -> TestResult<Output> {
    let mut command =
        tokio::process::Command::new(env!("CARGO_BIN_EXE_codex-router-board-upgrade"));
    command
        .arg("--source")
        .arg(source)
        .arg("--router-directory")
        .arg(root)
        .arg("--report")
        .arg(report)
        .args([
            "--owner-human-id",
            "fixture-owner",
            "--machine-label",
            "singleton fixture",
        ])
        .kill_on_drop(true);
    if dry_run {
        command.arg("--dry-run");
    } else {
        command.args(["--central-url", "http://127.0.0.1:1"]).env(
            "CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE",
            "relative.assertion",
        );
    }
    Ok(tokio::time::timeout(Duration::from_secs(30), command.output()).await??)
}

#[tokio::test]
async fn real_upgrade_refuses_a_running_host_without_touching_its_socket() -> TestResult {
    // Arrange: a real Host owns the exact artifacts used by the upgrade.
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    write_identity(directory.path())?;
    let paths = coordination(directory.path());
    let owner = HostInstance::acquire(paths.clone())?;
    let socket_inode = std::fs::metadata(paths.operator_socket())?.ino();
    let source_bytes = std::fs::read(&source)?;
    let report = directory.path().join("report.json");

    // Act / Assert: refuse before import, publication, or socket mutation.
    let output = upgrade(&source, directory.path(), &report, false).await?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already running"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::metadata(paths.operator_socket())?.ino(),
        socket_inode
    );
    assert_eq!(std::fs::read(source)?, source_bytes);
    assert!(!report.exists());
    drop(owner);
    Ok(())
}

#[tokio::test]
async fn real_upgrade_refuses_the_lock_type_used_by_the_shipped_host() -> TestResult {
    // Arrange: the v0.1.98 open / try_lock steps, without using the extracted guard.
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    write_identity(directory.path())?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(directory.path().join("host.lock"))?;
    lock.try_lock()?;
    let report = directory.path().join("report.json");

    // Act / Assert: the old Host's lock excludes the new upgrade process.
    let output = upgrade(&source, directory.path(), &report, false).await?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already running"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!directory.path().join("host.sock").exists());
    assert!(!report.exists());
    Ok(())
}

#[tokio::test]
async fn dry_run_uses_disposable_authority_without_contending_with_the_source_host() -> TestResult {
    // Arrange: dry run may read the explicit Router identity while the Host runs.
    let (directory, source) = fixture_copy("legacy-complete.sqlite")?;
    write_identity(directory.path())?;
    let paths = coordination(directory.path());
    let owner = HostInstance::acquire(paths.clone())?;
    let socket_inode = std::fs::metadata(paths.operator_socket())?.ino();
    let report = directory.path().join("report.json");
    let identity_file =
        router_collaboration_directory(directory.path()).join("service-identity.json");
    let identity_before = std::fs::read(&identity_file)?;

    // Act / Assert: only the disposable output's authority is acquired.
    let output = upgrade(&source, directory.path(), &report, true).await?;
    let report_bytes = std::fs::read(&report)?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}; report: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&report_bytes),
    );
    assert_eq!(
        std::fs::metadata(paths.operator_socket())?.ino(),
        socket_inode
    );
    assert_eq!(std::fs::read(identity_file)?, identity_before);
    assert!(!directory.path().join("service-identity.json").exists());
    drop(owner);
    Ok(())
}
