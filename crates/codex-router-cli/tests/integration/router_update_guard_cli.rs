//! J0 guard: with a one-time update pending, the compiled `codex-router host` and an
//! account command both refuse with "run agent-router-update" before they open or
//! create anything; once the ledger says the update is done, the same command
//! passes the guard and gives its own answer.
#![cfg(debug_assertions)]

use message_board_storage::{UpdateLedger, UpdateStepNumber, UpdateStepProgress};
use std::{
    collections::BTreeMap,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};
use tokio::process::Command;

type GuardResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A private copy of the root the old release populated, kept with the update tool.
fn old_layout_root() -> GuardResult<(tempfile::TempDir, PathBuf)> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    copy_tree(
        &repository().join("crates/agent-router-update/tests/fixtures/populated-root"),
        &root,
    )?;
    Ok((directory, root))
}

fn copy_tree(source: &Path, destination: &Path) -> GuardResult {
    std::fs::DirBuilder::new().mode(0o700).create(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn snapshot(root: &Path) -> GuardResult<BTreeMap<PathBuf, String>> {
    fn collect(
        root: &Path,
        directory: &Path,
        files: &mut BTreeMap<PathBuf, String>,
    ) -> GuardResult {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                collect(root, &entry.path(), files)?;
            } else {
                files.insert(
                    entry.path().strip_prefix(root)?.to_owned(),
                    file_digest(&entry.path())?,
                );
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    Ok(files)
}

async fn signed_router_binary() -> GuardResult<PathBuf> {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_codex-router"));
    let signed = Command::new(repository().join("scripts/cargo_debug_signing_runner.sh"))
        .arg("--sign-only")
        .arg(&binary)
        .output()
        .await?;
    let stderr = String::from_utf8_lossy(&signed.stderr);
    if !signed.status.success() || stderr.contains("stays linker-signed") {
        return Err(format!("codex-router could not be signed: {stderr}").into());
    }
    Ok(binary)
}

async fn run_router(root: &Path, arguments: &[&str]) -> GuardResult<Output> {
    let binary = signed_router_binary().await?;
    Ok(tokio::time::timeout(
        Duration::from_secs(60),
        Command::new(binary)
            .args(arguments)
            .arg("--router-root")
            .arg(root)
            .env("OTEL_SDK_DISABLED", "true")
            .env_remove("CODEX_ROUTER_USE_HOME_DEFAULT")
            .kill_on_drop(true)
            .output(),
    )
    .await??)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn host_start_on_an_older_routers_root_refuses_before_touching_it() -> GuardResult {
    // Arrange
    let (_directory, root) = old_layout_root()?;
    let before = snapshot(&root)?;

    // Act
    let output = run_router(&root, &["host"]).await?;

    // Assert
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("run agent-router-update"), "{stderr}");
    assert!(stderr.contains("fleet-cache.sqlite"), "{stderr}");
    assert_eq!(
        snapshot(&root)?,
        before,
        "the refused Host must not create or change a file"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_account_command_refuses_a_pending_update_and_runs_once_it_is_done() -> GuardResult {
    // Arrange
    let (_directory, root) = old_layout_root()?;
    let before = snapshot(&root)?;
    let disable_nobody = ["account", "disable", "--account", "nobody"];

    // Act: pending, then with the ledger recording the only known step done.
    let refused = run_router(&root, &disable_nobody).await?;
    let unchanged = snapshot(&root)?;
    let ledger =
        UpdateLedger::migrate_existing(&root.join("agent-communication/fleet-cache.sqlite"))
            .await?;
    ledger
        .record_step(UpdateStepNumber::new(1), UpdateStepProgress::Done)
        .await?;
    ledger.close().await;
    let allowed = run_router(&root, &disable_nobody).await?;

    // Assert
    let refused_stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{refused_stderr}");
    assert!(
        refused_stderr.contains("run agent-router-update"),
        "{refused_stderr}"
    );
    assert_eq!(
        unchanged, before,
        "the refused command must not create its state store"
    );
    // Past the guard, the command reaches its own state store and gives its own
    // answer for an account this root doesn't have.
    let allowed_stderr = String::from_utf8_lossy(&allowed.stderr);
    assert_eq!(allowed_stderr, "account status update failed\n");
    Ok(())
}

fn file_digest(path: &Path) -> GuardResult<String> {
    use sha2::Digest;
    Ok(format!("{:x}", sha2::Sha256::digest(std::fs::read(path)?)))
}
