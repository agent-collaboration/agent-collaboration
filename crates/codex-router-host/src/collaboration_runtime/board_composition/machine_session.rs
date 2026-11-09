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
use fleet_protocol::{DatabaseStatus, Notice, NoticePosition, PeerId, SchemaSupport, SessionId};
use fleet_session::{ClientEvent, SessionClient};
use fleet_session_client::{SessionPump, SessionRecoveryPolicy, SessionRequests};
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

type BoardSessionPump = SessionPump<
    crate::enrollment_session_transport::EnrollmentSessionTransport,
    MachineSessionStore,
    BoardConsumerProtocol,
>;

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
    link_failures: board_fleet_sync::FleetLinkPublisher,
}

pub(crate) struct RunningMachineSession {
    stop: watch::Sender<bool>,
    task: JoinSet<io::Result<()>>,
    #[cfg(any(test, feature = "runtime-test-support"))]
    pub(super) projects: Weak<ProjectWriters>,
}

impl PreparedMachineSession {
    pub async fn prepare(
        directory: &Path,
        inputs: BoardSessionInputs,
        registry: &Arc<ProjectStoreRegistry>,
        enrollment_directory: &Path,
        link: &super::machine_link::PreparedMachineLink,
    ) -> io::Result<Self> {
        let cache = link.cache.clone();
        cache
            .import_legacy_runtime(directory)
            .await
            .map_err(io::Error::other)?;
        let store = MachineSessionStore::from_cache(cache.clone());
        let link_failures = link.publisher.clone();
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
        let transport = crate::enrollment_session_transport::EnrollmentSessionTransport::new(
            transport,
            enrollment_directory,
        );
        let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
            transport,
            store,
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
        let (requests, events, pump) =
            create_machine_session_pump(client, inputs.endpoint, inputs.credential);
        let (welcomed, readiness) = watch::channel(false);
        let (stop, stopped) = watch::channel(false);
        let central_reads = Arc::new(super::central_read_copy::CentralReadCopies::new(
            directory,
            requests.clone(),
            inputs.receiver,
            readiness.clone(),
        ));
        central_reads.sweep_stale_session_roots().await;
        registry
            .attach_central_reads(central_reads)
            .map_err(io::Error::other)?;
        let projects = Arc::new(ProjectWriters::new(
            super::project_writers::ProjectWriterInputs {
                cache,
                directory,
                peer: inputs.peer,
                registry,
                requests: requests.clone(),
                receiver: inputs.receiver,
                welcomed: readiness,
                stopped,
            },
        ));
        projects.prepare_existing().await?;
        let notices = MachineNoticeReactions::open(directory).await?;
        Ok(Self {
            link_failures,
            pump,
            events,
            requests,
            peer: inputs.peer,
            relay: inputs.relay,
            notices,
            projects,
            welcomed,
            stop,
        })
    }

    /// Activation opens the socket after Host assembly; preparation observes failures independently.
    pub fn start(
        self,
        admission: Weak<ProjectAdmission>,
        fleet: Weak<FleetClient>,
        mail: Weak<MailDeliveryRuntime>,
        publisher: Weak<NotificationPublisher>,
        link_tasks: JoinSet<io::Result<()>>,
    ) -> RunningMachineSession {
        let stop = self.stop.clone();
        #[cfg(any(test, feature = "runtime-test-support"))]
        let projects = Arc::downgrade(&self.projects);
        let mut task = JoinSet::new();
        task.spawn(self.run(admission, fleet, mail, publisher, link_tasks));
        RunningMachineSession {
            stop,
            task,
            #[cfg(any(test, feature = "runtime-test-support"))]
            projects,
        }
    }

    async fn run(
        self,
        admission: Weak<ProjectAdmission>,
        fleet: Weak<FleetClient>,
        mail: Weak<MailDeliveryRuntime>,
        publisher: Weak<NotificationPublisher>,
        mut tasks: JoinSet<io::Result<()>>,
    ) -> io::Result<()> {
        let Self {
            pump,
            events,
            requests,
            notices,
            projects,
            welcomed,
            stop,
            link_failures,
            ..
        } = self;
        let stopped = stop.subscribe();
        let link_state = pump.link_state();
        let link_events = pump.observe_link();
        link_failures.attach_session(link_state);
        let log_stop = stop.subscribe();
        tasks.spawn(async move {
            board_fleet_sync::FleetLinkPublisher::observe_events(link_events).await;
            await_machine_stop(log_stop).await
        });
        let failures = link_failures.clone();
        let stopping = stop.clone();
        tasks.spawn(async move {
            let result = pump.run(stopped).await;
            let outcome = match result {
                Ok(()) if *stopping.borrow() => Ok(()),
                Ok(()) => Err(io::Error::other("machine session stopped unexpectedly")),
                Err(error) if expected_terminal(&error) => Err(io::Error::other(error)),
                Err(error) => {
                    failures.session_stopped(&error);
                    Err(io::Error::other(error))
                }
            };
            stopping.send_replace(true);
            outcome
        });
        let stopped = stop.subscribe();
        let events_stop = stopped.clone();
        let handling = projects.clone();
        tasks.spawn(async move {
            let result = run_events(
                events,
                requests,
                notices,
                MachineSessionEventOwners {
                    projects: handling,
                    admission,
                    fleet,
                    mail,
                    publisher,
                    link_failures: Some(link_failures),
                },
                welcomed,
                stopped,
            )
            .await;
            if result.is_ok() {
                await_machine_stop(events_stop).await?;
            }
            result
        });
        let first = tasks.join_next().await;
        let requested = *stop.borrow();
        stop.send_replace(true);
        let mut failure = match first {
            Some(Ok(Err(error))) => Some(error),
            Some(Err(error)) => Some(io::Error::other(error)),
            _ if !requested => Some(io::Error::other("machine session stopped unexpectedly")),
            _ => None,
        };
        drain_session_tasks(&mut tasks, &mut failure).await;
        if let Err(error) = projects.shutdown().await {
            retain_session_failure(&mut failure, error);
        }
        failure.map_or(Ok(()), Err)
    }
}

impl RunningMachineSession {
    #[cfg(test)]
    pub(in crate::collaboration_runtime) async fn wait_for_stop_for_test(&self) -> io::Result<()> {
        await_machine_stop(self.stop.subscribe()).await
    }

    pub(in crate::collaboration_runtime) fn is_expected_refusal_error(error: &io::Error) -> bool {
        expected_terminal_io(error)
    }

    #[cfg(test)]
    pub(in crate::collaboration_runtime) fn failed_for_test(error: io::Error) -> Self {
        let (stop, _stopped) = watch::channel(false);
        let mut task = JoinSet::new();
        task.spawn(async move { Err(error) });
        Self {
            stop,
            task,
            projects: Weak::new(),
        }
    }

    pub(in crate::collaboration_runtime) fn is_observed_refusal(
        error: &io::Error,
        record: &board_sync_protocol::FleetLinkRecord,
    ) -> bool {
        use board_sync_protocol::{FleetLinkFailureReason, FleetLinkRefusal, FleetLinkState};
        use fleet_protocol::AuthRefusal;
        use fleet_session::{ClientError, ConnectFailure};
        let Some(native) = error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ClientError>())
        else {
            return false;
        };
        let FleetLinkState::Refused { reason, .. } = &record.state else {
            return false;
        };
        match (native, reason) {
            (
                ClientError::Revoked,
                FleetLinkRefusal::CredentialRejected {
                    reason: AuthRefusal::Revoked,
                },
            )
            | (ClientError::ReplacedByNewerSession, FleetLinkRefusal::IdentityInUseElsewhere)
            | (ClientError::ConnectionReplaced, FleetLinkRefusal::IdentityInUseElsewhere)
            | (
                ClientError::Connect(ConnectFailure::InvalidEndpoint),
                FleetLinkRefusal::SessionFailure {
                    reason: FleetLinkFailureReason::InvalidEndpoint,
                },
            )
            | (
                ClientError::Connect(ConnectFailure::InvalidCredential),
                FleetLinkRefusal::SessionFailure {
                    reason: FleetLinkFailureReason::InvalidCredential,
                },
            ) => true,
            (
                ClientError::CredentialRejected { reason: expected },
                FleetLinkRefusal::CredentialRejected { reason: observed },
            ) => expected == observed,
            _ => false,
        }
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

async fn await_machine_stop(mut stopped: watch::Receiver<bool>) -> io::Result<()> {
    while !*stopped.borrow() {
        stopped.changed().await.map_err(io::Error::other)?;
    }
    Ok(())
}

fn expected_terminal(error: &fleet_session::ClientError) -> bool {
    use fleet_session::{ClientError, ConnectFailure};
    matches!(
        error,
        ClientError::Revoked
            | ClientError::CredentialRejected { .. }
            | ClientError::ReplacedByNewerSession
            | ClientError::ConnectionReplaced
            | ClientError::Connect(
                ConnectFailure::InvalidEndpoint | ConnectFailure::InvalidCredential
            )
    )
}

fn expected_terminal_io(error: &io::Error) -> bool {
    error
        .get_ref()
        .and_then(|error| error.downcast_ref::<fleet_session::ClientError>())
        .is_some_and(expected_terminal)
}

fn retain_session_failure(failure: &mut Option<io::Error>, next: io::Error) {
    if failure
        .as_ref()
        .is_none_or(|current| expected_terminal_io(current) && !expected_terminal_io(&next))
    {
        *failure = Some(next);
    }
}

async fn drain_session_tasks(tasks: &mut JoinSet<io::Result<()>>, failure: &mut Option<io::Error>) {
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => retain_session_failure(failure, error),
            Err(error) => retain_session_failure(failure, io::Error::other(error)),
        }
    }
}

type MachineSessionPumpParts<T, S> = (
    SessionRequests<BoardConsumerProtocol>,
    mpsc::Receiver<ClientEvent<BoardConsumerProtocol>>,
    SessionPump<T, S, BoardConsumerProtocol>,
);

// Keep the Host's selected policy in the same constructor used by its recovery
// tests. Tests replace external ports while keeping this policy and pump real.
fn create_machine_session_pump<
    T: fleet_session::SessionTransport,
    S: fleet_session::SessionClientStore,
>(
    client: SessionClient<T, S, BoardConsumerProtocol>,
    endpoint: fleet_session::Endpoint,
    credential: fleet_session::PeerCredential,
) -> MachineSessionPumpParts<T, S> {
    let (requests, events, pump) =
        SessionPump::new(client, endpoint, credential, SESSION_CALL_TIMEOUT);
    (
        requests,
        events,
        pump.with_recovery_policy(SessionRecoveryPolicy {
            connection_replaced: fleet_session_client::ConnectionReplacedRecovery::Terminal,
            welcome_timeout: Some(Duration::from_secs(30)),
            ..SessionRecoveryPolicy::default()
        }),
    )
}

#[cfg(test)]
#[path = "machine_session_recovery_test.rs"]
mod recovery_tests;

enum SessionCompletion {
    Operation(io::Result<()>),
    Reaction(notice_dispatch::ReactionCompletion),
}

struct MachineSessionEventOwners {
    link_failures: Option<board_fleet_sync::FleetLinkPublisher>,
    projects: Arc<ProjectWriters>,
    admission: Weak<ProjectAdmission>,
    fleet: Weak<FleetClient>,
    mail: Weak<MailDeliveryRuntime>,
    publisher: Weak<NotificationPublisher>,
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
            result = handlers.join_next(), if !handlers.is_empty() => match result {
                Some(Ok(SessionCompletion::Reaction(completed))) => {
                    finish_reaction(completed, generation, &mut reactions, &mut receipts, &requests, &owners.projects, owners.link_failures.as_ref()).await?;
                }
                Some(Ok(SessionCompletion::Operation(Err(error)))) => {
                    if let Some(error) = error.get_ref().and_then(|error| error.downcast_ref::<fleet_session::ClientError>())
                        && let Some(failures) = &owners.link_failures { failures.session_failed(error); }
                    tracing::warn!(%error, "machine-session operation unavailable");
                },
                Some(Err(error)) => tracing::warn!(%error, "machine-session operation task failed"),
                _ => {},
            },
            event = events.recv() => match event {
                Some(ClientEvent::Welcome { databases }) => {
                    generation = generation.checked_add(1).ok_or_else(|| io::Error::other("notice generation exhausted"))?;
                    receipts = NoticeReceipts::default();
                    welcomed.send_replace(true);
                    spawn_installation_directory_refresh(&mut handlers, &owners.mail);
                    status_interval.reset();
                    spawn_welcome_recovery(&mut handlers, &owners, databases);
                    reactions.catch_up(generation);
                    for (session_id, position, notice) in deferred_notices.drain(..) {
                        queue_notice(&mut reactions, &mut receipts, generation, session_id, position, notice);
                    }
                }
                Some(ClientEvent::Request { frame, body }) => {
                    spawn_operation(&mut handlers, answer_server_call(owners.projects.clone(), owners.admission.clone(), owners.mail.clone(), owners.publisher.clone(), requests.clone(), frame, body));
                }
                Some(ClientEvent::Notice { session_id, position, body }) => {
                    if *welcomed.borrow() {
                        queue_notice(&mut reactions, &mut receipts, generation, session_id, position, body);
                    } else {
                        deferred_notices.push((session_id, position, body));
                    }
                }
                Some(ClientEvent::Disconnected { reason, .. }) => {
                    if matches!(reason, Some(fleet_protocol::CloseReason::MalformedFrame))
                        && let Some(failures) = &owners.link_failures {
                        failures.session_failed(&fleet_session::ClientError::Protocol);
                    }
                    welcomed.send_replace(false);
                    generation = generation.checked_add(1).ok_or_else(|| io::Error::other("notice generation exhausted"))?;
                    deferred_notices.clear();
                    reactions.clear_queued();
                    receipts = NoticeReceipts::default();
                }
                Some(ClientEvent::Unknown { .. } | ClientEvent::Answer { .. }) => {}
                Some(ClientEvent::RequestBatch { .. } | ClientEvent::AnswerBatch { .. }) => {
                    // D3 admits the wire vocabulary; D5 owns batch execution.
                }
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
        let projects = owners.projects.clone();
        let mail = owners.mail.clone();
        spawn_operation(handlers, async move {
            projects.observe_completed_install(&project).await?;
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
    session_id: SessionId,
    position: NoticePosition,
    notice: Notice<BoardConsumerProtocol>,
) {
    if receipts.begin(position) {
        reactions.notice(generation, session_id, position, notice);
    }
}

// This transport owner alone sends ACKs. The separate consumer progress record
// records machine-read positions, which may also advance through catch-up.
async fn acknowledge_handled_prefix(
    requests: &SessionRequests<BoardConsumerProtocol>,
    receipts: &mut NoticeReceipts,
    session_id: SessionId,
    failures: Option<&board_fleet_sync::FleetLinkPublisher>,
) {
    if let Some(position) = receipts.handled_prefix() {
        match requests.acknowledge_notice(session_id, position).await {
            Ok(()) => receipts.acknowledged(position),
            Err(error) => {
                if let Some(failures) = failures {
                    failures.session_failed(&error);
                }
                receipts.acknowledgement_failed();
                tracing::warn!(%error, ?position, "notice acknowledgement unavailable");
            }
        }
    }
}

async fn finish_reaction(
    completed: notice_dispatch::ReactionCompletion,
    generation: u64,
    reactions: &mut notice_dispatch::NoticeReactionQueue,
    receipts: &mut NoticeReceipts,
    requests: &SessionRequests<BoardConsumerProtocol>,
    _projects: &Arc<ProjectWriters>,
    failures: Option<&board_fleet_sync::FleetLinkPublisher>,
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
            notice_dispatch::ReactionOutcome::Notice {
                session_id,
                position,
                outcome,
            } => {
                if let Err(error) = &outcome {
                    tracing::warn!(%error, ?position, "notice remains unacknowledged");
                }
                receipts.complete(position, outcome.is_ok());
                acknowledge_handled_prefix(requests, receipts, session_id, failures).await;
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
