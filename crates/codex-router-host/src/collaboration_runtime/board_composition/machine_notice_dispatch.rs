//! One movable reaction owner serializes its durable cursor outside the transport loop.
use super::{MachineNoticeReactions, MachineSessionEventOwners, SessionCompletion};
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{Notice, NoticePosition};
use fleet_sync_tokio_runner::SessionRequests;
use std::{collections::VecDeque, io};
use tokio::task::JoinSet;

enum ReactionInput {
    CatchUp,
    Notice {
        position: NoticePosition,
        body: Notice<BoardConsumerProtocol>,
    },
}

struct QueuedReaction {
    generation: u64,
    input: ReactionInput,
}

enum ReactionOwner {
    Available(Box<MachineNoticeReactions>),
    InFlight,
}

pub(super) enum ReactionOutcome {
    CatchUp(io::Result<()>),
    Notice {
        position: NoticePosition,
        outcome: io::Result<()>,
    },
}

pub(super) struct ReactionCompletion {
    pub generation: u64,
    pub owner: Box<MachineNoticeReactions>,
    pub outcome: ReactionOutcome,
}

pub(super) struct NoticeReactionQueue {
    owner: ReactionOwner,
    queued: VecDeque<QueuedReaction>,
}

impl NoticeReactionQueue {
    pub fn new(owner: MachineNoticeReactions) -> Self {
        Self {
            owner: ReactionOwner::Available(Box::new(owner)),
            queued: VecDeque::new(),
        }
    }

    pub fn catch_up(&mut self, generation: u64) {
        self.queued.push_back(QueuedReaction {
            generation,
            input: ReactionInput::CatchUp,
        });
    }

    pub fn notice(
        &mut self,
        generation: u64,
        position: NoticePosition,
        body: Notice<BoardConsumerProtocol>,
    ) {
        self.queued.push_back(QueuedReaction {
            generation,
            input: ReactionInput::Notice { position, body },
        });
    }

    pub fn clear_queued(&mut self) {
        self.queued.clear();
    }

    pub fn restore(&mut self, owner: Box<MachineNoticeReactions>) -> io::Result<()> {
        if !matches!(self.owner, ReactionOwner::InFlight) {
            return Err(io::Error::other("notice reaction owner returned twice"));
        }
        self.owner = ReactionOwner::Available(owner);
        Ok(())
    }

    pub fn dispatch_next(
        &mut self,
        handlers: &mut JoinSet<SessionCompletion>,
        owners: &MachineSessionEventOwners,
        requests: &SessionRequests<BoardConsumerProtocol>,
    ) {
        if self.queued.is_empty() || matches!(self.owner, ReactionOwner::InFlight) {
            return;
        }
        let ReactionOwner::Available(mut owner) =
            std::mem::replace(&mut self.owner, ReactionOwner::InFlight)
        else {
            return;
        };
        let Some(queued) = self.queued.pop_front() else {
            self.owner = ReactionOwner::Available(owner);
            return;
        };
        let projects = owners.projects.clone();
        let admission = owners.admission.clone();
        let fleet = owners.fleet.clone();
        let mail = owners.mail.clone();
        let requests = requests.clone();
        handlers.spawn(async move {
            let outcome = match queued.input {
                ReactionInput::CatchUp => ReactionOutcome::CatchUp(
                    owner
                        .catch_up_notices(&projects, &admission, &fleet, &mail, &requests)
                        .await,
                ),
                ReactionInput::Notice { position, body } => {
                    let outcome = handle_notice(
                        &mut owner, &projects, &admission, &fleet, &mail, &requests, body,
                    )
                    .await;
                    ReactionOutcome::Notice { position, outcome }
                }
            };
            SessionCompletion::Reaction(ReactionCompletion {
                generation: queued.generation,
                owner,
                outcome,
            })
        });
    }
}

pub(super) async fn handle_notice(
    owner: &mut MachineNoticeReactions,
    projects: &std::sync::Arc<super::ProjectWriters>,
    admission: &std::sync::Weak<collaboration_service::ProjectAdmission>,
    fleet: &std::sync::Weak<collaboration_runtime::FleetClient>,
    mail: &std::sync::Weak<collaboration_service::MailDeliveryRuntime>,
    requests: &SessionRequests<BoardConsumerProtocol>,
    body: Notice<BoardConsumerProtocol>,
) -> io::Result<()> {
    match body {
        Notice::LeaseChanged { database } => {
            let project = message_board::ProjectId::try_from(database.to_string())
                .map_err(io::Error::other)?;
            projects.prompt_project_renewal(&project).await
        }
        Notice::Consumer(notice) => {
            owner
                .handle_pushed_notice(notice, projects, admission, fleet, mail, requests)
                .await
        }
    }
}
