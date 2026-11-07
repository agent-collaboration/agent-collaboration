//! The populated old-layout root, byte snapshots of a root, and the real
//! `agent-router-update` binary driven through the debug signing runner, with a
//! causal barrier a test can hold it at and kill it there.
use std::{
    collections::BTreeMap,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};

pub type UpdateResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Workspace ids the root-fixture builder wrote; one page of them is only in the WAL.
pub const WORKSPACE_IN_DATABASE: &str = "019f0000-0000-7000-8000-00000000d301";
pub const WORKSPACE_ONLY_IN_LOG: &str = "019f0000-0000-7000-8000-00000000d302";

const BARRIER_SOCKET_VARIABLE: &str = "AGENT_ROUTER_UPDATE_DEBUG_BARRIER_SOCKET";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A private copy of the root the pinned old release populated.
pub fn populated_root() -> UpdateResult<(tempfile::TempDir, PathBuf)> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/populated-root"),
        &root,
    )?;
    Ok((directory, root))
}

fn copy_tree(source: &Path, destination: &Path) -> UpdateResult {
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

/// Every file under `root` with a digest of its bytes, so "changes nothing" is checked
/// exactly while a failure names only the paths that differ.
pub fn snapshot(root: &Path) -> UpdateResult<BTreeMap<PathBuf, String>> {
    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    Ok(files)
}

fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, String>) -> UpdateResult {
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

/// The update binary, signed with its debug identity before any test launches it.
pub async fn signed_update_binary() -> UpdateResult<PathBuf> {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_agent-router-update"));
    let signed =
        tokio::process::Command::new(repository().join("scripts/cargo_debug_signing_runner.sh"))
            .arg("--sign-only")
            .arg(&binary)
            .output()
            .await?;
    if !signed.status.success() {
        return Err(format!(
            "debug signing failed: {}",
            String::from_utf8_lossy(&signed.stderr)
        )
        .into());
    }
    if String::from_utf8_lossy(&signed.stderr).contains("stays linker-signed") {
        return Err("the update binary could not be signed; refusing to run it unsigned".into());
    }
    Ok(binary)
}

pub async fn run_update(arguments: &[&std::ffi::OsStr]) -> UpdateResult<Output> {
    let binary = signed_update_binary().await?;
    Ok(tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new(binary)
            .args(arguments)
            .kill_on_drop(true)
            .output(),
    )
    .await??)
}

/// Runs the update on `root` and kills it the moment it reports `marker` at a barrier.
pub async fn run_update_killed_at(root: &Path, marker: &str) -> UpdateResult {
    let binary = signed_update_binary().await?;
    let barrier_directory = tempfile::tempdir()?;
    let barrier_root = barrier_directory.path().join("barrier");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&barrier_root)?;
    let socket = barrier_root.join("update.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let mut child = tokio::process::Command::new(binary)
        .arg("--router-root")
        .arg(root)
        .env(BARRIER_SOCKET_VARIABLE, &socket)
        .kill_on_drop(true)
        .spawn()?;
    loop {
        let (stream, _) =
            tokio::time::timeout(Duration::from_secs(60), listener.accept()).await??;
        let mut reported = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut reported).await?;
        if reported.trim_end() == marker {
            // The process is parked at the barrier: kill it there, mid-step.
            child.start_kill()?;
            let status = tokio::time::timeout(Duration::from_secs(30), child.wait()).await??;
            if status.success() {
                return Err("the killed update reported success".into());
            }
            return Ok(());
        }
        // An earlier barrier: let the process continue to the one under test.
        tokio::io::AsyncWriteExt::write_all(reader.get_mut(), &[1]).await?;
    }
}

/// Fleet cache workspace ids readable from `store`, read only.
pub async fn cached_workspace_ids(store: &Path) -> UpdateResult<Vec<String>> {
    use sqlx::ConnectOptions;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store)
        .read_only(true)
        .connect()
        .await?;
    let ids = sqlx::query_scalar::<sqlx_turso::Turso, String>(
        "SELECT id FROM cached_fleet_records WHERE kind = 'workspace' ORDER BY id",
    )
    .fetch_all(&mut connection)
    .await?;
    sqlx::Connection::close(connection).await?;
    Ok(ids)
}

fn file_digest(path: &Path) -> UpdateResult<String> {
    use sha2::Digest;
    Ok(format!("{:x}", sha2::Sha256::digest(std::fs::read(path)?)))
}
