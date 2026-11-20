//! Run one pure provider session owner and execute its committed outbox.

use provider_session_machines::session_owner::{
    OwnerAddress, OwnerDeadline, OwnerEffect, OwnerEvent, OwnerMailboxEvent, SessionOwnerCommand,
    SessionOwnerRejection, SessionOwnerState, SessionSnapshot,
};
use router_clock::{ClockInstant, RuntimeClock};
use std::{
    collections::{BTreeMap, VecDeque},
    num::NonZeroUsize,
    sync::Arc,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OwnerMailboxUnavailable {
    #[error("session busy; send again")]
    Busy,
    #[error("the session owner stopped")]
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OwnerTaskError {
    #[error("the session owner has too many in-flight effects")]
    CompletionCapacity,
    #[error("an executor needed by the session owner stopped")]
    ExecutorStopped,
}

struct Submission {
    address: OwnerAddress,
    command: SessionOwnerCommand,
    accepted: oneshot::Sender<Result<(), SessionOwnerRejection>>,
}

#[derive(Clone)]
pub struct SessionOwnerHandle {
    commands: mpsc::Sender<Submission>,
    completions: OwnerCompletionPort,
    snapshots: watch::Receiver<SessionSnapshot>,
}

impl SessionOwnerHandle {
    pub fn try_submit(
        &self,
        address: OwnerAddress,
        command: SessionOwnerCommand,
    ) -> Result<oneshot::Receiver<Result<(), SessionOwnerRejection>>, OwnerMailboxUnavailable> {
        let (accepted, decision) = oneshot::channel();
        self.commands
            .try_send(Submission {
                address,
                command,
                accepted,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => OwnerMailboxUnavailable::Busy,
                mpsc::error::TrySendError::Closed(_) => OwnerMailboxUnavailable::Stopped,
            })?;
        Ok(decision)
    }

    #[must_use]
    pub fn snapshots(&self) -> watch::Receiver<SessionSnapshot> {
        self.snapshots.clone()
    }

    #[must_use]
    pub fn completion_port(&self) -> OwnerCompletionPort {
        self.completions.clone()
    }
}

#[derive(Clone)]
pub struct OwnerCompletionPort(mpsc::Sender<OwnerMailboxEvent>);

pub struct OwnerCompletion {
    address: OwnerAddress,
    permit: mpsc::OwnedPermit<OwnerMailboxEvent>,
}

impl OwnerCompletionPort {
    /// Reserve before offering external work. Completion then needs neither
    /// spare mailbox space nor an await, even if the business mailbox is full.
    pub fn reserve(&self, address: OwnerAddress) -> Result<OwnerCompletion, OwnerTaskError> {
        let permit = self
            .0
            .clone()
            .try_reserve_owned()
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => OwnerTaskError::CompletionCapacity,
                mpsc::error::TrySendError::Closed(_) => OwnerTaskError::ExecutorStopped,
            })?;
        Ok(OwnerCompletion { address, permit })
    }
}

impl OwnerCompletion {
    pub fn send(self, event: OwnerEvent) {
        self.permit.send(OwnerMailboxEvent {
            owner: self.address,
            event,
        });
    }
}

/// Effect offers are synchronous. An executor reserves completions before
/// accepting work it will carry out separately. Synchronous results such as
/// revoke are returned directly and reduced before reading another command.
pub trait SessionOwnerPorts {
    fn execute(
        &mut self,
        address: OwnerAddress,
        effect: OwnerEffect,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError>;
}

struct ArmedDeadline {
    address: OwnerAddress,
    deadline: OwnerDeadline,
}

pub struct SessionOwnerTask {
    state: SessionOwnerState,
    commands: mpsc::Receiver<Submission>,
    completion_port: OwnerCompletionPort,
    completions: mpsc::Receiver<OwnerMailboxEvent>,
    snapshots: watch::Sender<SessionSnapshot>,
    deadlines: BTreeMap<ClockInstant, VecDeque<ArmedDeadline>>,
    clock: Arc<dyn RuntimeClock>,
}

impl SessionOwnerTask {
    pub fn new(
        address: OwnerAddress,
        clock: Arc<dyn RuntimeClock>,
        mailbox_capacity: NonZeroUsize,
        completion_capacity: NonZeroUsize,
    ) -> (Self, SessionOwnerHandle) {
        let state = SessionOwnerState::new(address);
        let (commands, command_rx) = mpsc::channel(mailbox_capacity.get());
        let (completion_tx, completions) = mpsc::channel(completion_capacity.get());
        let completion_port = OwnerCompletionPort(completion_tx);
        let (snapshots, snapshot_rx) = watch::channel(state.snapshot());
        let task = Self {
            state,
            commands: command_rx,
            completion_port: completion_port.clone(),
            completions,
            snapshots,
            deadlines: BTreeMap::new(),
            clock,
        };
        let handle = SessionOwnerHandle {
            commands,
            completions: completion_port,
            snapshots: snapshot_rx,
        };
        (task, handle)
    }

    pub async fn run(
        mut self,
        mut ports: impl SessionOwnerPorts,
        shutdown: CancellationToken,
    ) -> Result<(), OwnerTaskError> {
        let mut accepting_commands = true;
        loop {
            let next_due = self.deadlines.first_key_value().map(|(time, _)| *time);
            let clock = Arc::clone(&self.clock);
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return Ok(()),
                () = wait_for_deadline(clock.as_ref(), next_due) => {
                    if let Some(alarm) = self.pop_deadline() {
                        self.process(OwnerMailboxEvent {
                            owner: alarm.address,
                            event: OwnerEvent::Deadline(alarm.deadline),
                        }, None, &mut ports)?;
                    }
                }
                Some(completion) = self.completions.recv() => {
                    self.process(completion, None, &mut ports)?;
                }
                submission = self.commands.recv(), if accepting_commands => {
                    if let Some(Submission { address, command, accepted }) = submission {
                        self.process(OwnerMailboxEvent {
                            owner: address,
                            event: OwnerEvent::Command(command),
                        }, Some(accepted), &mut ports)?;
                    } else {
                        accepting_commands = false;
                    }
                }
            }
        }
    }

    fn process(
        &mut self,
        event: OwnerMailboxEvent,
        accepted: Option<oneshot::Sender<Result<(), SessionOwnerRejection>>>,
        ports: &mut impl SessionOwnerPorts,
    ) -> Result<(), OwnerTaskError> {
        let mut ready = VecDeque::from([(event, accepted)]);
        while let Some((event, accepted)) = ready.pop_front() {
            let commit = match self.state.decide(event) {
                Ok(commit) => commit,
                Err(reason) => {
                    if let Some(accepted) = accepted {
                        let _reader_gone = accepted.send(Err(reason));
                    }
                    continue;
                }
            };
            let committed_at = self.clock.monotonic_now();
            self.state = commit.next;
            let address = commit.effects.owner;
            for effect in commit.effects.effects {
                if let OwnerEffect::Arm { deadline, budget } = effect {
                    self.deadlines
                        .entry(committed_at.after(budget))
                        .or_default()
                        .push_back(ArmedDeadline { address, deadline });
                } else if let Some(event) = ports.execute(address, effect, &self.completion_port)? {
                    ready.push_back((
                        OwnerMailboxEvent {
                            owner: address,
                            event,
                        },
                        None,
                    ));
                }
            }
            self.deadlines.retain(|_, alarms| {
                alarms.retain(|alarm| self.state.has_pending_deadline(&alarm.deadline));
                !alarms.is_empty()
            });
            let snapshot = self.state.snapshot();
            self.snapshots.send_if_modified(|published| {
                if *published == snapshot {
                    return false;
                }
                *published = snapshot;
                true
            });
            if let Some(accepted) = accepted {
                let _reader_gone = accepted.send(Ok(()));
            }
        }
        Ok(())
    }

    fn pop_deadline(&mut self) -> Option<ArmedDeadline> {
        let mut first = self.deadlines.first_entry()?;
        let alarm = first.get_mut().pop_front();
        if first.get().is_empty() {
            first.remove();
        }
        alarm
    }
}

async fn wait_for_deadline(clock: &dyn RuntimeClock, at: Option<ClockInstant>) {
    if let Some(at) = at {
        clock.sleep_until(at).await;
    } else {
        std::future::pending().await
    }
}

#[cfg(test)]
#[path = "session_owner_task_test.rs"]
mod tests;
