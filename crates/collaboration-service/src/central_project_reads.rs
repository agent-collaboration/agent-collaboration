//! Coalesce central pulls and publish only completed immutable project generations.
use fleet_admission::{DirectoryLease, DirectorySide, LeaseMetadata};
use message_board::{CurrentWriter, ProjectId, WriterObservation};
use message_board_storage::ProjectReadCopy;
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};
use tokio::sync::{Mutex, RwLock};

#[cfg(test)]
#[path = "central_project_reads_tests.rs"]
mod tests;

pub struct PulledProjectReadCopy {
    pub copy: ProjectReadCopy,
    pub lease: DirectorySide<LeaseMetadata>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum CentralReadError {
    #[error("central project copy is temporarily unavailable")]
    Unavailable,
    #[error("central project copy is uninitialized")]
    Uninitialized,
    #[error("central project copy is retired")]
    Retired,
    #[error("central project copy schema is unsupported")]
    UnsupportedSchema,
    #[error("central project copy is corrupt")]
    Corrupt,
}
pub type ProjectReadCopyFuture<'a> =
    Pin<Box<dyn Future<Output = Result<PulledProjectReadCopy, CentralReadError>> + Send + 'a>>;
pub trait ProjectReadCopySource: Send + Sync {
    fn pull<'a>(&'a self, project: &'a ProjectId) -> ProjectReadCopyFuture<'a>;
}

pub(crate) struct CentralReadGeneration {
    pub copy: Arc<ProjectReadCopy>,
    pub lease: DirectorySide<LeaseMetadata>,
}
impl CentralReadGeneration {
    pub fn authority(
        &self,
    ) -> Result<(CurrentWriter, Option<WriterObservation>), CentralReadError> {
        match &self.lease {
            DirectorySide::NotYetReported => Ok((CurrentWriter::Unknown {}, None)),
            DirectorySide::Known(metadata) => {
                if let fleet_admission::DirectoryLease::Held { epoch, .. } = &metadata.lease
                    && epoch.value() != metadata.lease_version.epoch().get()
                {
                    return Err(CentralReadError::Corrupt);
                }
                let writer = match &metadata.lease {
                    fleet_admission::DirectoryLease::Held { epoch, .. } => CurrentWriter::Held {
                        epoch: epoch
                            .value()
                            .try_into()
                            .map_err(|_| CentralReadError::Corrupt)?,
                    },
                    fleet_admission::DirectoryLease::Unheld => CurrentWriter::Unheld {},
                    fleet_admission::DirectoryLease::Retired { .. } => {
                        return Err(CentralReadError::Retired);
                    }
                };
                Ok((
                    writer,
                    Some(WriterObservation {
                        lease_version: metadata.lease_version,
                        as_of: metadata.as_of,
                    }),
                ))
            }
        }
    }
}

#[derive(Default)]
struct ProjectReadSlot {
    refresh: Mutex<()>,
    state: RwLock<ProjectReadState>,
}
#[derive(Default)]
struct ProjectReadState {
    attempt: u64,
    completed: Option<Arc<CentralReadGeneration>>,
    failure: Option<CentralReadError>,
}
pub(crate) struct CentralProjectReads {
    source: Arc<dyn ProjectReadCopySource>,
    projects: Mutex<BTreeMap<ProjectId, Arc<ProjectReadSlot>>>,
}
impl CentralProjectReads {
    pub fn new(source: Arc<dyn ProjectReadCopySource>) -> Self {
        Self {
            source,
            projects: Mutex::new(BTreeMap::new()),
        }
    }
    /// Inspect only a verified completed generation; never start or join a pull.
    pub(crate) async fn completed_authority(
        &self,
        project: &ProjectId,
    ) -> Result<Option<(CurrentWriter, Option<WriterObservation>)>, CentralReadError> {
        let slot = self.projects.lock().await.get(project).cloned();
        let Some(slot) = slot else {
            return Ok(None);
        };
        let completed = slot.state.read().await.completed.clone();
        completed
            .map(|generation| generation.authority())
            .transpose()
    }

    pub async fn read(
        &self,
        project: &ProjectId,
    ) -> Result<Arc<CentralReadGeneration>, CentralReadError> {
        let slot = self
            .projects
            .lock()
            .await
            .entry(project.clone())
            .or_default()
            .clone();
        let observed = slot.state.read().await.attempt;
        // This is only the refresh gate; no writer/read gate spans network I/O.
        let _refresh = slot.refresh.lock().await;
        let state = slot.state.read().await;
        if state.attempt != observed {
            return state.completed.clone().ok_or_else(|| {
                state
                    .failure
                    .clone()
                    .unwrap_or(CentralReadError::Unavailable)
            });
        }
        let previous = state.completed.clone();
        drop(state);
        let outcome = match self.source.pull(project).await {
            Ok(pulled) => (|| {
                let lease =
                    select_lease(previous.as_deref().map(|read| &read.lease), pulled.lease)?;
                let generation = Arc::new(CentralReadGeneration {
                    copy: Arc::new(pulled.copy),
                    lease,
                });
                generation.authority()?;
                Ok(generation)
            })(),
            Err(CentralReadError::Unavailable) => previous.ok_or(CentralReadError::Unavailable),
            Err(error) => Err(error),
        };
        let mut state = slot.state.write().await;
        state.attempt = state.attempt.saturating_add(1);
        state.completed = outcome.as_ref().ok().cloned();
        state.failure = outcome.as_ref().err().cloned();
        outcome
    }
}

fn select_lease(
    previous: Option<&DirectorySide<LeaseMetadata>>,
    incoming: DirectorySide<LeaseMetadata>,
) -> Result<DirectorySide<LeaseMetadata>, CentralReadError> {
    let Some(DirectorySide::Known(stored)) = previous else {
        return Ok(incoming);
    };
    match &incoming {
        DirectorySide::NotYetReported => Ok(DirectorySide::Known(stored.clone())),
        DirectorySide::Known(next) if next.lease_version < stored.lease_version => {
            Ok(DirectorySide::Known(stored.clone()))
        }
        DirectorySide::Known(next)
            if next.lease_version == stored.lease_version
                && !same_committed_lease(stored, next) =>
        {
            Err(CentralReadError::Corrupt)
        }
        DirectorySide::Known(_) => Ok(incoming),
    }
}

fn same_committed_lease(stored: &LeaseMetadata, incoming: &LeaseMetadata) -> bool {
    // Held/Expired is computed at read time, outside the committed lease identity.
    stored.as_of == incoming.as_of
        && match (&stored.lease, &incoming.lease) {
            (
                DirectoryLease::Held {
                    epoch: stored_epoch,
                    holder: stored_holder,
                    expires_at: stored_expiry,
                    state: _,
                },
                DirectoryLease::Held {
                    epoch: incoming_epoch,
                    holder: incoming_holder,
                    expires_at: incoming_expiry,
                    state: _,
                },
            ) => {
                stored_epoch == incoming_epoch
                    && stored_holder == incoming_holder
                    && stored_expiry == incoming_expiry
            }
            (DirectoryLease::Unheld, DirectoryLease::Unheld) => true,
            (
                DirectoryLease::Retired { retired_at: stored },
                DirectoryLease::Retired {
                    retired_at: incoming,
                },
            ) => stored == incoming,
            _ => false,
        }
}
