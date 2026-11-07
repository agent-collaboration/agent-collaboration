//! J0 guard: with a one-time update pending, the compiled `codex-router host` and an
//! account command both refuse with "run agent-router-update" before they open or
//! create anything; once the ledger says the update is done, the same command
//! passes the guard and gives its own answer.
#![cfg(debug_assertions)]

use message_board_storage::{FleetStore, UpdateLedger};
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
async fn an_account_command_refuses_a_pending_update_before_opening_its_state() -> GuardResult {
    // Arrange
    let (_directory, root) = old_layout_root()?;
    let before = snapshot(&root)?;

    // Act
    let refused = run_router(&root, &["account", "disable", "--account", "nobody"]).await?;

    // Assert
    let refused_stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{refused_stderr}");
    assert!(
        refused_stderr.contains("run agent-router-update"),
        "{refused_stderr}"
    );
    assert_eq!(
        snapshot(&root)?,
        before,
        "the refused command must not create its state store"
    );
    Ok(())
}

fn file_digest(path: &Path) -> GuardResult<String> {
    use sha2::Digest;
    Ok(format!("{:x}", sha2::Sha256::digest(std::fs::read(path)?)))
}

fn fresh_root(parent: &Path, name: &str) -> GuardResult<PathBuf> {
    let root = parent.join(name);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root.join("agent-communication"))?;
    Ok(root)
}

fn fleet_database(root: &Path) -> PathBuf {
    root.join("agent-communication").join(FleetStore::FILE_NAME)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_store_the_host_created_passes_and_one_it_left_unrecorded_is_refused() -> GuardResult {
    // Arrange: two roots with nothing old. On one the Host's own open created the Fleet
    // database; on the other a Host died after migrating it, before recording any step.
    let directory = tempfile::tempdir()?;
    let created_root = fresh_root(directory.path(), "created")?;
    let unrecorded_root = fresh_root(directory.path(), "unrecorded")?;
    FleetStore::open(&fleet_database(&created_root))
        .await?
        .close()
        .await;
    UpdateLedger::create(&fleet_database(&unrecorded_root))
        .await?
        .close()
        .await;
    let disable_nobody = ["account", "disable", "--account", "nobody"];

    // Act
    let created = run_router(&created_root, &disable_nobody).await?;
    let unrecorded = run_router(&unrecorded_root, &disable_nobody).await?;

    // Assert: past the guard, the command reaches its own state store and gives its
    // own answer for an account this root doesn't have.
    assert_eq!(
        String::from_utf8_lossy(&created.stderr),
        "account status update failed\n",
        "a fresh root's full ledger must pass the guard"
    );
    let unrecorded_stderr = String::from_utf8_lossy(&unrecorded.stderr);
    assert_eq!(unrecorded.status.code(), Some(2), "{unrecorded_stderr}");
    assert!(
        unrecorded_stderr.contains("unfinished (steps 0001, 0002, 0005, 0009, 0010"),
        "{unrecorded_stderr}"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_finished_root_with_an_older_store_left_beside_it_is_refused() -> GuardResult {
    // Arrange: every step done in the current Fleet database, yet an outbox store from
    // the older layout is back beside it, holding data the Host would never read.
    let directory = tempfile::tempdir()?;
    let root = fresh_root(directory.path(), "router")?;
    FleetStore::open(&fleet_database(&root))
        .await?
        .close()
        .await;
    let leftover = root.join("agent-communication/fleet-outbox.sqlite");
    std::fs::write(&leftover, b"older layout")?;

    // Act
    let refused = run_router(&root, &["account", "disable", "--account", "nobody"]).await?;

    // Assert
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("left from an older layout"), "{stderr}");
    assert!(stderr.contains(&leftover.display().to_string()), "{stderr}");
    Ok(())
}
