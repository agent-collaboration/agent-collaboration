//! One durable client and pump carry every project's native runtime requests.
use super::{
    machine_notice_reactions::MachineNoticeReactions,
    notice_receipts::NoticeReceipts,
    project_writers::{ProjectLifecycleSignal, ProjectWriters},
    server_calls::answer_server_call,
};
use board_fleet_sync::{
    BoardProjectSchema, BoardSessionInputs, MachineSessionStore, ReqwestSessionTransport,
};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_runtime::FleetClient;
use collaboration_runtime::ProjectStoreRegistry;
use collaboration_service::MailDeliveryRuntime;
use collaboration_service::NotificationPublisher;
use collaboration_service::ProjectAdmission;
use fleet_sync_protocol::{DatabaseStatus, Notice, NoticePosition, PeerId, SchemaSupport};
use fleet_sync_session::{ClientEvent, SessionClient};
use fleet_sync_tokio_runner::{SessionPump, SessionRequests};
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{
    future::Future,
    io,
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};

#[path = "machine_notice_dispatch.rs"]
mod notice_dispatch;

const SESSION_CALL_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAIL_RECEIVE_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MAIL_STATUS_INTERVAL: Duration = Duration::from_secs(10 * 60);
pub(super) const FLEET_REFRESH_PERIOD: Duration = Duration::from_secs(60);

fn mail_status_interval() -> Duration {
    #[cfg(feature = "runtime-test-support")]
    if let Some(milliseconds) = std::env::var("CODEX_ROUTER_TEST_MAIL_STATUS_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
    {
        return Duration::from_millis(milliseconds);
    }
    MAIL_STATUS_INTERVAL
}

#[cfg(test)]
#[path = "tests/mail_notice_events.rs"]
#[expect(
    dead_code,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod mail_notice_events;

type BoardSessionPump =
    SessionPump<ReqwestSessionTransport, MachineSessionStore, BoardConsumerProtocol>;

#[cfg(all(debug_assertions, feature = "runtime-test-support"))]
#[path = "machine_test_progress.rs"]
mod machine_test_progress;
#[cfg(all(debug_assertions, feature = "runtime-test-support"))]
use machine_test_progress::MachineProgress;

pub(super) struct PreparedMachineSession {
    pump: BoardSessionPump,
    events: mpsc::Receiver<ClientEvent<BoardConsumerProtocol>>,
    pub(super) requests: SessionRequests<BoardConsumerProtocol>,
    pub(super) peer: PeerId,
    pub(super) relay: PeerId,
    notices: MachineNoticeReactions,
    pub projects: Arc<ProjectWriters>,
    pub(super) welcomed: watch::Sender<bool>,
    stop: watch::Sender<bool>,
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    progress: MachineProgress,
}

pub(crate) struct RunningMachineSession {
    stop: watch::Sender<bool>,
    task: JoinSet<io::Result<()>>,
    #[cfg(any(test, feature = "runtime-test-support"))]
    pub(super) projects: Weak<ProjectWriters>,
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    progress: MachineProgress,
}

impl PreparedMachineSession {
    pub async fn prepare(
        directory: &Path,
        inputs: BoardSessionInputs,
        registry: &Arc<ProjectStoreRegistry>,
    ) -> io::Result<Self> {
        let schema = BoardProjectSchema::new().map_err(io::Error::other)?;
        let max_version = schema
            .supported_schemas()
            .iter()
            .map(|schema| schema.version)
            .max()
            .ok_or_else(|| io::Error::other("board schema support is empty"))?;
        let transport = ReqwestSessionTransport::new(
            inputs.access,
            SESSION_CONNECT_TIMEOUT,
            SESSION_CALL_TIMEOUT,
        )
        .map_err(io::Error::other)?;
        let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
            transport,
            MachineSessionStore::open(directory)
                .await
                .map_err(io::Error::other)?,
            &UuidV7Ids,
            SchemaSupport { max_version },
            Backoff {
                initial: Duration::from_secs(1),
                max: Duration::from_secs(30),
                factor: 2,
            },
        )
        .await
        .map_err(io::Error::other)?;
        let (requests, events, pump) = SessionPump::new(
            client,
            inputs.endpoint,
            inputs.credential,
            SESSION_CALL_TIMEOUT,
        );
        let (welcomed, readiness) = watch::channel(false);
        let (stop, stopped) = watch::channel(false);
        registry
            .attach_central_reads(Arc::new(super::central_read_copy::CentralReadCopies::new(
                directory,
                requests.clone(),
                inputs.receiver,
                readiness.clone(),
            )))
            .map_err(io::Error::other)?;
        let projects = Arc::new(ProjectWriters::new(
            directory,
            inputs.peer,
            registry,
            requests.clone(),
            inputs.receiver,
            readiness,
            stopped,
        ));
        projects.prepare_existing().await?;
        let notices = MachineNoticeReactions::open(directory).await?;
        Ok(Self {
            pump,
            events,
            requests,
            peer: inputs.peer,
            relay: inputs.relay,
            notices,
            projects,
            welcomed,
            stop,
            #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
            progress: MachineProgress::default(),
        })
    }

    /// Activation follows successful Host assembly; preparation starts no task or socket.
    pub fn start(
        self,
        admission: Weak<ProjectAdmission>,
        fleet: Weak<FleetClient>,
        mail: Weak<MailDeliveryRuntime>,
        publisher: Weak<NotificationPublisher>,
    ) -> RunningMachineSession {
        let stop = self.stop.clone();
        #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
        let progress = self.progress.clone();
        #[cfg(any(test, feature = "runtime-test-support"))]
        let projects = Arc::downgrade(&self.projects);
        let mut task = JoinSet::new();
        task.spawn(self.run(admission, fleet, mail, publisher));
        RunningMachineSession {
            stop,
            task,
            #[cfg(any(test, feature = "runtime-test-support"))]
            projects,
            #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
            progress,
        }
    }

    async fn run(
        self,
        admission: Weak<ProjectAdmission>,
        fleet: Weak<FleetClient>,
        mail: Weak<MailDeliveryRuntime>,
        publisher: Weak<NotificationPublisher>,
    ) -> io::Result<()> {
        let Self {
            pump,
            events,
            requests,
            notices,
            projects,
            welcomed,
            stop,
            #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
            progress,
            ..
        } = self;
        let mut tasks = JoinSet::new();
        let stopped = stop.subscribe();
        tasks.spawn(async move { pump.run(stopped).await.map_err(io::Error::other) });
        let stopped = stop.subscribe();
        let handling = projects.clone();
        tasks.spawn(run_events(
            events,
            requests,
            notices,
            MachineSessionEventOwners {
                projects: handling,
                admission,
                fleet,
                mail,
                publisher,
                #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
                progress,
            },
            welcomed,
            stopped,
        ));
        let first = tasks.join_next().await;
        let requested = *stop.borrow();
        stop.send_replace(true);
        let mut failure = match first {
            Some(Ok(Err(error))) => Some(error),
            Some(Err(error)) => Some(io::Error::other(error)),
            _ if !requested => Some(io::Error::other("machine session stopped unexpectedly")),
            _ => None,
        };
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
        if let Err(error) = projects.shutdown().await {
            failure.get_or_insert(error);
        }
        failure.map_or(Ok(()), Err)
    }
}

impl RunningMachineSession {
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    pub(in crate::collaboration_runtime) fn observe_progress(&self) -> watch::Receiver<u64> {
        self.progress.observe()
    }

    #[cfg(feature = "runtime-test-support")]
    pub(in crate::collaboration_runtime) async fn replicate_project_for_test(
        &self,
        project: &message_board::ProjectId,
    ) -> io::Result<()> {
        self.projects
            .upgrade()
            .ok_or_else(|| io::Error::other("test project runtime is unavailable"))?
            .replicate_project_for_test(project)
            .await
    }

    pub async fn shutdown(mut self) -> io::Result<()> {
        self.stop.send_replace(true);
        match self.task.join_next().await {
            Some(Ok(result)) => result,
            Some(Err(error)) => Err(io::Error::other(error)),
            None => Ok(()),
        }
    }
}

impl Drop for RunningMachineSession {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}

enum SessionCompletion {
    Operation(io::Result<()>),
    Reaction(notice_dispatch::ReactionCompletion),
}

struct MachineSessionEventOwners {
    projects: Arc<ProjectWriters>,
    admission: Weak<ProjectAdmission>,
    fleet: Weak<FleetClient>,
    mail: Weak<MailDeliveryRuntime>,
    publisher: Weak<NotificationPublisher>,
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    progress: MachineProgress,
}

async fn run_events(
    mut events: mpsc::Receiver<ClientEvent<BoardConsumerProtocol>>,
    requests: SessionRequests<BoardConsumerProtocol>,
    notices: MachineNoticeReactions,
    owners: MachineSessionEventOwners,
    welcomed: watch::Sender<bool>,
    mut stopped: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut handlers = JoinSet::new();
    let mut refresh = tokio::time::interval_at(
        tokio::time::Instant::now() + FLEET_REFRESH_PERIOD,
        FLEET_REFRESH_PERIOD,
    );
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut deferred_notices = Vec::new();
    let mut receipts = NoticeReceipts::default();
    let mut generation = 0_u64;
    let mut reactions = notice_dispatch::NoticeReactionQueue::new(notices);
    let mut mail_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + MAIL_RECEIVE_INTERVAL,
        MAIL_RECEIVE_INTERVAL,
    );
    mail_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let status_period = mail_status_interval();
    let mut status_interval =
        tokio::time::interval_at(tokio::time::Instant::now() + status_period, status_period);
    status_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *stopped.borrow() {
            break;
        }
        reactions.dispatch_next(&mut handlers, &owners, &requests);
        tokio::select! {
            _ = stopped.changed() => break,
            _ = mail_interval.tick(), if *welcomed.borrow() => {
                let mail = owners.mail.clone();
                spawn_operation(&mut handlers, async move { prompt_mail_receive(&mail).await });
            },
            _ = status_interval.tick(), if *welcomed.borrow() => {
                spawn_status_catch_up(&mut handlers, &owners.mail);
            },
            _ = refresh.tick(), if *welcomed.borrow() => {
                spawn_fleet_refresh(&mut handlers, &owners);
                spawn_installation_directory_refresh(&mut handlers, &owners.mail);
            },
            Some(signal) = owners.projects.next_lifecycle_signal() => {
                spawn_project_lifecycle(&mut handlers, &owners, signal);
            },
            result = handlers.join_next(), if !handlers.is_empty() => {
                match result {
                Some(Ok(SessionCompletion::Reaction(completed))) => {
                    finish_reaction(completed, generation, &mut reactions, &mut receipts, &requests, &owners.projects).await?;
                }
                Some(Ok(SessionCompletion::Operation(Err(error)))) => tracing::warn!(%error, "machine-session operation unavailable"),
                Some(Err(error)) => tracing::warn!(%error, "machine-session operation task failed"),
                _ => {},
                }
                #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
                owners.progress.record_completion();
            },
            event = events.recv() => match event {
                Some(ClientEvent::Welcome { databases }) => {
                    generation = generation.checked_add(1).ok_or_else(|| io::Error::other("notice generation exhausted"))?;
                    receipts = NoticeReceipts::default();
                    welcomed.send_replace(true);
                    mail_interval.reset();
                    spawn_installation_directory_refresh(&mut handlers, &owners.mail);
                    status_interval.reset();
                    spawn_welcome_recovery(&mut handlers, &owners, databases);
                    reactions.catch_up(generation);
                    for (position, notice) in deferred_notices.drain(..) {
                        queue_notice(&mut reactions, &mut receipts, generation, position, notice);
                    }
                }
                Some(ClientEvent::Request { frame, body }) => {
                    spawn_operation(&mut handlers, answer_server_call(owners.projects.clone(), owners.admission.clone(), owners.mail.clone(), owners.publisher.clone(), requests.clone(), frame, body));
                }
                Some(ClientEvent::Notice { position, body }) => {
                    if *welcomed.borrow() {
                        queue_notice(&mut reactions, &mut receipts, generation, position, body);
                    } else {
                        deferred_notices.push((position, body));
                    }
                }
                Some(ClientEvent::Disconnected { .. }) => {
                    welcomed.send_replace(false);
                    generation = generation.checked_add(1).ok_or_else(|| io::Error::other("notice generation exhausted"))?;
                    deferred_notices.clear();
                    reactions.clear_queued();
                    receipts = NoticeReceipts::default();
                }
                Some(ClientEvent::Unknown { .. } | ClientEvent::Answer { .. }) => {}
                None => break,
            },
        }
    }
    welcomed.send_replace(false);
    handlers.abort_all();
    while handlers.join_next().await.is_some() {}
    Ok(())
}

fn spawn_operation(
    handlers: &mut JoinSet<SessionCompletion>,
    operation: impl Future<Output = io::Result<()>> + Send + 'static,
) {
    handlers.spawn(async move { SessionCompletion::Operation(operation.await) });
}

fn spawn_status_catch_up(
    handlers: &mut JoinSet<SessionCompletion>,
    mail: &Weak<MailDeliveryRuntime>,
) {
    let mail = mail.clone();
    spawn_operation(handlers, async move {
        if let Some(mail) = mail.upgrade() {
            mail.catch_up_status().await.map_err(io::Error::other)?;
        }
        Ok(())
    });
}

fn spawn_welcome_recovery(
    handlers: &mut JoinSet<SessionCompletion>,
    owners: &MachineSessionEventOwners,
    databases: Vec<DatabaseStatus>,
) {
    let mail = owners.mail.clone();
    spawn_operation(handlers, async move { prompt_mail_receive(&mail).await });
    spawn_status_catch_up(handlers, &owners.mail);
    let publisher = owners.publisher.clone();
    spawn_operation(handlers, async move {
        if let Some(publisher) = publisher.upgrade() {
            publisher.recover().await.map_err(io::Error::other)?;
        }
        Ok(())
    });
    let recovery = owners.projects.clone();
    let recovering_fleet = owners.fleet.clone();
    let admission = owners.admission.clone();
    spawn_operation(handlers, async move {
        recovery.activate_existing().await?;
        super::archive_observation::synchronize_and_observe(
            &recovery,
            &admission,
            &recovering_fleet,
        )
        .await?;
        if let Some(admission) = admission.upgrade() {
            admission
                .recover_creating_projects()
                .await
                .map_err(io::Error::other)?;
        }
        Ok(())
    });
    for status in databases {
        let (database, lease) = match status {
            DatabaseStatus::Ready {
                database, lease, ..
            }
            | DatabaseStatus::Unavailable {
                database,
                lease: Some(lease),
                ..
            } => (database, lease),
            _ => continue,
        };
        if lease.holder == owners.projects.peer {
            let owner = owners.projects.clone();
            spawn_operation(handlers, async move {
                let project = message_board::ProjectId::try_from(database.to_string())
                    .map_err(io::Error::other)?;
                owner.receive_grant(&project, lease.epoch).await
            });
        }
    }
}

fn spawn_project_lifecycle(
    handlers: &mut JoinSet<SessionCompletion>,
    owners: &MachineSessionEventOwners,
    signal: ProjectLifecycleSignal,
) {
    if let ProjectLifecycleSignal::WritingConfirmed(project) = &signal {
        let project = project.clone();
        let mail = owners.mail.clone();
        spawn_operation(handlers, async move {
            if let Some(mail) = mail.upgrade() {
                mail.subscription_delivery()
                    .start_inherited_project_readers(&project)
                    .await
                    .map_err(io::Error::other)?;
            }
            Ok(())
        });
    }
    match signal {
        ProjectLifecycleSignal::Superseded { project, source } => {
            let owner = owners.projects.clone();
            spawn_operation(handlers, async move {
                owner.supersede_project(&project, source).await
            });
        }
        ProjectLifecycleSignal::Acknowledged(project)
        | ProjectLifecycleSignal::WritingConfirmed(project) => {
            let publisher = owners.publisher.clone();
            let publishing_project = project.clone();
            spawn_operation(handlers, async move {
                if let Some(publisher) = publisher.upgrade() {
                    publisher
                        .queue_project(publishing_project)
                        .await
                        .map_err(io::Error::other)?;
                }
                Ok(())
            });
            let projects = owners.projects.clone();
            let admission = owners.admission.clone();
            let fleet = owners.fleet.clone();
            spawn_operation(handlers, async move {
                super::archive_observation::observe_acknowledged_project(
                    &projects, &admission, &fleet, &project,
                )
                .await
            });
        }
        ProjectLifecycleSignal::RegistrationReady(project) => {
            let admission = owners.admission.clone();
            spawn_operation(handlers, async move {
                let admission = admission
                    .upgrade()
                    .ok_or_else(|| io::Error::other("project admission has closed"))?;
                admission
                    .resume_creating_project(&project)
                    .await
                    .map_err(io::Error::other)
            });
        }
    }
}

fn spawn_fleet_refresh(
    handlers: &mut JoinSet<SessionCompletion>,
    owners: &MachineSessionEventOwners,
) {
    let projects = owners.projects.clone();
    let admission = owners.admission.clone();
    let fleet = owners.fleet.clone();
    spawn_operation(handlers, async move {
        super::archive_observation::synchronize_and_observe(&projects, &admission, &fleet).await
    });
}

fn spawn_installation_directory_refresh(
    handlers: &mut JoinSet<SessionCompletion>,
    mail: &Weak<MailDeliveryRuntime>,
) {
    let mail = mail.clone();
    spawn_operation(handlers, async move {
        if let Some(mail) = mail.upgrade() {
            mail.refresh_installation_peers()
                .await
                .map_err(io::Error::other)?;
        }
        Ok(())
    });
}

fn queue_notice(
    reactions: &mut notice_dispatch::NoticeReactionQueue,
    receipts: &mut NoticeReceipts,
    generation: u64,
    position: NoticePosition,
    notice: Notice<BoardConsumerProtocol>,
) {
    if receipts.begin(position) {
        reactions.notice(generation, position, notice);
    }
}

// This transport owner alone sends ACKs. The separate consumer progress file
// records machine-read positions, which may also advance through catch-up.
async fn acknowledge_handled_prefix(
    requests: &SessionRequests<BoardConsumerProtocol>,
    receipts: &mut NoticeReceipts,
) {
    if let Some(position) = receipts.handled_prefix() {
        match requests.acknowledge_notice(position).await {
            Ok(()) => receipts.acknowledged(position),
            Err(error) => {
                receipts.acknowledgement_failed();
                tracing::warn!(%error, ?position, "notice acknowledgement unavailable");
            }
        }
    }
}

async fn prompt_mail_receive(mail: &Weak<MailDeliveryRuntime>) -> io::Result<()> {
    if let Some(mail) = mail.upgrade() {
        mail.prompt_receive_all().await.map_err(io::Error::other)?;
    }
    Ok(())
}

async fn finish_reaction(
    completed: notice_dispatch::ReactionCompletion,
    generation: u64,
    reactions: &mut notice_dispatch::NoticeReactionQueue,
    receipts: &mut NoticeReceipts,
    requests: &SessionRequests<BoardConsumerProtocol>,
    _projects: &Arc<ProjectWriters>,
) -> io::Result<()> {
    let notice_dispatch::ReactionCompletion {
        generation: completed_generation,
        owner,
        outcome,
    } = completed;
    reactions.restore(owner)?;
    if completed_generation == generation {
        match outcome {
            notice_dispatch::ReactionOutcome::CatchUp(Err(error)) => {
                tracing::warn!(%error, "machine notice catch-up unavailable")
            }
            notice_dispatch::ReactionOutcome::Notice { position, outcome } => {
                if let Err(error) = &outcome {
                    tracing::warn!(%error, ?position, "notice remains unacknowledged");
                }
                receipts.complete(position, outcome.is_ok());
                acknowledge_handled_prefix(requests, receipts).await;
                #[cfg(test)]
                {
                    use super::project_writers::NoticeHandling;
                    let result = if outcome.is_ok() {
                        NoticeHandling::Handled
                    } else {
                        NoticeHandling::Failed
                    };
                    let _ = _projects.notice_completions.send((position, result));
                }
            }
            notice_dispatch::ReactionOutcome::CatchUp(Ok(())) => {}
        }
    }
    Ok(())
}
