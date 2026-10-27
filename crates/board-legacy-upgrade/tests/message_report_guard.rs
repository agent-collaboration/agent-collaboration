#![expect(
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Report aliases must refuse before any legacy source or message store is opened.
#![allow(clippy::panic_in_result_fn)]
mod support;
use std::{path::Path, process::Output, time::Duration};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn router_directory() -> TestResult<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    support::create_host_identity(directory.path())?;
    Ok(directory)
}

async fn run(root: &Path, report: &Path) -> TestResult<Output> {
    // An absent board source makes the ordering observable: the report guard
    // must win before source preparation can report its unrelated refusal.
    Ok(tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_codex-router-board-upgrade"))
            .arg("--source")
            .arg(root.join("unread-legacy.sqlite"))
            .arg("--router-directory")
            .arg(root)
            .arg("--report")
            .arg(report)
            .args([
                "--dry-run",
                "--owner-human-id",
                "fixture-owner",
                "--machine-label",
                "report fixture",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await??)
}

#[tokio::test]
async fn report_cannot_overwrite_or_hard_link_the_hosts_service_identity() -> TestResult {
    // Arrange: the identity was created by the same owner as the real Host.
    let directory = router_directory()?;
    let identity = codex_router_host::router_collaboration_directory(directory.path())
        .join("service-identity.json");
    let before = std::fs::read(&identity)?;
    let modified = identity.metadata()?.modified()?;
    let alias = directory.path().join("identity-report.json");
    std::fs::hard_link(&identity, &alias)?;

    // Act / Assert: neither a direct output nor an inode alias can replace it.
    for report in [&identity, &alias] {
        let output = run(directory.path(), report).await?;
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("would overwrite"));
        assert_eq!(std::fs::read(&identity)?, before);
        assert_eq!(identity.metadata()?.modified()?, modified);
    }
    Ok(())
}

#[tokio::test]
async fn report_cannot_overwrite_or_hard_link_either_message_store() -> TestResult {
    // Arrange: sentinels deliberately need no valid SQLite schema. No store
    // admission or migration may occur before the output alias is refused.
    let directory = router_directory()?;
    for name in ["automation.sqlite", "interaction.sqlite"] {
        let store = directory.path().join("agent-communication").join(name);
        let bytes = b"unopened message store sentinel";
        std::fs::write(&store, bytes)?;
        let alias = directory.path().join(format!("{name}-report.json"));
        std::fs::hard_link(&store, &alias)?;
        let modified = store.metadata()?.modified()?;

        // Act / Assert: both the path and a different hard-link path refuse.
        for report in [&store, &alias] {
            let output = run(directory.path(), report).await?;
            assert_eq!(output.status.code(), Some(2));
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("would overwrite"),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(std::fs::read(&store)?, bytes);
            assert_eq!(store.metadata()?.modified()?, modified);
        }
    }
    Ok(())
}

#[tokio::test]
async fn report_cannot_name_a_message_destination_that_does_not_exist_yet() -> TestResult {
    // Arrange: D4 may create the destination later; inode-only checks cannot
    // protect its reserved pathname before that first owning open.
    let directory = router_directory()?;
    let destination = directory
        .path()
        .join("agent-communication/interaction.sqlite");

    // Act / Assert: refuse before creating a report at the future store path.
    let output = run(directory.path(), &destination).await?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("would overwrite"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!destination.exists());
    Ok(())
}

#[tokio::test]
async fn dangling_report_symlink_refuses_before_creating_the_message_destination() -> TestResult {
    // Arrange: a final-component link targets the store that an upgrade would create.
    let directory = router_directory()?;
    let destination = directory
        .path()
        .join("agent-communication/interaction.sqlite");
    let report = directory.path().join("report.json");
    std::os::unix::fs::symlink(&destination, &report)?;
    let identity = std::fs::read(
        codex_router_host::router_collaboration_directory(directory.path())
            .join("service-identity.json"),
    )?;
    // Act: the actual CLI must reject output admission before source preparation.
    let output = run(directory.path(), &report).await?;
    // Assert: no refusal report may create the sole future verified message store.
    assert_eq!(output.status.code(), Some(2));
    assert!(!destination.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("symbolic link"));
    assert_eq!(std::fs::read_link(&report)?, destination);
    assert_eq!(
        std::fs::read(
            codex_router_host::router_collaboration_directory(directory.path())
                .join("service-identity.json")
        )?,
        identity
    );
    assert!(!directory.path().join("unread-legacy.sqlite").exists());
    Ok(())
}
