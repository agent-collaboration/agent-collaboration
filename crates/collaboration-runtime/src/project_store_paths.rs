//! Per-project, per-start-epoch file discovery; rejected registrations have a separate suffix.
use crate::ProjectRegistryError;
use message_board::{EpochNumber, ProjectId};
use std::path::{Path, PathBuf};

async fn newest_file(
    directory: &Path,
    suffix: &str,
) -> Result<Option<(EpochNumber, PathBuf)>, ProjectRegistryError> {
    let mut entries = match tokio::fs::read_dir(directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut newest: Option<(EpochNumber, PathBuf)> = None;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(epoch) = name
            .strip_prefix('e')
            .and_then(|name| name.strip_suffix(suffix))
        else {
            continue;
        };
        if epoch.contains('.') {
            continue;
        }
        let value = epoch
            .parse::<u64>()
            .map_err(|_| ProjectRegistryError::InvalidTransition)?;
        let value =
            EpochNumber::try_from(value).map_err(|_| ProjectRegistryError::InvalidTransition)?;
        if epoch != value.get().to_string() {
            return Err(ProjectRegistryError::InvalidTransition);
        }
        if newest
            .as_ref()
            .is_none_or(|(previous, _)| value > *previous)
        {
            newest = Some((value, entry.path()));
        }
    }
    Ok(newest)
}

pub(crate) async fn live_file(directory: &Path) -> Result<Option<PathBuf>, ProjectRegistryError> {
    Ok(live_file_with_epoch(directory).await?.map(|(_, path)| path))
}

pub(crate) async fn live_file_with_epoch(
    directory: &Path,
) -> Result<Option<(EpochNumber, PathBuf)>, ProjectRegistryError> {
    newest_file(directory, ".sqlite").await
}

pub(crate) async fn rejected_file(
    directory: &Path,
) -> Result<Option<PathBuf>, ProjectRegistryError> {
    Ok(newest_file(directory, ".orphaned.sqlite")
        .await?
        .map(|(_, path)| path))
}

pub(crate) async fn project_ids(directory: &Path) -> Result<Vec<ProjectId>, ProjectRegistryError> {
    let mut entries = match tokio::fs::read_dir(directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut projects = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let Ok(project) = ProjectId::try_from(entry.file_name().to_string_lossy().into_owned())
        else {
            continue;
        };
        if rejected_file(&entry.path()).await?.is_none()
            && live_file(&entry.path()).await?.is_some()
        {
            projects.push(project);
        }
    }
    projects.sort();
    Ok(projects)
}
