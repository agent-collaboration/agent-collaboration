//! What an update reads before it changes anything: where the root's Fleet database is,
//! the ledger it holds, the older stores beside it, and the errors an update reports.
use codex_router_host::{
    InstanceAcquireError, router_collaboration_directory, router_fleet_cache_database,
    router_fleet_database,
};
use message_board_storage::{
    LegacyFleetSource, RouterLocalStorageError, RouterUpdateStep, StoredUpdateLedger,
    UpdateLedgerEntry, UpdateSourceProgress, read_update_ledger,
};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum RouterUpdateError {
    #[error("the Router Host is running for this root; stop it before running agent-router-update")]
    HostRunning,
    #[error("cannot take this root's Host lock: {0}")]
    HostLock(#[source] InstanceAcquireError),
    #[error(
        "{path} exists, but no update made it; move it aside or remove it, then run agent-router-update again"
    )]
    UnexplainedFleetDatabase { path: PathBuf },
    #[error(
        "both {current} and {former} exist; keep the one that holds this Router's data, move the other aside, then run agent-router-update again"
    )]
    BothFleetDatabaseNames { current: PathBuf, former: PathBuf },
    #[error(
        "{path} still holds data after it was recorded removed; move it aside, then run agent-router-update again"
    )]
    RemovedSourceReappeared { path: PathBuf },
    #[error(
        "{legacy} and {fleet} hold different board cursor signing keys; the Fleet database keeps one, so move aside the store whose key this Router does not use, then run agent-router-update again"
    )]
    CursorKeysDiffer { legacy: PathBuf, fleet: PathBuf },
    #[error(
        "{path} is back after its update step, but {destination}, which holds what that step carried, is missing; restore {destination} or move {path} aside, then run agent-router-update again"
    )]
    StaleSourceWithoutDestination { path: PathBuf, destination: PathBuf },
    #[error(
        "{path} still has a write-ahead log or shared-memory file after the checkpoint; the rename stopped so no committed row is lost"
    )]
    LogRemainsAfterCheckpoint { path: PathBuf },
    #[error("{path}: its approval routes could not be imported into interaction.sqlite: {source}")]
    ApprovalRoutesImport {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error(
        "both {settings} and {older} exist, and {settings} was not written from it by an update; keep the settings you want in {settings}, move {older} aside, then run agent-router-update again"
    )]
    SettingsConflict { settings: PathBuf, older: PathBuf },
    #[error(
        "{older} was carried into {settings}, but {settings} no longer holds its settings (it is missing or was changed since); {older} is kept: put back the {settings} this update wrote, or copy {older}'s settings into its section, then run agent-router-update again"
    )]
    CarriedSettingsMissing { settings: PathBuf, older: PathBuf },
    #[error(
        "{path} is not valid settings JSON ({message}); fix or move it aside, then run agent-router-update again"
    )]
    MalformedSettingsFile { path: PathBuf, message: String },
    #[error(
        "both {former} and {current} exist; keep the machine list you want as {current}, move {former} aside, then run agent-router-update again"
    )]
    BothMachineListNames { former: PathBuf, current: PathBuf },
    #[error(
        "both {current} and {former} exist; keep the one that holds this Router's accounts, move the other aside, then run agent-router-update again"
    )]
    BothProviderProxyNames { current: PathBuf, former: PathBuf },
    #[error("{store}: {source}")]
    ProviderProxyStore {
        store: PathBuf,
        #[source]
        source: codex_router_state::ProviderProxyCheckpointError,
    },
    #[error("{store}: {source}")]
    Store {
        store: PathBuf,
        #[source]
        source: RouterLocalStorageError,
    },
    #[error("{path}: {source}")]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("update barrier failed: {0}")]
    Barrier(#[source] std::io::Error),
}

/// Where the root's Fleet database is right now, and the ledger it holds.
pub(crate) struct RootLayout {
    pub(crate) store: PathBuf,
    pub(crate) store_is_current: bool,
    pub(crate) ledger: StoredUpdateLedger,
}

impl RootLayout {
    pub(crate) async fn read(root: &Path) -> Result<Self, RouterUpdateError> {
        let current = router_fleet_database(root);
        let former = router_fleet_cache_database(root);
        let (store, store_is_current) = match (exists(&current)?, exists(&former)?) {
            (true, true) => {
                return Err(RouterUpdateError::BothFleetDatabaseNames { current, former });
            }
            (true, false) => (current, true),
            (false, _) => (former, false),
        };
        let ledger =
            read_update_ledger(&store)
                .await
                .map_err(|source| RouterUpdateError::Store {
                    store: store.clone(),
                    source,
                })?;
        if store_is_current && ledger == StoredUpdateLedger::WithoutLedger {
            return Err(RouterUpdateError::UnexplainedFleetDatabase { path: store });
        }
        Ok(Self {
            store,
            store_is_current,
            ledger,
        })
    }

    /// The ledger's progress for one step-0005 source, if it has any.
    pub(crate) fn source_progress(
        &self,
        source: LegacyFleetSource,
    ) -> Option<UpdateSourceProgress> {
        let StoredUpdateLedger::Present(entries) = &self.ledger else {
            return None;
        };
        entries.iter().find_map(|entry| match entry {
            UpdateLedgerEntry::Source {
                step,
                source: name,
                progress,
            } if *step == RouterUpdateStep::MergeFleetStores.number()
                && name == source.file_name() =>
            {
                Some(*progress)
            }
            _ => None,
        })
    }
}

/// A legacy source's database file in this root.
pub(crate) fn legacy_source_path(root: &Path, source: LegacyFleetSource) -> PathBuf {
    router_collaboration_directory(root).join(source.file_name())
}

pub(crate) fn exists(path: &Path) -> Result<bool, RouterUpdateError> {
    path.try_exists().map_err(|source| RouterUpdateError::File {
        path: path.to_owned(),
        source,
    })
}
