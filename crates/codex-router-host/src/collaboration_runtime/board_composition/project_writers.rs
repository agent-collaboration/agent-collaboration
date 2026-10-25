//! The Host's registration port owns runtimes; the registry owns only database files.
#[path = "project_writers/writer_registration.rs"]
mod writer_registration;

use super::project_writer::ProjectWriter;
use board_fleet_sync::{BoardCaptureBookkeeping, BoardPieceAnnotation};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_service::{
    ProjectRegistration, ProjectRegistrationError, ProjectRegistrationFuture,
    ProjectRegistrationState, ProjectReplicationPosition, ProjectStoreRegistry,
    ReplicatedPositionFuture,
};
use fleet_sync_protocol::{CopyRole, ReceiverId, RegistrationRefusal};
use fleet_sync_tokio_runner::{SessionRequests, WriterSessionCalls};
use fleet_sync_writer::{ReplicationSender, SenderOptions, WriterRegistrationOutcome};
use message_board::{BoardError, EpochNumber, FeedCursor, FeedPosition, ProjectId};
use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, Weak},
};
use tokio::{
    sync::{Mutex, mpsc, watch},
    task::JoinSet,
};

type WriterSlot = Arc<Mutex<ProjectWriterSlot>>;
pub(super) enum ProjectWriterSlot {
    Installing(Arc<super::snapshot_installation::SnapshotInstallation>),
    Empty,
    Ready(Arc<ProjectWriter>),
    Terminal { writer: Option<Arc<ProjectWriter>> },
}

pub(super) enum ProjectLifecycleSignal {
    Superseded {
        project: ProjectId,
        source: Weak<collaboration_service::NativeProjectStore>,
    },
    RegistrationReady(ProjectId),
    Acknowledged(ProjectId),
    WritingConfirmed(ProjectId),
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NoticeHandling {
    Handled,
    Failed,
}

pub(super) struct ProjectWriters {
    #[cfg(test)]
    pub(super) notice_completions:
        tokio::sync::broadcast::Sender<(fleet_sync_protocol::NoticePosition, NoticeHandling)>,
    pub(super) directory: std::path::PathBuf,
    pub(super) peer: fleet_sync_protocol::PeerId,
    registry: Weak<ProjectStoreRegistry>,
    pub(super) requests: SessionRequests<BoardConsumerProtocol>,
    pub(super) receiver: ReceiverId,
    pub(super) welcomed: watch::Receiver<bool>,
    pub(super) stopped: watch::Receiver<bool>,
    writers: Mutex<BTreeMap<ProjectId, WriterSlot>>,
    pub(super) operations: Mutex<JoinSet<io::Result<()>>>,
    pub(super) lifecycle_signals: mpsc::UnboundedSender<ProjectLifecycleSignal>,
    lifecycle_requests: Mutex<mpsc::UnboundedReceiver<ProjectLifecycleSignal>>,
    registration_starts: tokio::sync::broadcast::Sender<ProjectId>,
    #[cfg(test)]
    renewal_prompts: tokio::sync::broadcast::Sender<(ProjectId, runtime_primitives::OperationId)>,
    #[cfg(test)]
    registration_waits: tokio::sync::broadcast::Sender<ProjectId>,
}

impl ProjectWriters {
    pub fn new(
        directory: &std::path::Path,
        peer: fleet_sync_protocol::PeerId,
        registry: &Arc<ProjectStoreRegistry>,
        requests: SessionRequests<BoardConsumerProtocol>,
        receiver: ReceiverId,
        welcomed: watch::Receiver<bool>,
        stopped: watch::Receiver<bool>,
    ) -> Self {
        let (lifecycle_signals, lifecycle_requests) = mpsc::unbounded_channel();
        Self {
            #[cfg(test)]
            notice_completions: tokio::sync::broadcast::channel(32).0,
            directory: directory.into(),
            peer,
            registry: Arc::downgrade(registry),
            requests,
            receiver,
            welcomed,
            stopped,
            writers: Mutex::new(BTreeMap::new()),
            operations: Mutex::new(JoinSet::new()),
            lifecycle_signals,
            lifecycle_requests: Mutex::new(lifecycle_requests),
            registration_starts: tokio::sync::broadcast::channel(64).0,
            #[cfg(test)]
            renewal_prompts: tokio::sync::broadcast::channel(64).0,
            #[cfg(test)]
            registration_waits: tokio::sync::broadcast::channel(64).0,
        }
    }

    pub(super) fn registry(&self) -> io::Result<Arc<ProjectStoreRegistry>> {
        self.registry
            .upgrade()
            .ok_or_else(|| io::Error::other("project registry has closed"))
    }

    pub async fn ensure(&self, project: &ProjectId) -> io::Result<Arc<ProjectWriter>> {
        if *self.stopped.borrow() {
            return Err(io::Error::other("machine session has stopped"));
        }
        let registry = self.registry()?;
        if registry
            .existing_project(project)
            .await
            .map_err(io::Error::other)?
            .is_none()
        {
            return Err(io::Error::other("project has no live native file"));
        }
        let slot = self.writer_slot(project).await;
        let mut owner = slot.lock().await;
        match &*owner {
            ProjectWriterSlot::Ready(writer) => return Ok(writer.clone()),
            ProjectWriterSlot::Terminal { .. } => {
                return Err(io::Error::other("project writer is terminal"));
            }
            ProjectWriterSlot::Installing(_) => {
                return Err(io::Error::other("project seed is not published"));
            }
            ProjectWriterSlot::Empty => {}
        }
        let writer = Arc::new(
            ProjectWriter::open(
                &registry,
                project,
                self.receiver,
                self.requests.clone(),
                self.stopped.clone(),
                self.lifecycle_signals.clone(),
            )
            .await?,
        );
        if *self.stopped.borrow() {
            *owner = ProjectWriterSlot::Terminal {
                writer: Some(writer.clone()),
            };
            writer.stop().await?;
            return Err(io::Error::other("machine session has stopped"));
        }
        *owner = ProjectWriterSlot::Ready(writer.clone());
        Ok(writer)
    }

    pub(super) async fn writer_slot(&self, project: &ProjectId) -> WriterSlot {
        self.writers
            .lock()
            .await
            .entry(project.clone())
            .or_insert_with(|| Arc::new(Mutex::new(ProjectWriterSlot::Empty)))
            .clone()
    }

    pub async fn next_lifecycle_signal(&self) -> Option<ProjectLifecycleSignal> {
        self.lifecycle_requests.lock().await.recv().await
    }

    pub async fn prompt_project_renewal(&self, project: &ProjectId) -> io::Result<()> {
        if self.writer_is_terminal(project).await? {
            return Ok(());
        }
        let operation = self
            .ensure(project)
            .await?
            .runtime
            .prompt_renewal()
            .await
            .map_err(io::Error::other)?;
        #[cfg(test)]
        let _ = self.renewal_prompts.send((project.clone(), operation));
        #[cfg(not(test))]
        let _ = operation;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn subscribe_renewal_prompts(
        &self,
    ) -> tokio::sync::broadcast::Receiver<(ProjectId, runtime_primitives::OperationId)> {
        self.renewal_prompts.subscribe()
    }

    #[cfg(test)]
    pub(super) fn subscribe_registration_waits(
        &self,
    ) -> tokio::sync::broadcast::Receiver<ProjectId> {
        self.registration_waits.subscribe()
    }

    pub async fn prompt_all_renewals(&self) -> io::Result<()> {
        for project in self
            .registry()?
            .project_ids()
            .await
            .map_err(io::Error::other)?
        {
            if !self.writer_is_terminal(&project).await? {
                self.prompt_project_renewal(&project).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn prompt_superseded_writer(&self, project: &ProjectId) -> io::Result<()> {
        let slot = self.writer_slot(project).await;
        let writer = {
            let owner = slot.lock().await;
            match &*owner {
                ProjectWriterSlot::Installing(_) | ProjectWriterSlot::Terminal { .. } => {
                    return Ok(());
                }
                ProjectWriterSlot::Ready(writer) => Some(writer.clone()),
                ProjectWriterSlot::Empty => None,
            }
        };
        if let Some(writer) = writer {
            let operation = writer
                .runtime
                .prompt_renewal()
                .await
                .map_err(io::Error::other)?;
            #[cfg(test)]
            let _ = self.renewal_prompts.send((project.clone(), operation));
            #[cfg(not(test))]
            let _ = operation;
            return Ok(());
        }
        if self.superseded_current(project).await?.is_some()
            || self
                .registry()?
                .existing_project(project)
                .await
                .map_err(io::Error::other)?
                .is_none()
        {
            return Ok(());
        }
        self.prompt_project_renewal(project).await
    }

    pub async fn supersede_project(
        &self,
        project: &ProjectId,
        source: Weak<collaboration_service::NativeProjectStore>,
    ) -> io::Result<()> {
        let slot = self.writer_slot(project).await;
        let mut owner = slot.lock().await;
        let Some(source) = source.upgrade() else {
            return Ok(());
        };
        let writer = match &*owner {
            ProjectWriterSlot::Ready(writer)
            | ProjectWriterSlot::Terminal {
                writer: Some(writer),
            } if Arc::ptr_eq(&writer.store, &source) => writer.clone(),
            _ => return Ok(()),
        };
        // The subject and the registry close belong to this one project lease.
        // A delayed old signal cannot close a replacement writer between them.
        *owner = ProjectWriterSlot::Terminal {
            writer: Some(writer.clone()),
        };
        writer.stop().await?;
        self.registry()?
            .supersede_project(project)
            .await
            .map_err(io::Error::other)
    }

    pub async fn writer_is_terminal(&self, project: &ProjectId) -> io::Result<bool> {
        if self
            .registry()?
            .is_superseded(project)
            .await
            .map_err(io::Error::other)?
        {
            return Ok(true);
        }
        let slot = self.writers.lock().await.get(project).cloned();
        let Some(slot) = slot else {
            return Ok(false);
        };
        Ok(matches!(
            *slot.lock().await,
            ProjectWriterSlot::Terminal { .. }
        ))
    }

    pub async fn superseded_current(
        &self,
        project: &ProjectId,
    ) -> io::Result<Option<Option<EpochNumber>>> {
        Ok(self.registry()?.superseded_current(project).await)
    }

    /// Repair local snapshot baselines before admission, without starting client work.
    pub async fn prepare_existing(&self) -> io::Result<()> {
        for project in self
            .registry()?
            .project_ids()
            .await
            .map_err(io::Error::other)?
        {
            if let Err(error) = self.ensure(&project).await {
                tracing::warn!(?project, %error, "project runtime preparation unavailable");
            }
        }
        Ok(())
    }

    pub async fn activate_existing(&self) -> io::Result<()> {
        for project in self
            .registry()?
            .project_ids()
            .await
            .map_err(io::Error::other)?
        {
            match self.ensure(&project).await {
                Ok(writer) => {
                    if let Err(error) = writer.activate(&project).await {
                        tracing::warn!(?project, %error, "project runtime activation unavailable");
                    }
                }
                Err(error) => {
                    tracing::warn!(?project, %error, "project runtime preparation unavailable")
                }
            }
        }
        Ok(())
    }

    pub async fn settle_operations(&self) -> io::Result<()> {
        let mut operations = self.operations.lock().await;
        let mut failure = None;
        while let Some(result) = operations.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(error) => {
                    failure.get_or_insert(io::Error::other(error));
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub async fn shutdown(&self) -> io::Result<()> {
        let mut failure = self.settle_operations().await.err();
        let slots: Vec<_> = self.writers.lock().await.values().cloned().collect();
        for slot in slots {
            if let Err(error) = stop_writer_slot(&slot).await {
                failure.get_or_insert(error);
            }
        }
        self.writers.lock().await.clear();
        failure.map_or(Ok(()), Err)
    }
}

async fn stop_writer_slot(slot: &WriterSlot) -> io::Result<()> {
    let writer = {
        let mut owner = slot.lock().await;
        if let ProjectWriterSlot::Installing(install) = &*owner {
            install.stop().await?;
        }
        let writer = match &*owner {
            ProjectWriterSlot::Ready(writer) => Some(writer.clone()),
            ProjectWriterSlot::Terminal { writer } => writer.clone(),
            ProjectWriterSlot::Installing(_) | ProjectWriterSlot::Empty => None,
        };
        *owner = ProjectWriterSlot::Terminal {
            writer: writer.clone(),
        };
        writer
    };
    if let Some(writer) = writer {
        writer.stop().await?;
    }
    Ok(())
}

impl ProjectWriters {
    pub(super) async fn acknowledged_project_copy(
        &self,
        project: &ProjectId,
    ) -> Result<
        (
            Arc<collaboration_service::NativeProjectStore>,
            Option<fleet_sync_protocol::ReplicationPosition>,
        ),
        BoardError,
    > {
        // Both the copied position and the board mapping belong to this physical
        // owner's database. This reads durable progress without running transport.
        let registry = self
            .registry()
            .map_err(|_| BoardError::board_unavailable())?;
        let store = registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        let database = registry
            .database(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let calls = Arc::new(WriterSessionCalls::new(
            database.clone(),
            self.receiver,
            self.requests.clone(),
        ));
        let sender = ReplicationSender::new(
            database,
            Arc::new(BoardPieceAnnotation),
            Arc::new(BoardCaptureBookkeeping),
            calls,
            SenderOptions::default(),
        );
        let copies = sender
            .copies()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let position = match copies.into_iter().find(|copy| copy.copy == self.receiver) {
            Some(copy) if copy.role != CopyRole::Authoritative => {
                return Err(BoardError::board_unavailable());
            }
            Some(copy) => Some(copy.acked),
            None => None,
        };
        // Selection of the store and database crosses awaits. A replacement during
        // that read invalidates the pair, even when its numeric progress is newer.
        let current = registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        if !Arc::ptr_eq(&store, &current) {
            return Err(BoardError::board_unavailable());
        }
        Ok((store, position))
    }
}

impl ProjectReplicationPosition for ProjectWriters {
    fn confirmed_writer_epoch<'port>(
        &'port self,
        project: &'port ProjectId,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Option<fleet_sync_protocol::Epoch>> + Send + 'port>,
    > {
        Box::pin(async move {
            let registry = self.registry().ok()?;
            if registry.is_superseded(project).await.ok()? {
                return None;
            }
            let store = registry.existing_project(project).await.ok()??;
            match store.writer_state().await.ok()? {
                fleet_sync_lease::WriterState::Writing { epoch } => Some(epoch),
                _ => None,
            }
        })
    }
    fn replicated_through<'port>(
        &'port self,
        project: &'port ProjectId,
        epoch: EpochNumber,
    ) -> ReplicatedPositionFuture<'port> {
        Box::pin(async move {
            let (store, position) = self.acknowledged_project_copy(project).await?;
            match position {
                Some(position) => {
                    collaboration_service::project_replication_cursor(&store, position, epoch).await
                }
                None => Ok(FeedCursor {
                    epoch,
                    position: FeedPosition::ZERO,
                }),
            }
        })
    }

    fn acknowledged_mail_changes<'port>(
        &'port self,
        project: &'port ProjectId,
    ) -> collaboration_service::AcknowledgedMailChangesFuture<'port> {
        Box::pin(async move {
            let (store, position) = self.acknowledged_project_copy(project).await?;
            match position {
                Some(position) => store
                    .acknowledged_capture(position)
                    .await
                    .map_err(|_| BoardError::board_unavailable()),
                None => Ok(message_board_storage::AcknowledgedProjectChanges::default()),
            }
        })
    }
}
