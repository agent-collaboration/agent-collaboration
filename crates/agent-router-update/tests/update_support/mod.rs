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

/// The fixture builder's other literals: two outbox requests (one pending, one decided
/// at a fixed time), a pending project creation with its input, and the cursor key.
pub const PENDING_REQUEST: &str = "019f0000-0000-7000-8000-00000000d311";
pub const DECIDED_REQUEST: &str = "019f0000-0000-7000-8000-00000000d313";
pub const CREATION_REQUEST: &str = "019f0000-0000-7000-8000-00000000d321";
pub const DECIDED_AT_MS: i64 = 1_793_000_000_000;
pub const CURSOR_KEY_BYTES: [u8; 32] = [0xd3; 32];

/// Every Fleet value a root holds, read raw, wherever the current layout keeps it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetValues {
    /// request_id, canonical_hash, request, state, settled_at_ms
    pub requests: Vec<(String, String, String, String, Option<i64>)>,
    /// request_id, canonical_hash, input
    pub creation_inputs: Vec<(String, String, String)>,
    /// key_id, key_bytes
    pub cursor_keys: Vec<(String, Vec<u8>)>,
    /// kind, id, payload
    pub cached_records: Vec<(String, String, String)>,
    pub checkpoint: i64,
}

impl FleetValues {
    /// Reads the older layout: three stores and their own table names.
    pub async fn read_legacy(root: &Path) -> UpdateResult<Self> {
        let directory = root.join("agent-communication");
        let outbox = directory.join("fleet-outbox.sqlite");
        let cursors = directory.join("board-cursors.sqlite");
        let cache = directory.join("fleet-cache.sqlite");
        Ok(Self {
            requests: requests(&outbox).await?,
            creation_inputs: creation_inputs(&outbox, TableNames::Legacy).await?,
            cursor_keys: cursor_keys(&cursors, TableNames::Legacy).await?,
            cached_records: cached_records(&cache).await?,
            checkpoint: checkpoint(&cache).await?,
        })
    }

    /// Reads one Fleet database after the merge, with its new table names.
    pub async fn read_merged(store: &Path) -> UpdateResult<Self> {
        Ok(Self {
            requests: requests(store).await?,
            creation_inputs: creation_inputs(store, TableNames::Merged).await?,
            cursor_keys: cursor_keys(store, TableNames::Merged).await?,
            cached_records: cached_records(store).await?,
            checkpoint: checkpoint(store).await?,
        })
    }
}

/// The older stores' table names, or the Fleet database's after the merge.
#[derive(Clone, Copy)]
enum TableNames {
    Legacy,
    Merged,
}

async fn read_only(path: &Path) -> UpdateResult<sqlx_turso::TursoConnection> {
    use sqlx::ConnectOptions;
    Ok(sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?)
}

async fn requests(path: &Path) -> UpdateResult<Vec<(String, String, String, String, Option<i64>)>> {
    let mut connection = read_only(path).await?;
    let rows = sqlx::query_as::<sqlx_turso::Turso, (String, String, String, String, Option<i64>)>(
        "SELECT request_id, canonical_hash, request, state, settled_at_ms FROM fleet_outbox_requests ORDER BY request_id",
    )
    .fetch_all(&mut connection)
    .await?;
    sqlx::Connection::close(connection).await?;
    Ok(rows)
}

async fn creation_inputs(
    path: &Path,
    names: TableNames,
) -> UpdateResult<Vec<(String, String, String)>> {
    let mut connection = read_only(path).await?;
    let rows = sqlx::query_as::<sqlx_turso::Turso, (String, String, String)>(match names {
        TableNames::Legacy => {
            "SELECT request_id, canonical_hash, input FROM project_creation_inputs ORDER BY request_id"
        }
        TableNames::Merged => {
            "SELECT request_id, canonical_hash, input FROM fleet_project_creation_inputs ORDER BY request_id"
        }
    })
    .fetch_all(&mut connection)
    .await?;
    sqlx::Connection::close(connection).await?;
    Ok(rows)
}

async fn cursor_keys(path: &Path, names: TableNames) -> UpdateResult<Vec<(String, Vec<u8>)>> {
    let mut connection = read_only(path).await?;
    let rows = sqlx::query_as::<sqlx_turso::Turso, (String, Vec<u8>)>(match names {
        TableNames::Legacy => "SELECT key_id, key_bytes FROM cursor_signing_key ORDER BY key_id",
        TableNames::Merged => {
            "SELECT key_id, key_bytes FROM fleet_cursor_signing_key ORDER BY key_id"
        }
    })
    .fetch_all(&mut connection)
    .await?;
    sqlx::Connection::close(connection).await?;
    Ok(rows)
}

async fn cached_records(path: &Path) -> UpdateResult<Vec<(String, String, String)>> {
    let mut connection = read_only(path).await?;
    let rows = sqlx::query_as::<sqlx_turso::Turso, (String, String, String)>(
        "SELECT kind, id, payload FROM cached_fleet_records ORDER BY kind, id",
    )
    .fetch_all(&mut connection)
    .await?;
    sqlx::Connection::close(connection).await?;
    Ok(rows)
}

async fn checkpoint(path: &Path) -> UpdateResult<i64> {
    let mut connection = read_only(path).await?;
    let position = sqlx::query_scalar::<sqlx_turso::Turso, i64>(
        "SELECT position FROM fleet_cache_checkpoint WHERE singleton = 1",
    )
    .fetch_one(&mut connection)
    .await?;
    sqlx::Connection::close(connection).await?;
    Ok(position)
}

/// Runs the update on `root`, pausing at `marker` to run `inspect` before releasing it.
pub async fn run_update_inspecting_at<Inspect, Future>(
    root: &Path,
    marker: &str,
    inspect: Inspect,
) -> UpdateResult<std::process::ExitStatus>
where
    Inspect: FnOnce() -> Future,
    Future: std::future::Future<Output = UpdateResult>,
{
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
    let mut inspect = Some(inspect);
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted?,
            status = child.wait() => {
                return if inspect.is_some() {
                    Err(format!("the update never reached {marker}").into())
                } else {
                    Ok(status?)
                };
            }
        };
        let (stream, _) = accepted;
        let mut reported = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut reported).await?;
        if reported.trim_end() == marker
            && let Some(inspect) = inspect.take()
        {
            inspect().await?;
        }
        tokio::io::AsyncWriteExt::write_all(reader.get_mut(), &[1]).await?;
    }
}

/// Runs one statement against a store in place, through a writable connection, to give
/// a well-formed store a value the update must refuse.
pub async fn write_store_value(path: &Path, statement: &str) -> UpdateResult {
    use sqlx::ConnectOptions;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .connect()
        .await?;
    let changed = sqlx::query::<sqlx_turso::Turso>(sqlx::AssertSqlSafe(statement.to_owned()))
        .execute(&mut connection)
        .await?
        .rows_affected();
    sqlx::Connection::close(connection).await?;
    if changed != 1 {
        return Err(format!("{statement} changed {changed} rows, not one").into());
    }
    Ok(())
}
