//! Re-enrollment invalidates central-derived state before any new session attaches.
use serde::{Deserialize, Serialize};
use std::{future::Future, io, os::unix::fs::OpenOptionsExt, path::Path};

const JOURNAL_FILE: &str = "enrollment-reset.json";
pub const ENROLLMENT_RESTART_INSTRUCTION: &str =
    "Enrollment changed; restart the Host before it connects to central again.";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnrollmentReset {
    generation: collaboration_protocol::OperationId,
    phase: ResetPhase,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ResetPhase {
    Pending,
    RouterStateCleared,
    Completed,
}

/// A pre-publication crash may require an extra refresh, never a missed reset.
pub(crate) fn record_pending_enrollment_reset(directory: &Path) -> io::Result<std::fs::File> {
    let publication = lock_reset_journal(directory)?;
    save_reset(
        directory,
        &EnrollmentReset {
            generation: collaboration_protocol::OperationId::generate(),
            phase: ResetPhase::Pending,
        },
    )?;
    Ok(publication)
}

fn lock_reset_journal(directory: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(directory.join("enrollment-reset.lock"))?;
    lock.lock()?;
    Ok(lock)
}

/// The CLI's existing status surface reads the same marker as the attach guard.
pub fn fleet_enrollment_restart_required(router_root: &Path) -> io::Result<bool> {
    reset_pending_in_directory(&crate::router_collaboration_directory(router_root))
}

pub(crate) fn reset_pending_in_directory(directory: &Path) -> io::Result<bool> {
    Ok(load_reset(directory)?.is_some_and(|reset| !matches!(reset.phase, ResetPhase::Completed)))
}

fn load_reset(directory: &Path) -> io::Result<Option<EnrollmentReset>> {
    match std::fs::read(directory.join(JOURNAL_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| io::Error::other("invalid enrollment reset journal")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn save_reset(directory: &Path, reset: &EnrollmentReset) -> io::Result<()> {
    use std::{io::Write, os::unix::fs::DirBuilderExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    let temporary = directory.join(format!(
        ".enrollment-reset-{}.tmp",
        collaboration_protocol::OperationId::generate().as_str()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(reset).map_err(io::Error::other)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, directory.join(JOURNAL_FILE))?;
        std::fs::File::open(directory)?.sync_all()
    })();
    if result.is_err() {
        let _cleanup = std::fs::remove_file(&temporary);
    }
    result
}

async fn persist_phase(
    directory: &Path,
    reset: &EnrollmentReset,
    phase: ResetPhase,
) -> io::Result<()> {
    let directory = directory.to_owned();
    let reset = EnrollmentReset {
        generation: reset.generation.clone(),
        phase,
    };
    tokio::task::spawn_blocking(move || save_reset(&directory, &reset))
        .await
        .map_err(io::Error::other)?
}

pub(crate) async fn resume_enrollment_reset(
    journal_directory: &Path,
    storage_directory: &Path,
) -> io::Result<()> {
    resume_with_barrier(journal_directory, storage_directory, reset_barrier()).await
}

async fn resume_with_barrier(
    journal_directory: &Path,
    storage_directory: &Path,
    barrier: impl Future<Output = io::Result<()>>,
) -> io::Result<()> {
    let owned = journal_directory.to_owned();
    let (reset, _reset_owner) = tokio::task::spawn_blocking(move || {
        let owner = lock_reset_journal(&owned)?;
        Ok::<_, io::Error>((load_reset(&owned)?, owner))
    })
    .await
    .map_err(io::Error::other)??;
    let Some(reset) = reset else {
        return Ok(());
    };
    if matches!(reset.phase, ResetPhase::Completed) {
        return Ok(());
    }
    if matches!(reset.phase, ResetPhase::Pending) {
        clear_router_state(storage_directory).await?;
        persist_phase(journal_directory, &reset, ResetPhase::RouterStateCleared).await?;
    }
    // Cancellation here leaves the committed stage for the next Host start.
    barrier.await?;
    forget_project_authority(storage_directory).await?;
    persist_phase(journal_directory, &reset, ResetPhase::Completed).await
}

async fn clear_router_state(directory: &Path) -> io::Result<()> {
    tokio::fs::create_dir_all(directory).await?;
    for file in [
        "machine-session/state.json",
        "machine-session/machine-notice-progress.json",
        "fleet-cache.sqlite",
        "fleet-cache.sqlite-wal",
        "fleet-cache.sqlite-shm",
    ] {
        match tokio::fs::remove_file(directory.join(file)).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    for child in [
        "board-read-cache",
        "claim-installs",
        "snapshot-install-attempts",
    ] {
        match tokio::fs::remove_dir_all(directory.join(child)).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    tokio::fs::File::open(directory).await?.sync_all().await
}

async fn forget_project_authority(directory: &Path) -> io::Result<()> {
    let registry = collaboration_runtime::ProjectStoreRegistry::new(
        directory,
        None,
        std::sync::Arc::new(runtime_primitives::SystemClock),
    );
    let projects = registry.project_ids().await.map_err(io::Error::other)?;
    if !projects.is_empty() {
        // The library owns these records. Keep the session detached until its
        // validated forget_central_authority operation preserves each file.
        return Err(io::Error::other(
            "enrollment reset awaits writer authority reset support; central session remains detached",
        ));
    }
    Ok(())
}

async fn reset_barrier() -> io::Result<()> {
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    if let Some(path) = std::env::var_os("CODEX_ROUTER_DEBUG_ENROLLMENT_RESET_BARRIER_SOCKET") {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        validate_barrier_socket(Path::new(&path))?;
        let mut barrier = tokio::net::UnixStream::connect(path).await?;
        barrier.write_all(b"router-state-cleared\n").await?;
        let mut release = [0];
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            barrier.read_exact(&mut release),
        )
        .await
        .map_err(io::Error::other)??;
    }
    Ok(())
}

#[cfg(all(debug_assertions, feature = "runtime-test-support"))]
fn validate_barrier_socket(socket: &Path) -> io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    if !socket.is_absolute() {
        return Err(io::Error::other(
            "debug enrollment reset barrier socket must be absolute",
        ));
    }
    let parent = socket
        .parent()
        .ok_or_else(|| io::Error::other("debug barrier requires a dedicated directory"))?;
    let directory = std::fs::symlink_metadata(parent)?;
    let endpoint = std::fs::symlink_metadata(socket)?;
    let owner = rustix::process::geteuid().as_raw();
    if !directory.is_dir()
        || directory.uid() != owner
        || directory.permissions().mode() & 0o077 != 0
        || !endpoint.file_type().is_socket()
        || endpoint.uid() != owner
    {
        return Err(io::Error::other(
            "debug enrollment reset barrier requires an owner-controlled socket in a private directory",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "enrollment_reset_test.rs"]
mod tests;
