//! What tells a reader that the target of its held mail may have loaded, so it checks
//! presence again then and never on a timer.
//!
//! A Codex thread that loads is recorded in the lifecycle journal as a thread status
//! leaving "not loaded"; the reader follows the journal from the position it recorded
//! before its last presence check and is owed a check only for its own thread. An ACP
//! session loads through the provider supervisor, whose completions are a signal. While
//! lifecycle observation is unavailable, each observation retry is a signal.
use collaboration_protocol::{
    JournalPosition, LifecycleChange, LifecycleSubject, NativeThreadStatus, SessionRef,
};
use std::sync::Arc;
use tokio::sync::watch;

/// The signals a Host hands its readers; each is absent where the Host has none.
#[derive(Clone, Default)]
pub struct HeldMailSignals {
    /// The lifecycle journal, whose commits carry Codex thread loads.
    pub journal: Option<Arc<lifecycle_observation::LifecycleStore>>,
    /// Bumped when the provider supervisor publishes an operation's result, a load among
    /// them.
    pub provider_completions: Option<watch::Receiver<u64>>,
    /// Bumped on each lifecycle observation retry while observation is unavailable.
    pub observation_rechecks: Option<watch::Receiver<u64>>,
}

/// One reader's view of the signals.
pub(super) struct HeldMailWatch {
    journal: Option<(
        Arc<lifecycle_observation::LifecycleStore>,
        watch::Receiver<u64>,
    )>,
    provider_completions: Option<watch::Receiver<u64>>,
    observation_rechecks: Option<watch::Receiver<u64>>,
    /// The journal position recorded before the reader's last presence check.
    position: Option<JournalPosition>,
    /// The signal that woke the last wait. Waiting for a change marks it seen, so the
    /// wake itself is the record that it happened.
    woken_by: Option<Signal>,
}

/// Which signal woke a wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Signal {
    Journal,
    ProviderCompletion,
    ObservationRecheck,
}

/// The largest page a journal read may ask for.
const JOURNAL_PAGE: u32 = 100;

impl HeldMailWatch {
    pub(super) fn new(signals: &HeldMailSignals) -> Self {
        Self {
            journal: signals.journal.as_ref().map(|journal| {
                let mut commits = journal.subscribe_commits();
                commits.borrow_and_update();
                (Arc::clone(journal), commits)
            }),
            provider_completions: signals.provider_completions.clone().map(|mut receiver| {
                receiver.borrow_and_update();
                receiver
            }),
            observation_rechecks: signals.observation_rechecks.clone().map(|mut receiver| {
                receiver.borrow_and_update();
                receiver
            }),
            position: None,
            woken_by: None,
        }
    }

    /// Records where the journal is, just before a presence check reads: a load the
    /// journal records after this point, even during the check, owes another check.
    pub(super) async fn record_position(&mut self) {
        let Some((journal, _)) = &self.journal else {
            return;
        };
        self.position = journal.bounds().await.ok().map(|bounds| JournalPosition {
            journal_id: bounds.journal_id,
            sequence: bounds.last_sequence,
        });
    }

    /// Resolves at the next signal and records which one; never when the reader has no
    /// signal at all.
    pub(super) async fn changed(&mut self) {
        let Self {
            journal,
            provider_completions,
            observation_rechecks,
            woken_by,
            ..
        } = self;
        let journal = async {
            match journal {
                Some((_, commits)) => {
                    if commits.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                }
                None => std::future::pending().await,
            }
        };
        let provider = async {
            match provider_completions {
                Some(receiver) => {
                    if receiver.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                }
                None => std::future::pending().await,
            }
        };
        let rechecks = async {
            match observation_rechecks {
                Some(receiver) => {
                    if receiver.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                }
                None => std::future::pending().await,
            }
        };
        *woken_by = Some(tokio::select! {
            () = journal => Signal::Journal,
            () = provider => Signal::ProviderCompletion,
            () = rechecks => Signal::ObservationRecheck,
        });
    }

    /// Takes every signal since the last call and reports whether the reader owes its
    /// held mail for `target` another presence check.
    pub(super) async fn take_owed_check(&mut self, target: &SessionRef) -> bool {
        // The signal that woke the wait is already marked seen; any other is checked and
        // marked seen in one call, so no signal lands between the two.
        let woken_by = self.woken_by.take();
        let mut owed = woken_by == Some(Signal::ProviderCompletion)
            || self
                .provider_completions
                .as_mut()
                .is_some_and(|receiver| receiver.borrow_and_update().has_changed());
        owed |= woken_by == Some(Signal::ObservationRecheck)
            || self
                .observation_rechecks
                .as_mut()
                .is_some_and(|receiver| receiver.borrow_and_update().has_changed());
        let journal_changed = woken_by == Some(Signal::Journal)
            || self
                .journal
                .as_mut()
                .is_some_and(|(_, commits)| commits.borrow_and_update().has_changed());
        if journal_changed {
            owed |= self.target_loaded_since_position(target).await;
        }
        owed
    }

    /// Reads the journal after the recorded position for a status of `target`'s thread
    /// that left "not loaded", and moves the position past what it read.
    async fn target_loaded_since_position(&mut self, target: &SessionRef) -> bool {
        let (Some((journal, _)), Some(position)) = (&self.journal, self.position.clone()) else {
            return false;
        };
        let mut after = position;
        let mut loaded = false;
        loop {
            let Ok(page) = journal
                .read_wait(&target.endpoint, after.clone(), JOURNAL_PAGE, 0)
                .await
            else {
                // An unreadable journal is unavailable observation: its retry is a signal.
                return loaded;
            };
            loaded |= page.records.iter().any(|record| {
                matches!(
                    (&record.observation.subject, &record.observation.change),
                    (
                        LifecycleSubject::Thread { address },
                        LifecycleChange::ThreadStatus { status, .. },
                    ) if address.native_thread_id == target.session_id
                        && *status != NativeThreadStatus::NotLoaded
                )
            });
            after = page.next.clone();
            if page.caught_up {
                break;
            }
        }
        self.position = Some(after);
        loaded
    }
}
