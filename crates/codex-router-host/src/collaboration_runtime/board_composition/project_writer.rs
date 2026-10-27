//! One project's native journal and sender, joined before the file is moved or closed.
use super::project_writers::ProjectLifecycleSignal;
use board_fleet_sync::{BoardCaptureBookkeeping, BoardPieceAnnotation, BoardProjectSchema};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_service::{NativeProjectStore, ProjectStoreRegistry};
use fleet_sync_lease::{HolderError, WriterState};
use fleet_sync_protocol::{PieceRefusal, ReceiverId};
use fleet_sync_tokio_runner::{NativeWriterRuntime, SessionRequests, WriterSessionCalls};
use fleet_sync_writer::{ReplicationEvents, ReplicationSender, SenderError, SenderOptions};
use message_board::ProjectId;
use message_board_storage::ProjectStorageError;
use std::{io, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, OnceCell, mpsc, watch},
    task::JoinSet,
};

// Retry failed copies after 500 ms; idle copies wait on commit events.
const REPLICATION_ERROR_RETRY_AFTER: Duration = Duration::from_millis(500);

type BoardWriterRuntime = NativeWriterRuntime<BoardProjectSchema, BoardConsumerProtocol>;
type BoardReplicationSender = ReplicationSender<
    BoardProjectSchema,
    BoardPieceAnnotation,
    BoardCaptureBookkeeping,
    WriterSessionCalls<BoardProjectSchema, BoardConsumerProtocol>,
>;

enum WriterTasks {
    Prepared,
    Running(JoinSet<io::Result<()>>),
    Stopped,
}

pub(super) struct ProjectWriter {
    pub runtime: Arc<BoardWriterRuntime>,
    pub sender: Arc<BoardReplicationSender>,
    pub store: Arc<NativeProjectStore>,
    pub registration_watch: OnceCell<()>,
    machine_stop: watch::Receiver<bool>,
    stop: watch::Sender<bool>,
    tasks: Mutex<WriterTasks>,
    authority_changes: watch::Receiver<()>,
    lifecycle_signals: mpsc::UnboundedSender<ProjectLifecycleSignal>,
}

impl ProjectWriter {
    pub async fn open(
        registry: &ProjectStoreRegistry,
        project: &ProjectId,
        receiver: ReceiverId,
        requests: SessionRequests<BoardConsumerProtocol>,
        machine_stop: watch::Receiver<bool>,
        lifecycle_signals: mpsc::UnboundedSender<ProjectLifecycleSignal>,
    ) -> io::Result<Self> {
        // A server call or view must never create an unknown project.
        let store = registry
            .existing_project(project)
            .await
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("project has no live native file"))?;
        let database = registry.database(project).await.map_err(io::Error::other)?;
        let runtime = Arc::new(
            NativeWriterRuntime::new(database.clone(), receiver, requests)
                .await
                .map_err(io::Error::other)?,
        );
        if !matches!(
            store.writer_state().await.map_err(io::Error::other)?,
            WriterState::Seeding { .. }
        ) && let Some(receipt) = database
            .installed_snapshot()
            .await
            .map_err(io::Error::other)?
        {
            // The per-project OnceCell serializes this initialization. Admission's
            // native guard refuses board effects until this exact receipt is marked.
            runtime
                .store()
                .rule_transaction(move |scope| {
                    Box::pin(async move {
                        message_board_storage::record_snapshot_capture(scope, &receipt)
                            .await
                            .map_err(durable_steps::StoreError::backend)
                    })
                })
                .await
                .map_err(io::Error::other)?;
        }
        let authority_changes = database.subscribe_authority_changes();
        let sender = Arc::new(ReplicationSender::new(
            database,
            Arc::new(BoardPieceAnnotation),
            Arc::new(BoardCaptureBookkeeping),
            runtime.transport(),
            SenderOptions {
                retry_after: REPLICATION_ERROR_RETRY_AFTER,
                ..SenderOptions::default()
            },
        ));
        Ok(Self {
            runtime,
            sender,
            store,
            registration_watch: OnceCell::new(),
            machine_stop,
            stop: watch::channel(false).0,
            tasks: Mutex::new(WriterTasks::Prepared),
            authority_changes,
            lifecycle_signals,
        })
    }

    /// Only the machine's Welcome handler or its ready registration port calls this.
    pub async fn activate(&self, project: &ProjectId) -> io::Result<()> {
        let mut owned = self.tasks.lock().await;
        match &*owned {
            WriterTasks::Running(_) => return Ok(()),
            WriterTasks::Stopped => return Err(io::Error::other("project runtime has stopped")),
            WriterTasks::Prepared => {}
        }
        let mut tasks = JoinSet::new();
        let stop = self.stop.clone();
        let mut machine_stop = self.machine_stop.clone();
        let mut project_stop = self.stop.subscribe();
        tasks.spawn(async move {
            loop {
                if *machine_stop.borrow() {
                    stop.send_replace(true);
                    return Ok(());
                }
                tokio::select! {
                    changed = machine_stop.changed() => {
                        if changed.is_err() {
                            stop.send_replace(true);
                            return Ok(());
                        }
                    }
                    _ = project_stop.changed() => return Ok(()),
                }
            }
        });
        let runtime = self.runtime.clone();
        let stopped = self.stop.subscribe();
        tasks.spawn(async move { runtime.run(stopped).await.map_err(io::Error::other) });
        let sender = self.sender.clone();
        let sender_stopped = self.stop.subscribe();
        let (events, mut received) =
            mpsc::channel::<Result<ReplicationEvents, fleet_sync_writer::CopyFailure>>(128);
        let project = project.clone();
        let mut stopped = self.stop.subscribe();
        let mut authority = self.authority_changes.clone();
        let store = self.store.clone();
        let lifecycle = self.lifecycle_signals.clone();
        let terminal_stop = self.stop.clone();
        let observing = async move {
            let mut was_writable = false;
            loop {
                if *stopped.borrow() {
                    return Ok(());
                }
                match store.writer_state().await {
                    Ok(WriterState::Writing { .. }) => {
                        if !was_writable {
                            lifecycle
                                .send(ProjectLifecycleSignal::WritingConfirmed(project.clone()))
                                .map_err(io::Error::other)?;
                        }
                        was_writable = true;
                    }
                    Ok(WriterState::Superseded { .. }) => {
                        lifecycle
                            .send(ProjectLifecycleSignal::Superseded {
                                project: project.clone(),
                                source: Arc::downgrade(&store),
                            })
                            .map_err(io::Error::other)?;
                        return Ok(());
                    }
                    Err(ProjectStorageError::Writer(fleet_sync_writer::OpenError::Holder {
                        error: HolderError::RegistrationRejected { .. },
                    })) => return Ok(()), // The registration watcher owns orphaning.
                    Err(error) => return Err(io::Error::other(error)),
                    Ok(_) => was_writable = false,
                }
                tokio::select! {
                    _ = stopped.changed() => return Ok(()),
                    changed = authority.changed() => {
                        if changed.is_err() { return Ok(()); }
                    },
                    event = received.recv() => match event {
                        Some(Ok(ReplicationEvents::LeaseChanged(WriterState::Superseded { .. }))) => {
                            lifecycle.send(ProjectLifecycleSignal::Superseded { project: project.clone(), source: Arc::downgrade(&store) }).map_err(io::Error::other)?;
                            return Ok(());
                        },
                        Some(Ok(ReplicationEvents::Acknowledged { .. })) => {
                            lifecycle.send(ProjectLifecycleSignal::Acknowledged(project.clone())).map_err(io::Error::other)?;
                        },
                        Some(Ok(ReplicationEvents::LeaseChanged(WriterState::Writing { .. }))) => {},
                        Some(Ok(event)) => tracing::debug!(?project, ?event, "native project replication event"),
                        Some(Err(error)) => match &error.error {
                            SenderError::Refused { reason: PieceRefusal::IntegrityViolation { table, references }, .. } => {
                                tracing::error!(?project, table = table.as_str(), references = references.as_str(), %error, "native project replication stopped after integrity refusal");
                                terminal_stop.send_replace(true);
                                return Err(io::Error::other(error));
                            },
                            _ => tracing::warn!(?project, %error, "native project copy delayed"),
                        },
                        None => return Ok(()),
                    }
                }
            }
        };
        tasks.spawn(async move {
            // The owned result cannot escape while this project's sender can
            // still retry. The project stop leaves other project owners alone.
            let (sending, observed) = tokio::join!(sender.run(sender_stopped, events), observing);
            observed?;
            sending.map_err(io::Error::other)
        });
        *owned = WriterTasks::Running(tasks);
        Ok(())
    }

    #[cfg(all(test, feature = "runtime-test-support"))]
    pub(super) async fn join_without_stopping_for_test(&self) -> io::Result<()> {
        let mut owned = self.tasks.lock().await;
        let WriterTasks::Running(children) = &mut *owned else {
            return Err(io::Error::other("writer was not running"));
        };
        let mut failure = None;
        while let Some(result) = children.join_next().await {
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
        *owned = WriterTasks::Stopped;
        failure.map_or(Ok(()), Err)
    }

    pub async fn stop(&self) -> io::Result<()> {
        stop_writer_tasks(&self.tasks, &self.stop).await
    }
}

async fn stop_writer_tasks(
    tasks: &Mutex<WriterTasks>,
    stop: &watch::Sender<bool>,
) -> io::Result<()> {
    let mut owned = tasks.lock().await;
    stop.send_replace(true);
    let WriterTasks::Running(tasks) = &mut *owned else {
        *owned = WriterTasks::Stopped;
        return Ok(());
    };
    let mut failure = None;
    while let Some(result) = tasks.join_next().await {
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
    *owned = WriterTasks::Stopped;
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod stop_tests {
    #![allow(clippy::panic_in_result_fn)]
    use super::*;
    use std::task::Poll;
    use tokio::sync::oneshot;

    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[tokio::test]
    async fn overlapping_stops_both_wait_for_the_owned_task_to_finish() -> TestResult {
        let (release, held) = oneshot::channel::<()>();
        let mut children = JoinSet::new();
        children.spawn(async move { held.await.map_err(io::Error::other) });
        let tasks = Mutex::new(WriterTasks::Running(children));
        let (signal, _) = watch::channel(false);
        let mut first = Box::pin(stop_writer_tasks(&tasks, &signal));
        assert!(matches!(futures_util::poll!(&mut first), Poll::Pending));
        let mut second = Box::pin(stop_writer_tasks(&tasks, &signal));
        assert!(matches!(futures_util::poll!(&mut second), Poll::Pending));
        release
            .send(())
            .map_err(|_| io::Error::other("child was aborted"))?;
        let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            tokio::join!(first, second)
        })
        .await?;
        first?;
        second?;
        assert!(matches!(*tasks.lock().await, WriterTasks::Stopped));
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_stop_leaves_the_same_owned_task_for_the_next_drain() -> TestResult {
        let (release, held) = oneshot::channel::<()>();
        let mut children = JoinSet::new();
        children.spawn(async move { held.await.map_err(io::Error::other) });
        let tasks = Mutex::new(WriterTasks::Running(children));
        let (signal, _) = watch::channel(false);
        let mut first = Box::pin(stop_writer_tasks(&tasks, &signal));
        assert!(matches!(futures_util::poll!(&mut first), Poll::Pending));
        drop(first);
        let mut retry = Box::pin(stop_writer_tasks(&tasks, &signal));
        assert!(matches!(futures_util::poll!(&mut retry), Poll::Pending));
        release
            .send(())
            .map_err(|_| io::Error::other("cancelled drain aborted its child"))?;
        tokio::time::timeout(std::time::Duration::from_secs(30), retry).await??;
        assert!(matches!(*tasks.lock().await, WriterTasks::Stopped));
        Ok(())
    }
}
