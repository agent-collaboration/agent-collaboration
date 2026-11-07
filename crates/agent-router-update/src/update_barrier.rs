//! Debug-build causal barriers inside each update step, so a test can hold the real
//! process at a step boundary and kill it there. Release builds compile none of this.
use message_board_storage::LegacyFleetSource;
use std::io;

/// Names the boundary a debug update process has reached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UpdateBarrier {
    /// Step 0001 committed the ledger migration but hasn't recorded itself done.
    LedgerMigrated,
    /// Step 0001 recorded itself done.
    LedgerStepDone,
    /// Step 0005 committed a source's rows and its `written` row together.
    SourceWritten(LegacyFleetSource),
    /// Step 0005 compared every value of a source with the Fleet database.
    SourceVerified(LegacyFleetSource),
    /// Step 0005 removed a source's database file; its side files may remain.
    SourceDatabaseRemoved(LegacyFleetSource),
    /// Step 0005 recorded a source `removed`.
    SourceRemoved(LegacyFleetSource),
    /// Step 0005 recorded itself done.
    MergeDone,
    /// Step 0007 renamed the complete `settings.jsonc` into place, before the ledger row.
    SettingsFileWritten,
    /// Step 0007 recorded `settings.jsonc` written.
    SettingsRecordedWritten,
    /// Step 0007 removed this carried old settings file.
    SettingsOldFileRemoved(&'static str),
    /// Step 0007 renamed `routers.jsonc` to `agent-sessions.jsonc`.
    MachineListRenamed,
    /// Step 0007 recorded itself done.
    SettingsDone,
    /// Step 0010 recorded itself in progress, before the checkpoint.
    RenameBeforeCheckpoint,
    /// Step 0010 checkpointed and closed the store, before the rename.
    RenameCheckpointed,
    /// Step 0010 renamed the file but hasn't recorded itself done.
    Renamed,
    /// Step 0010 recorded itself done.
    RenameDone,
}

impl UpdateBarrier {
    #[cfg_attr(
        not(debug_assertions),
        expect(dead_code, reason = "debug barriers only")
    )]
    fn marker(self) -> String {
        let source_marker = |source: LegacyFleetSource, point: &str| {
            format!("step-0005:{}:{point}", source.file_name())
        };
        match self {
            Self::LedgerMigrated => "step-0001:migrated".to_owned(),
            Self::LedgerStepDone => "step-0001:done".to_owned(),
            Self::SourceWritten(source) => source_marker(source, "written"),
            Self::SourceVerified(source) => source_marker(source, "verified"),
            Self::SourceDatabaseRemoved(source) => source_marker(source, "database-removed"),
            Self::SourceRemoved(source) => source_marker(source, "removed"),
            Self::MergeDone => "step-0005:done".to_owned(),
            Self::SettingsFileWritten => "step-0007:settings-written".to_owned(),
            Self::SettingsRecordedWritten => "step-0007:recorded-written".to_owned(),
            Self::SettingsOldFileRemoved(file) => format!("step-0007:old-file-removed:{file}"),
            Self::MachineListRenamed => "step-0007:machine-list-renamed".to_owned(),
            Self::SettingsDone => "step-0007:done".to_owned(),
            Self::RenameBeforeCheckpoint => "step-0010:before-checkpoint".to_owned(),
            Self::RenameCheckpointed => "step-0010:checkpointed".to_owned(),
            Self::Renamed => "step-0010:renamed".to_owned(),
            Self::RenameDone => "step-0010:done".to_owned(),
        }
    }
}

#[cfg(debug_assertions)]
const BARRIER_SOCKET_VARIABLE: &str = "AGENT_ROUTER_UPDATE_DEBUG_BARRIER_SOCKET";

/// Waits at `barrier` when a debug test named a barrier socket; otherwise returns at once.
pub(crate) async fn wait_at(barrier: UpdateBarrier) -> io::Result<()> {
    #[cfg(debug_assertions)]
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let Some(path) = std::env::var_os(BARRIER_SOCKET_VARIABLE) else {
            return Ok(());
        };
        let path = std::path::PathBuf::from(path);
        validate_barrier_socket(&path)?;
        let mut socket = tokio::net::UnixStream::connect(&path).await?;
        socket
            .write_all(format!("{}\n", barrier.marker()).as_bytes())
            .await?;
        let mut release = [0];
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            socket.read_exact(&mut release),
        )
        .await
        .map_err(io::Error::other)??;
    }
    #[cfg(not(debug_assertions))]
    let _ = barrier;
    Ok(())
}

#[cfg(debug_assertions)]
fn validate_barrier_socket(socket: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    let owner = nix::unistd::geteuid().as_raw();
    let parent = socket
        .parent()
        .filter(|_| socket.is_absolute())
        .ok_or_else(|| io::Error::other("update barrier socket must be an absolute path"))?;
    let directory = std::fs::symlink_metadata(parent)?;
    let endpoint = std::fs::symlink_metadata(socket)?;
    if !directory.is_dir()
        || directory.uid() != owner
        || directory.permissions().mode() & 0o077 != 0
        || !endpoint.file_type().is_socket()
        || endpoint.uid() != owner
    {
        return Err(io::Error::other(
            "update barrier requires an owner-controlled socket in a private directory",
        ));
    }
    Ok(())
}
