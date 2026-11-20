//! One bounded publication inbox for each hub session.

use super::{HubEvent, ProviderSessionEventHub, SessionHistory};
use message_board::SessionRef;
use session_event_model::{
    CapabilityReport, SessionEvent, SessionSettings, SessionState, SkillView,
};
use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    sync::{Arc, Mutex, MutexGuard, TryLockError},
};
use tokio::sync::{Notify, oneshot, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubPublicationStamp {
    pub incarnation: Uuid,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubHistoryAvailability {
    Available,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubPublicationFact {
    Event(SessionEvent),
    PreviousTurnEnded {
        turn_id: String,
        outcome: session_event_model::TurnOutcome,
    },
    Skills(SkillView),
    ReplayBoundary,
    HistoryUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubPublicationBaseline {
    pub state: SessionState,
    pub turn_running: bool,
    pub capabilities: Option<CapabilityReport>,
    pub settings: Option<SessionSettings>,
    pub skills: SkillView,
    pub history: HubHistoryAvailability,
}

#[derive(Debug)]
pub enum HubPublication {
    Commit {
        stamp: HubPublicationStamp,
        facts: Vec<HubPublicationFact>,
        observed_at_seconds: i64,
    },
    Rebase {
        stamp: HubPublicationStamp,
        baseline: Box<HubPublicationBaseline>,
    },
}

impl HubPublication {
    fn stamp(&self) -> HubPublicationStamp {
        match self {
            Self::Commit { stamp, .. } | Self::Rebase { stamp, .. } => *stamp,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HubPublicationRefusal {
    #[error("the session's hub inbox is full")]
    Full,
    #[error("the session's hub inbox is applying a batch")]
    Applying,
    #[error("the session's hub inbox is closed")]
    Closed,
    #[error("a different session owner incarnation has the inbox")]
    Superseded,
    #[error("publication must resume from a rebase or the next sequence")]
    OutOfSequence,
    #[error("the hub could not apply the session publication")]
    Projection,
}

pub type HubPublicationReceipt =
    oneshot::Receiver<Result<HubPublicationStamp, HubPublicationRefusal>>;

#[derive(Clone)]
pub struct HubSessionInbox {
    inner: Arc<InboxInner>,
}

struct InboxInner {
    state: Mutex<InboxState>,
    ready: Notify,
    capacity: usize,
    capacity_returned: watch::Sender<()>,
}

#[derive(Default)]
struct InboxState {
    incarnation: Option<Uuid>,
    last_applied: Option<u64>,
    displaced_projection: bool,
    pending: VecDeque<PublicationOffer>,
    closed: bool,
}

struct PublicationOffer {
    publication: HubPublication,
    reply: oneshot::Sender<Result<HubPublicationStamp, HubPublicationRefusal>>,
}

pub struct HubInboxTask {
    session: SessionRef,
    hub: Arc<ProviderSessionEventHub>,
    inbox: HubSessionInbox,
}

impl ProviderSessionEventHub {
    /// The first lookup returns the one worker for this inbox. Its lifetime is
    /// the hub's, even when the session owner that publishes to it is replaced.
    pub fn owner_inbox(
        self: &Arc<Self>,
        session: SessionRef,
        capacity: NonZeroUsize,
    ) -> (HubSessionInbox, Option<HubInboxTask>) {
        use std::collections::btree_map::Entry;

        let mut inboxes = self
            .owner_inboxes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inboxes.entry(session.clone()) {
            Entry::Occupied(entry) => (entry.get().clone(), None),
            Entry::Vacant(entry) => {
                let inbox = HubSessionInbox {
                    inner: Arc::new(InboxInner {
                        state: Mutex::default(),
                        ready: Notify::new(),
                        capacity: capacity.get(),
                        capacity_returned: watch::channel(()).0,
                    }),
                };
                entry.insert(inbox.clone());
                let worker = HubInboxTask {
                    session,
                    hub: Arc::clone(self),
                    inbox: inbox.clone(),
                };
                (inbox, Some(worker))
            }
        }
    }
}

impl HubSessionInbox {
    fn state(&self) -> MutexGuard<'_, InboxState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The registry advances the inbox only for the incarnation it replaces.
    /// Displaced offers are answered without touching the history lock.
    pub fn install_incarnation(
        &self,
        expected: Option<Uuid>,
        new: Uuid,
    ) -> Result<(), HubPublicationRefusal> {
        let mut state = self.state();
        if state.closed {
            return Err(HubPublicationRefusal::Closed);
        }
        if state.incarnation != expected {
            return Err(HubPublicationRefusal::Superseded);
        }
        if expected == Some(new) {
            return Ok(());
        }
        state.incarnation = Some(new);
        state.last_applied = None;
        state.displaced_projection = expected.is_some();
        let was_full = state.pending.len() == self.inner.capacity;
        for entry in state.pending.drain(..) {
            let _reader_gone = entry.reply.send(Err(HubPublicationRefusal::Superseded));
        }
        if was_full {
            self.inner.capacity_returned.send_replace(());
        }
        Ok(())
    }

    pub fn try_offer(
        &self,
        publication: HubPublication,
    ) -> Result<HubPublicationReceipt, HubPublicationRefusal> {
        let mut state = match self.inner.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Err(HubPublicationRefusal::Applying),
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        };
        if state.closed {
            return Err(HubPublicationRefusal::Closed);
        }
        if state.incarnation != Some(publication.stamp().incarnation) {
            return Err(HubPublicationRefusal::Superseded);
        }
        if state.pending.len() == self.inner.capacity {
            return Err(HubPublicationRefusal::Full);
        }
        let (reply, result) = oneshot::channel();
        state
            .pending
            .push_back(PublicationOffer { publication, reply });
        self.inner.ready.notify_one();
        Ok(result)
    }

    /// A publisher may use this edge to rebase ahead of its armed deadline.
    #[must_use]
    pub fn capacity_returned(&self) -> watch::Receiver<()> {
        self.inner.capacity_returned.subscribe()
    }
}

impl HubInboxTask {
    pub async fn run(self, shutdown: CancellationToken) {
        loop {
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return,
                () = self.inbox.inner.ready.notified() => {
                    let history = {
                        let mut all = self.hub.histories.lock().await;
                        all.entry(self.session.clone()).or_insert_with(|| Arc::new(
                            tokio::sync::Mutex::new(SessionHistory::new(self.hub.subscriber_capacity)),
                        )).clone()
                    };
                    let mut history = tokio::select! {
                        () = shutdown.cancelled() => return,
                        history = history.lock() => history,
                    };
                    self.apply_one(&mut history);
                }
            }
        }
    }

    fn apply_one(&self, history: &mut SessionHistory) {
        // Holding this lock while mutating history fences a rebase from a
        // replacement against an old batch already waiting for history.
        let mut state = self.inbox.state();
        let Some(entry) = state.pending.pop_front() else {
            return;
        };
        let result = apply_publication(history, &mut state, entry.publication);
        let _reader_gone = entry.reply.send(result);
        self.inbox.inner.capacity_returned.send_replace(());
        if !state.pending.is_empty() {
            self.inbox.inner.ready.notify_one();
        }
    }
}

impl Drop for HubInboxTask {
    fn drop(&mut self) {
        let mut state = self.inbox.state();
        state.closed = true;
        for entry in state.pending.drain(..) {
            let _reader_gone = entry.reply.send(Err(HubPublicationRefusal::Closed));
        }
    }
}

fn apply_publication(
    history: &mut SessionHistory,
    state: &mut InboxState,
    publication: HubPublication,
) -> Result<HubPublicationStamp, HubPublicationRefusal> {
    let stamp = publication.stamp();
    let prior = state.last_applied;
    let next = prior.and_then(|sequence| sequence.checked_add(1));
    match publication {
        HubPublication::Commit {
            facts,
            observed_at_seconds,
            ..
        } if next == Some(stamp.sequence) => {
            history.apply_owner_facts(facts, observed_at_seconds)?;
        }
        HubPublication::Rebase { baseline, .. } if stamp.sequence > prior.unwrap_or(0) => {
            history.rebase_owner_projection(*baseline, state.displaced_projection)?;
            state.displaced_projection = false;
        }
        _ => return Err(HubPublicationRefusal::OutOfSequence),
    }
    state.last_applied = Some(stamp.sequence);
    Ok(stamp)
}

impl SessionHistory {
    fn apply_owner_facts(
        &mut self,
        facts: Vec<HubPublicationFact>,
        observed_at_seconds: i64,
    ) -> Result<(), HubPublicationRefusal> {
        // Validate projection against a disposable view, then apply all facts
        // while attach and any other publisher are excluded. A rejected batch
        // cannot expose a valid prefix to existing subscribers.
        let mut projected = Self::new(1);
        projected.state = self.state.clone();
        projected.settings = self.settings.clone();
        projected.pending.clone_from(&self.pending);
        projected.turn_running = self.turn_running;
        projected.next_sequence = self.next_sequence;
        projected.replay_epoch = self.replay_epoch;
        for fact in &facts {
            projected.validate_owner_fact(fact)?;
        }
        for fact in facts {
            match fact {
                HubPublicationFact::Event(event) => self.commit_owner_event(event)?,
                HubPublicationFact::PreviousTurnEnded { turn_id, outcome } => {
                    self.retain_owner_event(SessionEvent::TurnEnded { turn_id, outcome })?;
                }
                HubPublicationFact::Skills(skills) => {
                    self.skill_view.send_replace(skills);
                }
                HubPublicationFact::ReplayBoundary | HubPublicationFact::HistoryUnavailable => {
                    self.reset_owner_history(true)?;
                }
            }
        }
        self.inventory_metadata
            .observe_activity_at(observed_at_seconds);
        Ok(())
    }

    fn validate_owner_fact(
        &mut self,
        fact: &HubPublicationFact,
    ) -> Result<(), HubPublicationRefusal> {
        match fact {
            HubPublicationFact::Event(event) => {
                self.project(event)
                    .map_err(|_| HubPublicationRefusal::Projection)?;
                self.advance_owner_sequence()?;
            }
            HubPublicationFact::PreviousTurnEnded { .. } => {
                self.advance_owner_sequence()?;
            }
            HubPublicationFact::ReplayBoundary | HubPublicationFact::HistoryUnavailable => {
                self.reset_owner_history(true)?;
            }
            HubPublicationFact::Skills(_) => {}
        }
        Ok(())
    }

    fn advance_owner_sequence(&mut self) -> Result<u64, HubPublicationRefusal> {
        let sequence = self.next_sequence;
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or(HubPublicationRefusal::Projection)?;
        Ok(sequence)
    }

    fn commit_owner_event(&mut self, event: SessionEvent) -> Result<(), HubPublicationRefusal> {
        self.project(&event)
            .map_err(|_| HubPublicationRefusal::Projection)?;
        self.retain_owner_event(event)
    }

    fn retain_owner_event(&mut self, event: SessionEvent) -> Result<(), HubPublicationRefusal> {
        let sequence = self.advance_owner_sequence()?;
        let item = HubEvent { sequence, event };
        match &item.event {
            SessionEvent::ItemStarted { item: snapshot }
            | SessionEvent::ItemUpdated { item: snapshot } => {
                self.items
                    .entry(snapshot.item_id.clone())
                    .or_default()
                    .latest = Some(item.clone());
            }
            SessionEvent::ItemCompleted { item_id } => {
                self.items.entry(item_id.clone()).or_default().completion = Some(item.clone());
            }
            _ => {
                self.events.insert(sequence, item.clone());
            }
        }
        let _no_subscribers = self.sender.send(item);
        Ok(())
    }

    fn reset_owner_history(&mut self, replace_history: bool) -> Result<(), HubPublicationRefusal> {
        let replay_epoch = self
            .replay_epoch
            .checked_add(1)
            .ok_or(HubPublicationRefusal::Projection)?;
        let _no_subscribers = self.sender.send(HubEvent {
            sequence: self.next_sequence,
            event: SessionEvent::ResyncRequired { replay_epoch },
        });
        self.sender = tokio::sync::broadcast::channel(self.subscriber_capacity).0;
        self.replay_epoch = replay_epoch;
        self.skill_view.send_replace(SkillView::Unknown);
        self.pending.clear();
        self.turn_running = false;
        self.state = SessionState::Unloaded;
        self.settings = None;
        if replace_history {
            self.events.clear();
            self.items.clear();
            self.next_sequence = 1;
        }
        Ok(())
    }

    fn rebase_owner_projection(
        &mut self,
        baseline: HubPublicationBaseline,
        displaced: bool,
    ) -> Result<(), HubPublicationRefusal> {
        let replace_history = displaced || baseline.history == HubHistoryAvailability::Unavailable;
        if !replace_history && self.next_sequence.checked_add(3).is_none() {
            return Err(HubPublicationRefusal::Projection);
        }
        self.reset_owner_history(replace_history)?;
        self.turn_running = baseline.turn_running;
        if let Some(capabilities) = baseline.capabilities {
            self.commit_owner_event(SessionEvent::CapabilitiesChanged { capabilities })?;
        }
        if let Some(settings) = baseline.settings {
            self.commit_owner_event(SessionEvent::SettingsChanged { settings })?;
        }
        self.commit_owner_event(SessionEvent::StateChanged {
            state: baseline.state,
        })?;
        self.skill_view.send_replace(baseline.skills);
        Ok(())
    }
}

#[cfg(test)]
#[path = "provider_session_event_hub_inbox_test.rs"]
mod tests;
