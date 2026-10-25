//! Coalesced project notification work; native mail retains all custody authority.
mod handoff_scan;
mod intent_handoffs;
mod intent_scan;
use crate::{MailDeliveryRuntime, ProjectAdmission};
use interaction_storage::InteractionStore;
use message_board::{BoardError, ProjectId};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};
use tokio::sync::Mutex;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
mod cancel_expansions;
mod frontier_coverage;
mod generation_coverage;
mod handoff_retirement;
mod local_cancellation;
mod remote_cancellation;
mod settled_frontier;
pub(crate) use generation_coverage::GenerationPreventedFollow;
pub(crate) use settled_frontier::SettledMailPrefix;

const INTENT_PAGE_SIZE: usize = 100;
async fn publication_epoch(
    store: &crate::NativeProjectStore,
) -> Result<message_board::EpochNumber, BoardError> {
    publication_epoch_if_writable(store)
        .await?
        .ok_or_else(BoardError::board_unavailable)
}

async fn publication_epoch_if_writable(
    store: &crate::NativeProjectStore,
) -> Result<Option<message_board::EpochNumber>, BoardError> {
    match store
        .writer_state()
        .await
        .map_err(|_| BoardError::board_unavailable())?
    {
        fleet_sync_lease::WriterState::Provisional => Ok(Some(message_board::EpochNumber::INITIAL)),
        fleet_sync_lease::WriterState::Writing { epoch } => {
            message_board::EpochNumber::try_from(epoch.value())
                .map(Some)
                .map_err(|_| BoardError::board_unavailable())
        }
        _ => Ok(None),
    }
}
enum IntentHandling {
    Consumed,
    Pending,
}
enum ProjectWork {
    Running,
    RescanRequested,
}
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum ProjectWorkStream {
    Publication,
    Settlement,
}

struct PublisherInner {
    admission: Arc<ProjectAdmission>,
    mail: Weak<MailDeliveryRuntime>,
    progress: Arc<InteractionStore>,
    projects: Mutex<HashMap<(ProjectId, ProjectWorkStream), ProjectWork>>,
    shutdown: CancellationToken,
    tasks: TaskTracker,
}

pub struct NotificationPublisher {
    inner: Arc<PublisherInner>,
}

impl NotificationPublisher {
    pub(crate) async fn prompt_terminal_item(
        &self,
        item: fleet_sync_protocol::ItemId,
    ) -> Result<(), BoardError> {
        let id = item
            .as_uuid()
            .to_string()
            .try_into()
            .map_err(|_| BoardError::board_unavailable())?;
        for project in self
            .inner
            .admission
            .registry
            .project_ids()
            .await
            .map_err(|_| BoardError::board_unavailable())?
        {
            let Some(store) = self
                .inner
                .admission
                .registry
                .existing_project(&project)
                .await
                .map_err(|_| BoardError::board_unavailable())?
            else {
                continue;
            };
            let mut read = store
                .snapshot()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            let exists = read
                .mail_handoff(&id)
                .await
                .map_err(|_| BoardError::board_unavailable())?
                .is_some();
            read.finish()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            if exists {
                self.queue_project(project).await?;
            }
        }
        Ok(())
    }
    #[must_use]
    pub fn new(
        admission: Arc<ProjectAdmission>,
        progress: Arc<InteractionStore>,
        mail: Weak<MailDeliveryRuntime>,
    ) -> Self {
        Self {
            inner: Arc::new(PublisherInner {
                admission,
                mail,
                progress,
                projects: Mutex::new(HashMap::new()),
                shutdown: CancellationToken::new(),
                tasks: TaskTracker::new(),
            }),
        }
    }

    /// Hints coalesce; every pass reads the project and its current durable marks.
    pub async fn queue_project(&self, project: ProjectId) -> Result<(), BoardError> {
        let mut work = self.inner.projects.lock().await;
        if self.inner.shutdown.is_cancelled() {
            return Err(BoardError::board_unavailable());
        }
        let key = (project.clone(), ProjectWorkStream::Publication);
        if let Some(state) = work.get_mut(&key) {
            *state = ProjectWork::RescanRequested;
            return Ok(());
        }
        work.insert(key.clone(), ProjectWork::Running);
        let inner = self.inner.clone();
        self.inner.tasks.spawn(async move {
            loop {
                if let Err(error) = inner.consume_project_intents(&project).await {
                    tracing::warn!(%error, project = project.as_str(), "notification publication delayed");
                }
                match inner.publish_project_handoffs(&project).await {
                    Err(error) => tracing::warn!(%error, project = project.as_str(), "frozen handoff publication delayed"),
                    Ok(()) => {
                        inner.queue_project_settlement(project.clone()).await;
                    }
                }
                let mut work = inner.projects.lock().await;
                if matches!(work.remove(&key), Some(ProjectWork::RescanRequested))
                    && !inner.shutdown.is_cancelled() {
                    work.insert(key.clone(), ProjectWork::Running);
                    continue;
                }
                break;
            }
        });
        Ok(())
    }

    pub(crate) async fn follow_recipient_enrollment(
        &self,
        service: &message_board::ServiceId,
    ) -> Result<crate::IntentRecipientEnrollment, BoardError> {
        let runtime = self
            .inner
            .mail
            .upgrade()
            .ok_or_else(BoardError::board_unavailable)?;
        let peer = runtime
            .resolve_installation_peer(service)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        Ok(crate::IntentRecipientEnrollment {
            service: service.clone(),
            peer,
        })
    }

    pub async fn recover(&self) -> Result<(), BoardError> {
        for project in self
            .inner
            .admission
            .registry
            .project_ids()
            .await
            .map_err(|_| BoardError::board_unavailable())?
        {
            self.queue_project(project).await?;
        }
        Ok(())
    }

    pub(crate) async fn prompt_follow_coverage(
        &self,
        projects: &[ProjectId],
    ) -> Result<(), BoardError> {
        // The native submission projection supplies the item's owning project.
        // The existing activity channel only hints; readers still derive debt.
        for project in projects {
            self.inner.admission.registry.notify_activity(project);
            self.queue_project(project.clone()).await?;
        }
        Ok(())
    }

    pub fn cancel(&self) {
        self.inner.shutdown.cancel();
    }

    pub async fn shutdown(&self) {
        // The queue and close share this gate, so no task can enter after drain begins.
        let work = self.inner.projects.lock().await;
        self.cancel();
        self.inner.tasks.close();
        drop(work);
        self.inner.tasks.wait().await;
    }

    #[cfg(any(test, feature = "runtime-test-support"))]
    #[doc(hidden)]
    pub async fn wait_project_idle_for_test(&self, project: &ProjectId) {
        loop {
            if !self
                .inner
                .projects
                .lock()
                .await
                .keys()
                .any(|(active, _)| active == project)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn advance_project_frontier_for_test(
        &self,
        project: &ProjectId,
    ) -> Result<(), BoardError> {
        self.inner.advance_project_frontier(project).await
    }

    #[cfg(test)]
    pub(crate) async fn consume_project_intents(
        &self,
        project: &ProjectId,
    ) -> Result<(), BoardError> {
        self.inner.consume_project_intents(project).await
    }
}
impl PublisherInner {
    /// Settlement may await another mailbox; publication keeps its own hint state.
    async fn queue_project_settlement(self: &Arc<Self>, project: ProjectId) {
        let mut work = self.projects.lock().await;
        if self.shutdown.is_cancelled() {
            return;
        }
        let key = (project.clone(), ProjectWorkStream::Settlement);
        if let Some(state) = work.get_mut(&key) {
            *state = ProjectWork::RescanRequested;
            return;
        }
        work.insert(key.clone(), ProjectWork::Running);
        let inner = self.clone();
        self.tasks.spawn(async move {
            loop {
                if let Err(error) = inner.advance_project_frontier(&project).await {
                    tracing::warn!(%error, project = project.as_str(), "settled frontier delayed");
                }
                if let Err(error) = inner.retire_project_handoffs(&project).await {
                    tracing::warn!(%error, project = project.as_str(), "acknowledged handoff retirement delayed");
                }
                let mut work = inner.projects.lock().await;
                if matches!(work.remove(&key), Some(ProjectWork::RescanRequested))
                    && !inner.shutdown.is_cancelled()
                {
                    work.insert(key.clone(), ProjectWork::Running);
                    continue;
                }
                break;
            }
        });
    }
}
impl Drop for NotificationPublisher {
    fn drop(&mut self) {
        self.cancel();
        self.inner.tasks.close();
    }
}
