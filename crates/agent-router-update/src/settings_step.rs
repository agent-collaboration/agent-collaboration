//! Step 0007: the Router's hand-edited settings move into one file, and the session
//! picker's machine list gets its current name.
//!
//! `providers.json` and `ports.json` are carried unchanged, byte for byte, as the
//! `providers` and `ports` sections of `settings.jsonc`. The new file is written whole
//! (a temporary file, synced, then renamed into place); the ledger then records it
//! `written`; only then are the old files removed and the ledger records them
//! `removed`. A rerun after the write finishes the removals.
//!
//! A `settings.jsonc` this step would have written from the old files still present is
//! recognised as its own (the content is deterministic), so a kill between the rename
//! and the ledger row is finished, not refused. Any other `settings.jsonc` beside an old
//! file is a conflict: the step stops and names both. A malformed old file stops it with
//! its path. `routers.jsonc` is renamed to `agent-sessions.jsonc` unchanged; when both
//! exist neither is touched, and the picker names both.
use crate::{
    root_layout::{RootLayout, RouterUpdateError, exists},
    update_barrier::{UpdateBarrier, wait_at},
};
use codex_router_host::router_settings_path;
use message_board_storage::{
    RouterUpdateStep, StoredUpdateLedger, UpdateLedger, UpdateSourceProgress, UpdateStepProgress,
};
use std::{
    io::Write as _,
    os::unix::fs::OpenOptionsExt as _,
    path::{Path, PathBuf},
};

/// The two files whose content becomes `settings.jsonc`, in section order.
const SETTINGS_SOURCES: [(&str, &str); 2] =
    [("providers.json", "providers"), ("ports.json", "ports")];
/// The ledger names the written `settings.jsonc` by its file name.
const SETTINGS_LEDGER_SOURCE: &str = "settings.jsonc";
const LEGACY_MACHINE_LIST: &str = "routers.jsonc";
const MACHINE_LIST: &str = "agent-sessions.jsonc";

/// What step 0007 will do on a root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsChange {
    pub settings: SettingsFileChange,
    pub machine_list: MachineListChange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsFileChange {
    /// No old settings file is left.
    Nothing,
    /// Write `settings` from these old files, then remove them.
    Carry { into: PathBuf, from: Vec<PathBuf> },
    /// `settings` holds the old files' content already; remove the old files left.
    RemoveCarried { into: PathBuf, from: Vec<PathBuf> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineListChange {
    Nothing,
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    /// Both names exist: neither is touched, and the picker names both.
    BothNamesKept {
        former: PathBuf,
        current: PathBuf,
    },
}

impl std::fmt::Display for SettingsChange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let joined = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" and ")
        };
        let settings = match &self.settings {
            SettingsFileChange::Nothing => "no older settings file to carry".to_owned(),
            SettingsFileChange::Carry { into, from } => format!(
                "write {} from {}, then remove them",
                into.display(),
                joined(from)
            ),
            SettingsFileChange::RemoveCarried { into, from } if from.is_empty() => format!(
                "record the older settings files removed, already carried into {}",
                into.display()
            ),
            SettingsFileChange::RemoveCarried { into, from } => format!(
                "remove {}, already carried into {}",
                joined(from),
                into.display()
            ),
        };
        let machine_list = match &self.machine_list {
            MachineListChange::Nothing => String::new(),
            MachineListChange::Rename { from, to } => {
                format!("; rename {} to {}", from.display(), to.display())
            }
            MachineListChange::BothNamesKept { former, current } => format!(
                "; both {} and {} exist and are left as they are",
                current.display(),
                former.display()
            ),
        };
        write!(formatter, "{settings}{machine_list}")
    }
}

/// Decides what step 0007 does, reading files but changing nothing.
pub(crate) fn plan_settings_step(
    root: &Path,
    ledger: &StoredUpdateLedger,
) -> Result<SettingsChange, RouterUpdateError> {
    let into = router_settings_path(root);
    let mut from = Vec::new();
    for (file, _) in SETTINGS_SOURCES {
        let path = root.join(file);
        if exists(&path)? {
            from.push(path);
        }
    }
    let written = ledger.source_progress(
        RouterUpdateStep::CarrySettingsFiles.number(),
        SETTINGS_LEDGER_SOURCE,
    ) == Some(UpdateSourceProgress::Written);
    let settings = if written {
        // Carried already: finish any removal left, and record it, even when a kill
        // came after the last old file went but before its `removed` row.
        SettingsFileChange::RemoveCarried { into, from }
    } else if from.is_empty() {
        SettingsFileChange::Nothing
    } else if exists(&into)? {
        let expected = render_settings(root)?;
        let present = std::fs::read(&into).map_err(|source| RouterUpdateError::File {
            path: into.clone(),
            source,
        })?;
        if present == expected.into_bytes() {
            SettingsFileChange::RemoveCarried { into, from }
        } else {
            return Err(RouterUpdateError::SettingsConflict {
                settings: into,
                older: from.swap_remove(0),
            });
        }
    } else {
        render_settings(root)?;
        SettingsFileChange::Carry { into, from }
    };
    let former = root.join(LEGACY_MACHINE_LIST);
    let current = root.join(MACHINE_LIST);
    let machine_list = match (exists(&former)?, exists(&current)?) {
        (true, false) => MachineListChange::Rename {
            from: former,
            to: current,
        },
        (true, true) => MachineListChange::BothNamesKept { former, current },
        (false, _) => MachineListChange::Nothing,
    };
    Ok(SettingsChange {
        settings,
        machine_list,
    })
}

/// Runs step 0007 as planned under the Host lock.
pub(crate) async fn carry_settings_files(root: &Path) -> Result<(), RouterUpdateError> {
    let layout = RootLayout::read(root).await?;
    let change = plan_settings_step(root, &layout.ledger)?;
    let ledger = UpdateLedger::migrate_existing(&layout.store)
        .await
        .map_err(|source| store_error(&layout.store, source))?;
    let carried = async {
        match &change.settings {
            SettingsFileChange::Nothing => {}
            SettingsFileChange::Carry { into, from } => {
                write_settings_file(into, &render_settings(root)?)?;
                barrier(UpdateBarrier::SettingsFileWritten).await?;
                record_written(&ledger, &layout.store).await?;
                remove_carried(&ledger, &layout.store, from).await?;
            }
            SettingsFileChange::RemoveCarried { from, .. } => {
                record_written(&ledger, &layout.store).await?;
                remove_carried(&ledger, &layout.store, from).await?;
            }
        }
        if let MachineListChange::Rename { from, to } = &change.machine_list {
            std::fs::rename(from, to).map_err(|source| RouterUpdateError::File {
                path: from.clone(),
                source,
            })?;
            barrier(UpdateBarrier::MachineListRenamed).await?;
        }
        ledger
            .record_step(
                RouterUpdateStep::CarrySettingsFiles.number(),
                UpdateStepProgress::Done,
            )
            .await
            .map_err(|source| store_error(&layout.store, source))?;
        barrier(UpdateBarrier::SettingsDone).await
    }
    .await;
    ledger.close().await;
    carried
}

/// The `settings.jsonc` the old files present now become: each old file's bytes, checked
/// to be one JSON object, become its section unchanged.
fn render_settings(root: &Path) -> Result<String, RouterUpdateError> {
    let mut sections = Vec::new();
    for (file, section) in SETTINGS_SOURCES {
        let path = root.join(file);
        if !exists(&path)? {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|source| RouterUpdateError::File {
            path: path.clone(),
            source,
        })?;
        let malformed = |message: String| RouterUpdateError::MalformedSettingsFile {
            path: path.clone(),
            message,
        };
        let text = String::from_utf8(bytes).map_err(|error| malformed(error.to_string()))?;
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(_)) => {}
            Ok(_) => return Err(malformed("the file is not a JSON object".to_owned())),
            Err(error) => return Err(malformed(error.to_string())),
        }
        sections.push(format!("  \"{section}\": {}", text.trim()));
    }
    Ok(format!(
        "// Router settings, carried unchanged from providers.json and ports.json by\n\
         // agent-router-update. Edit by hand; the Host reads it at each start.\n\
         {{\n{}\n}}\n",
        sections.join(",\n")
    ))
}

/// Writes `contents` to `path` whole: a private temporary file beside it, synced, then
/// renamed into place and the directory synced, so a crash leaves the old state or the
/// complete new file.
fn write_settings_file(path: &Path, contents: &str) -> Result<(), RouterUpdateError> {
    let file_error = |path: &Path, source| RouterUpdateError::File {
        path: path.to_owned(),
        source,
    };
    let directory = path
        .parent()
        .ok_or_else(|| file_error(path, std::io::Error::other("settings path has no parent")))?;
    let temporary = directory.join(".settings.jsonc.update.tmp");
    match std::fs::remove_file(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(file_error(&temporary, source)),
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|source| file_error(&temporary, source))?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|source| file_error(&temporary, source))?;
    std::fs::rename(&temporary, path).map_err(|source| file_error(path, source))?;
    std::fs::File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| file_error(directory, source))
}

async fn record_written(ledger: &UpdateLedger, store: &Path) -> Result<(), RouterUpdateError> {
    ledger
        .record_step_source(
            RouterUpdateStep::CarrySettingsFiles,
            SETTINGS_LEDGER_SOURCE,
            UpdateSourceProgress::Written,
        )
        .await
        .map_err(|source| store_error(store, source))?;
    barrier(UpdateBarrier::SettingsRecordedWritten).await
}

async fn remove_carried(
    ledger: &UpdateLedger,
    store: &Path,
    from: &[PathBuf],
) -> Result<(), RouterUpdateError> {
    for path in from {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(RouterUpdateError::File {
                    path: path.clone(),
                    source,
                });
            }
        }
        barrier(UpdateBarrier::SettingsOldFileRemoved(carried_file_name(
            path,
        )))
        .await?;
    }
    ledger
        .record_step_source(
            RouterUpdateStep::CarrySettingsFiles,
            SETTINGS_LEDGER_SOURCE,
            UpdateSourceProgress::Removed,
        )
        .await
        .map_err(|source| store_error(store, source))
}

/// The old settings file's name as the catalog spells it, for its barrier marker.
fn carried_file_name(path: &Path) -> &'static str {
    SETTINGS_SOURCES
        .iter()
        .map(|(file, _)| *file)
        .find(|file| path.file_name().and_then(|name| name.to_str()) == Some(*file))
        .unwrap_or("unknown")
}

fn store_error(
    store: &Path,
    source: message_board_storage::RouterLocalStorageError,
) -> RouterUpdateError {
    RouterUpdateError::Store {
        store: store.to_owned(),
        source,
    }
}

async fn barrier(point: UpdateBarrier) -> Result<(), RouterUpdateError> {
    wait_at(point).await.map_err(RouterUpdateError::Barrier)
}
