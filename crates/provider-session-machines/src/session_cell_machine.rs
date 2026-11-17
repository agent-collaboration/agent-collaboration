//! One provider session's composite cell: its lifecycle, its queue line and
//! the queued messages in it, its turn, and its session-lifetime fast hold, as
//! one pure machine (D1 state machines; design note §11.2, §12.1–§12.3,
//! §13.1). Commit, cancel, close and shutdown are all transitions on this one
//! cell, so exactly one of them wins. Each move delegates to the sub-machine
//! that owns it, whose table is checked on its own.

use crate::ProviderSettingKind;
use crate::SessionGeneration;
use crate::queue_line_machine::{
    QueueLine, QueueLineEffect, QueueLineEvent, QueueLineMachine, QueueLineRefusal,
};
use crate::queued_entry_machine::{
    NotSentReason, QueuedEntry, QueuedEntryEffect, QueuedEntryEvent, QueuedEntryMachine,
    QueuedEntryRefusal, SentOutcome,
};
use crate::session_lifecycle_machine::{
    CloseAttempt, LoadAttempt, RestoreVia, SessionLifecycle, SessionLifecycleEffect,
    SessionLifecycleEvent, SessionLifecycleMachine, SessionLifecycleRefusal,
};
use crate::session_setting_machine::{
    SessionSetting, SessionSettingEvent, SessionSettingMachine, SessionSettingRefusal,
    SettingValueId, UnresolvedCause,
};
use crate::session_turn_machine::{
    SessionTurn, SessionTurnEffect, SessionTurnEvent, SessionTurnMachine, SessionTurnRefusal,
    TurnId,
};
use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};
use std::collections::{BTreeMap, HashMap};

/// A queued message's operation, as the Host names it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QueuedOperation(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntrySlot {
    pub operation: QueuedOperation,
    pub entry: QueuedEntry,
    pub turn: Option<TurnId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionCell {
    pub lifecycle: SessionLifecycle,
    pub line: QueueLine,
    pub entries: BTreeMap<u64, EntrySlot>,
    pub turn: SessionTurn,
    /// The caller asked to keep fast off: it lasts the session's lifetime,
    /// across actor exits, until fast-auto, accepting a refused fast, close,
    /// or a Router restart (R9; design note §12.3).
    pub keep_fast_off: bool,
    /// Each setting kind's state this run (design note §11.6, §12.6, with
    /// the Lead's 3b-5 ruling: a prompt waits behind an in-flight change in
    /// the actor and is refused only while a kind is Unresolved).
    pub settings: HashMap<ProviderSettingKind, SessionSetting>,
}

impl SessionCell {
    /// A session this connection hasn't run yet.
    pub fn fresh() -> Self {
        Self {
            lifecycle: SessionLifecycle::NotLoaded {
                last: SessionGeneration::UNTRACKED,
            },
            line: QueueLine::Ended {
                epoch: SessionGeneration::UNTRACKED,
                reason: NotSentReason::SessionClosed,
                next_sequence: 0,
            },
            entries: BTreeMap::new(),
            turn: SessionTurn::Idle,
            keep_fast_off: false,
            settings: HashMap::new(),
        }
    }

    pub fn run_generation(&self) -> Option<SessionGeneration> {
        match self.lifecycle {
            SessionLifecycle::Loading { generation, .. }
            | SessionLifecycle::Live { generation } => Some(generation),
            SessionLifecycle::NotLoaded { .. } | SessionLifecycle::Closing { .. } => None,
        }
    }

    fn live_generation(&self) -> Option<SessionGeneration> {
        match self.lifecycle {
            SessionLifecycle::Live { generation } => Some(generation),
            SessionLifecycle::NotLoaded { .. }
            | SessionLifecycle::Loading { .. }
            | SessionLifecycle::Closing { .. } => None,
        }
    }

    /// A kind holds prompts: its last change was refused (where refusals
    /// hold) or its outcome is unknown.
    pub fn settings_held(&self) -> bool {
        self.settings
            .values()
            .any(|setting| matches!(setting, SessionSetting::Unresolved { .. }))
    }

    /// Some held setting's last change has an unknown outcome.
    pub fn any_setting_outcome_unknown(&self) -> bool {
        self.settings.values().any(|setting| {
            matches!(
                setting,
                SessionSetting::Unresolved {
                    cause: UnresolvedCause::OutcomeUnknown,
                    ..
                }
            )
        })
    }

    /// The kind's last change has an unknown outcome.
    pub fn setting_outcome_unknown(&self, kind: ProviderSettingKind) -> bool {
        matches!(
            self.settings.get(&kind),
            Some(SessionSetting::Unresolved {
                cause: UnresolvedCause::OutcomeUnknown,
                ..
            })
        )
    }

    fn line_open_for(&self, generation: SessionGeneration) -> bool {
        matches!(self.line, QueueLine::Open { epoch, .. } if epoch == generation)
    }

    /// The lowest waiting or held message: the only one that may commit.
    fn head(&self) -> Option<u64> {
        self.entries
            .iter()
            .find(|(_, slot)| matches!(slot.entry, QueuedEntry::Waiting | QueuedEntry::Held))
            .map(|(sequence, _)| *sequence)
    }

    /// A message is between commit and its turn's end.
    fn in_flight(&self) -> bool {
        self.entries.values().any(|slot| {
            matches!(
                slot.entry,
                QueuedEntry::Committed
                    | QueuedEntry::Marking
                    | QueuedEntry::Withdrawing(_)
                    | QueuedEntry::Sent
            )
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionCellEvent {
    CreateAdmitted,
    LoadRequested {
        via: RestoreVia,
        attempt: LoadAttempt,
    },
    Loaded {
        attempt: LoadAttempt,
    },
    LoadFailed {
        attempt: LoadAttempt,
    },
    CloseRequested {
        attempt: CloseAttempt,
    },
    CloseDone {
        attempt: CloseAttempt,
    },
    CloseFailed {
        attempt: CloseAttempt,
    },
    ActorExited {
        generation: SessionGeneration,
    },
    Publish {
        generation: SessionGeneration,
    },
    Enqueue {
        operation: QueuedOperation,
    },
    Claim {
        sequence: u64,
    },
    Commit {
        sequence: u64,
    },
    StartSendClaimed {
        sequence: u64,
    },
    MarkCommitted {
        sequence: u64,
        transport_open: bool,
        turn: TurnId,
    },
    MarkFailed {
        sequence: u64,
    },
    MarkUncertain {
        sequence: u64,
    },
    UnmarkCommitted {
        sequence: u64,
    },
    UnmarkFailed {
        sequence: u64,
    },
    CancelEntry {
        operation: QueuedOperation,
    },
    /// A prompt sent directly, not from the line, opens the session's turn.
    PromptStarted {
        turn: TurnId,
    },
    SteerRequested {
        agent_steers: bool,
    },
    CancelTurn,
    TurnEnded {
        turn: TurnId,
        outcome: SentOutcome,
    },
    Stop,
    /// The provider connection ended: its lines end.
    ConnectionClosed,
    /// One setting kind's event (a change requested, its result, an
    /// observation, a lost reply).
    Setting {
        kind: ProviderSettingKind,
        event: SessionSettingEvent,
    },
    /// The caller accepts every refused change at once; an in-flight change
    /// is left alone, and an unknown outcome refuses the whole accept.
    AcceptSettings,
    KeepFastOff(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionCellEffect {
    Lifecycle(SessionLifecycleEffect),
    /// A message joined the run's line under this number.
    Queued {
        generation: SessionGeneration,
        sequence: u64,
    },
    Entry {
        sequence: u64,
        operation: QueuedOperation,
        effect: QueuedEntryEffect,
    },
    Turn(SessionTurnEffect),
    /// The caller accepted this kind's refused change.
    SettingAccepted {
        kind: ProviderSettingKind,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionCellRefusal {
    Lifecycle(SessionLifecycleRefusal),
    Line(QueueLineRefusal),
    Entry(QueuedEntryRefusal),
    Turn(SessionTurnRefusal),
    /// Only the line's head may commit, and only one message is in flight.
    NotHead,
    UnknownEntry,
    /// The session isn't Live in the line's run, so nothing commits.
    NotLive,
    Setting(SessionSettingRefusal),
}

impl RefusalTag for SessionCellRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::Lifecycle(refusal) => refusal.refusal_tag(),
            Self::Line(refusal) => refusal.refusal_tag(),
            Self::Entry(refusal) => refusal.refusal_tag(),
            Self::Turn(refusal) => refusal.refusal_tag(),
            Self::NotHead => "not_head",
            Self::UnknownEntry => "unknown_entry",
            Self::NotLive => "not_live",
            Self::Setting(refusal) => refusal.refusal_tag(),
        }
    }
}

impl StateTag for SessionCell {
    fn state_tag(&self) -> &'static str {
        self.lifecycle.state_tag()
    }
    fn is_terminal(&self) -> bool {
        false
    }
}

impl EventTag for SessionCellEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::CreateAdmitted => "create_admitted",
            Self::LoadRequested { .. } => "load_requested",
            Self::Loaded { .. } => "loaded",
            Self::LoadFailed { .. } => "load_failed",
            Self::CloseRequested { .. } => "close_requested",
            Self::CloseDone { .. } => "close_done",
            Self::CloseFailed { .. } => "close_failed",
            Self::ActorExited { .. } => "actor_exited",
            Self::Publish { .. } => "publish",
            Self::Enqueue { .. } => "enqueue",
            Self::Claim { .. } => "claim",
            Self::Commit { .. } => "commit",
            Self::StartSendClaimed { .. } => "start_send_claimed",
            Self::MarkCommitted { .. } => "mark_committed",
            Self::MarkFailed { .. } => "mark_failed",
            Self::MarkUncertain { .. } => "mark_uncertain",
            Self::UnmarkCommitted { .. } => "unmark_committed",
            Self::UnmarkFailed { .. } => "unmark_failed",
            Self::CancelEntry { .. } => "cancel_entry",
            Self::PromptStarted { .. } => "prompt_started",
            Self::SteerRequested { .. } => "steer_requested",
            Self::CancelTurn => "cancel_turn",
            Self::TurnEnded { .. } => "turn_ended",
            Self::Stop => "stop",
            Self::ConnectionClosed => "connection_closed",
            Self::Setting { .. } => "setting",
            Self::AcceptSettings => "accept_settings",
            Self::KeepFastOff(_) => "keep_fast_off",
        }
    }
}

fn lift<E>(
    refusal: TransitionError<E>,
    wrap: impl FnOnce(E) -> SessionCellRefusal,
) -> TransitionError<SessionCellRefusal> {
    match refusal {
        TransitionError::Domain(refusal) => TransitionError::Domain(wrap(refusal)),
        TransitionError::NotAllowed { state, event } => {
            TransitionError::NotAllowed { state, event }
        }
        TransitionError::Stale => TransitionError::Stale,
    }
}

/// Every waiting, held or committed message ends not sent; a marking one is
/// withdrawn for its runner to settle; a sent one is left to its turn.
fn withdraw_all(
    cell: &mut SessionCell,
    reason: NotSentReason,
    effects: &mut Vec<SessionCellEffect>,
) {
    for (sequence, slot) in &mut cell.entries {
        if let Ok((next, more)) =
            QueuedEntryMachine::transition(&slot.entry, QueuedEntryEvent::Withdraw(reason))
        {
            slot.entry = next;
            effects.extend(more.into_iter().map(|effect| SessionCellEffect::Entry {
                sequence: *sequence,
                operation: slot.operation.clone(),
                effect,
            }));
        }
    }
    cell.entries.retain(|_, slot| !slot.entry.is_terminal());
}

fn end_line(cell: &mut SessionCell, reason: NotSentReason, effects: &mut Vec<SessionCellEffect>) {
    if let Ok((line, _)) = QueueLineMachine::transition(&cell.line, QueueLineEvent::End { reason })
    {
        cell.line = line;
    }
    withdraw_all(cell, reason, effects);
}

/// Applies a lifecycle event and carries its effects into the line and turn.
fn lifecycle_move(
    cell: &SessionCell,
    event: SessionLifecycleEvent,
) -> StateMachineResult<SessionCellMachine> {
    let (lifecycle, lifecycle_effects) =
        SessionLifecycleMachine::transition(&cell.lifecycle, event)
            .map_err(|refusal| lift(refusal, SessionCellRefusal::Lifecycle))?;
    let mut next = cell.clone();
    next.lifecycle = lifecycle;
    let mut effects = Vec::new();
    for effect in lifecycle_effects {
        match effect {
            SessionLifecycleEffect::EndLine { .. } => {
                end_line(&mut next, NotSentReason::SessionClosed, &mut effects)
            }
            SessionLifecycleEffect::CancelTurn { .. } => {
                if let Ok((turn, more)) =
                    SessionTurnMachine::transition(&next.turn, SessionTurnEvent::CancelRequested)
                {
                    next.turn = turn;
                    effects.extend(more.into_iter().map(SessionCellEffect::Turn));
                }
            }
            SessionLifecycleEffect::ReopenLine { generation } => {
                if let Ok((line, _)) = QueueLineMachine::transition(
                    &next.line,
                    QueueLineEvent::Reopen { epoch: generation },
                ) {
                    next.line = line;
                }
            }
            // An accepted load opens the run's fresh line, so a message
            // arriving while it loads joins it (design note §11.2); it
            // commits only once the run is Live.
            SessionLifecycleEffect::StartRestore { generation, .. } => {
                if let Ok((line, _)) = QueueLineMachine::transition(
                    &next.line,
                    QueueLineEvent::OpenFresh { epoch: generation },
                ) {
                    next.line = line;
                }
                effects.push(SessionCellEffect::Lifecycle(effect));
            }
            SessionLifecycleEffect::RegisterActor { generation } => {
                if let Ok((line, _)) = QueueLineMachine::transition(
                    &next.line,
                    QueueLineEvent::OpenFresh { epoch: generation },
                ) {
                    next.line = line;
                }
                effects.push(SessionCellEffect::Lifecycle(effect));
            }
            SessionLifecycleEffect::PublishClosed { .. } => {
                next.keep_fast_off = false;
                next.settings.clear();
                effects.push(SessionCellEffect::Lifecycle(effect));
            }
            SessionLifecycleEffect::PublishUnloaded { .. } => {
                // The run ends: its settings sub-machines go with it, as on
                // Closed; the fast hold is the session's and stays (note
                // §12.3, R2-3; Lead V5).
                next.settings.clear();
                end_line(&mut next, NotSentReason::AgentStopped, &mut effects);
                effects.push(SessionCellEffect::Lifecycle(effect));
            }
            SessionLifecycleEffect::PublishActivation { .. }
            | SessionLifecycleEffect::StartFree { .. }
            | SessionLifecycleEffect::PublishToHub { .. } => {
                effects.push(SessionCellEffect::Lifecycle(effect))
            }
        }
    }
    Ok((next, effects))
}

/// Applies an entry event to the message at `sequence`.
fn entry_move(
    cell: &SessionCell,
    sequence: u64,
    event: QueuedEntryEvent,
    turn: Option<TurnId>,
) -> StateMachineResult<SessionCellMachine> {
    let slot = cell
        .entries
        .get(&sequence)
        .ok_or(TransitionError::Domain(SessionCellRefusal::UnknownEntry))?;
    let (entry, entry_effects) = QueuedEntryMachine::transition(&slot.entry, event)
        .map_err(|refusal| lift(refusal, SessionCellRefusal::Entry))?;
    let mut next = cell.clone();
    let mut effects: Vec<SessionCellEffect> = entry_effects
        .into_iter()
        .map(|effect| SessionCellEffect::Entry {
            sequence,
            operation: slot.operation.clone(),
            effect,
        })
        .collect();
    if entry == QueuedEntry::Sent
        && let Some(turn) = turn
    {
        // The handover opens the turn in the same move (design note §13.1).
        let (opened, more) =
            SessionTurnMachine::transition(&next.turn, SessionTurnEvent::PromptStarted { turn })
                .map_err(|refusal| lift(refusal, SessionCellRefusal::Turn))?;
        next.turn = opened;
        effects.extend(more.into_iter().map(SessionCellEffect::Turn));
    }
    if let Some(slot) = next.entries.get_mut(&sequence) {
        slot.entry = entry;
        if entry == QueuedEntry::Sent {
            slot.turn = turn;
        }
    }
    next.entries.retain(|_, slot| !slot.entry.is_terminal());
    Ok((next, effects))
}

fn turn_ended(
    cell: &SessionCell,
    turn: TurnId,
    outcome: SentOutcome,
) -> StateMachineResult<SessionCellMachine> {
    let (idle, turn_effects) =
        SessionTurnMachine::transition(&cell.turn, SessionTurnEvent::TurnEnded { turn })
            .map_err(|refusal| lift(refusal, SessionCellRefusal::Turn))?;
    let generation = match cell.lifecycle {
        SessionLifecycle::Closing { generation, .. }
        | SessionLifecycle::Live { generation }
        | SessionLifecycle::Loading { generation, .. }
        | SessionLifecycle::NotLoaded { last: generation } => generation,
    };
    let mut next = cell.clone();
    next.turn = idle;
    let mut effects: Vec<SessionCellEffect> = turn_effects
        .into_iter()
        .map(SessionCellEffect::Turn)
        .collect();
    if let Some(sequence) = next
        .entries
        .iter()
        .find(|(_, slot)| slot.turn == Some(turn))
        .map(|(sequence, _)| *sequence)
        && let Ok((settled, more)) = entry_move(
            &next,
            sequence,
            QueuedEntryEvent::TurnSettled(outcome),
            None,
        )
    {
        next = settled;
        effects.extend(more);
    }
    if let Ok((closing, more)) =
        lifecycle_move(&next, SessionLifecycleEvent::TurnEnded { generation })
    {
        next = closing;
        effects.extend(more);
    }
    Ok((next, effects))
}

pub struct SessionCellMachine;

impl StateMachine for SessionCellMachine {
    type State = SessionCell;
    type Event = SessionCellEvent;
    type Effect = SessionCellEffect;
    type Error = SessionCellRefusal;

    fn transition(cell: &SessionCell, event: SessionCellEvent) -> StateMachineResult<Self> {
        use SessionCellEvent as V;
        match event {
            V::CreateAdmitted => lifecycle_move(cell, SessionLifecycleEvent::CreateAdmitted),
            V::LoadRequested { via, attempt } => {
                lifecycle_move(cell, SessionLifecycleEvent::LoadRequested { via, attempt })
            }
            V::Loaded { attempt } => {
                lifecycle_move(cell, SessionLifecycleEvent::Loaded { attempt })
            }
            V::LoadFailed { attempt } => {
                // The failed load's run ends, and with it every message that
                // waited in its line.
                let (mut next, mut effects) =
                    lifecycle_move(cell, SessionLifecycleEvent::LoadFailed { attempt })?;
                end_line(&mut next, NotSentReason::LoadFailed, &mut effects);
                Ok((next, effects))
            }
            V::CloseRequested { attempt } => {
                let turn_running = matches!(
                    cell.turn,
                    SessionTurn::Running { .. } | SessionTurn::Cancelling { .. }
                );
                lifecycle_move(
                    cell,
                    SessionLifecycleEvent::CloseRequested {
                        attempt,
                        turn_running,
                    },
                )
            }
            V::CloseDone { attempt } => {
                lifecycle_move(cell, SessionLifecycleEvent::CloseDone { attempt })
            }
            V::CloseFailed { attempt } => {
                lifecycle_move(cell, SessionLifecycleEvent::CloseFailed { attempt })
            }
            V::ActorExited { generation } => {
                lifecycle_move(cell, SessionLifecycleEvent::ActorExited { generation })
            }
            V::Publish { generation } => {
                lifecycle_move(cell, SessionLifecycleEvent::Publish { generation })
            }
            V::Enqueue { operation } => {
                let Some(generation) = cell.run_generation() else {
                    // NotLoaded or Closing: answer with the lifecycle's own refusal.
                    return Err(
                        match lifecycle_move(cell, SessionLifecycleEvent::PromptRequested) {
                            Err(refusal) => refusal,
                            Ok(_) => TransitionError::Stale,
                        },
                    );
                };
                let (line, line_effects) = QueueLineMachine::transition(
                    &cell.line,
                    QueueLineEvent::Enqueue { epoch: generation },
                )
                .map_err(|refusal| lift(refusal, SessionCellRefusal::Line))?;
                let mut next = cell.clone();
                next.line = line;
                let mut effects = Vec::new();
                for effect in line_effects {
                    match effect {
                        QueueLineEffect::Admit { sequence } => {
                            effects.push(SessionCellEffect::Queued {
                                generation,
                                sequence,
                            });
                            next.entries.insert(
                                sequence,
                                EntrySlot {
                                    operation: operation.clone(),
                                    entry: QueuedEntry::Waiting,
                                    turn: None,
                                },
                            );
                        }
                        QueueLineEffect::WithdrawAll { reason } => {
                            withdraw_all(&mut next, reason, &mut effects)
                        }
                    }
                }
                Ok((next, effects))
            }
            V::Claim { sequence } => {
                if cell.head() != Some(sequence) {
                    return Err(TransitionError::Domain(SessionCellRefusal::NotHead));
                }
                entry_move(cell, sequence, QueuedEntryEvent::Claimed, None)
            }
            V::Commit { sequence } => {
                let Some(generation) = cell.live_generation() else {
                    return Err(TransitionError::Domain(SessionCellRefusal::NotLive));
                };
                if !cell.line_open_for(generation) {
                    return Err(TransitionError::Domain(SessionCellRefusal::NotLive));
                }
                if cell.head() != Some(sequence)
                    || cell.in_flight()
                    || cell.turn != SessionTurn::Idle
                {
                    return Err(TransitionError::Domain(SessionCellRefusal::NotHead));
                }
                entry_move(cell, sequence, QueuedEntryEvent::Commit, None)
            }
            V::StartSendClaimed { sequence } => {
                entry_move(cell, sequence, QueuedEntryEvent::StartSendClaimed, None)
            }
            V::MarkCommitted {
                sequence,
                transport_open,
                turn,
            } => {
                let run_live = cell
                    .live_generation()
                    .is_some_and(|generation| cell.line_open_for(generation));
                entry_move(
                    cell,
                    sequence,
                    QueuedEntryEvent::MarkCommitted {
                        transport_open,
                        run_live,
                    },
                    Some(turn),
                )
            }
            V::MarkFailed { sequence } => {
                entry_move(cell, sequence, QueuedEntryEvent::MarkFailed, None)
            }
            V::MarkUncertain { sequence } => {
                entry_move(cell, sequence, QueuedEntryEvent::MarkUncertain, None)
            }
            V::UnmarkCommitted { sequence } => {
                entry_move(cell, sequence, QueuedEntryEvent::UnmarkCommitted, None)
            }
            V::UnmarkFailed { sequence } => {
                entry_move(cell, sequence, QueuedEntryEvent::UnmarkFailed, None)
            }
            V::CancelEntry { operation } => {
                let sequence = cell
                    .entries
                    .iter()
                    .find(|(_, slot)| slot.operation == operation)
                    .map(|(sequence, _)| *sequence)
                    .ok_or(TransitionError::Domain(SessionCellRefusal::UnknownEntry))?;
                entry_move(cell, sequence, QueuedEntryEvent::Cancel, None)
            }
            V::PromptStarted { turn } => {
                if cell.live_generation().is_none() {
                    // NotLoaded, Loading or Closing: the lifecycle's refusal.
                    return Err(
                        match lifecycle_move(cell, SessionLifecycleEvent::PromptRequested) {
                            Err(refusal) => refusal,
                            Ok(_) => TransitionError::Stale,
                        },
                    );
                }
                if cell.in_flight() {
                    return Err(TransitionError::Domain(SessionCellRefusal::Turn(
                        SessionTurnRefusal::TurnRunning,
                    )));
                }
                let (opened, effects) = SessionTurnMachine::transition(
                    &cell.turn,
                    SessionTurnEvent::PromptStarted { turn },
                )
                .map_err(|refusal| lift(refusal, SessionCellRefusal::Turn))?;
                let mut next = cell.clone();
                next.turn = opened;
                Ok((
                    next,
                    effects.into_iter().map(SessionCellEffect::Turn).collect(),
                ))
            }
            V::SteerRequested { agent_steers } => {
                let (turn, effects) = SessionTurnMachine::transition(
                    &cell.turn,
                    SessionTurnEvent::SteerRequested { agent_steers },
                )
                .map_err(|refusal| lift(refusal, SessionCellRefusal::Turn))?;
                let mut next = cell.clone();
                next.turn = turn;
                Ok((
                    next,
                    effects.into_iter().map(SessionCellEffect::Turn).collect(),
                ))
            }
            V::CancelTurn => {
                let (turn, effects) =
                    SessionTurnMachine::transition(&cell.turn, SessionTurnEvent::CancelRequested)
                        .map_err(|refusal| lift(refusal, SessionCellRefusal::Turn))?;
                let mut next = cell.clone();
                next.turn = turn;
                Ok((
                    next,
                    effects.into_iter().map(SessionCellEffect::Turn).collect(),
                ))
            }
            V::TurnEnded { turn, outcome } => turn_ended(cell, turn, outcome),
            V::Stop => {
                let mut next = cell.clone();
                let mut effects = Vec::new();
                end_line(&mut next, NotSentReason::RouterShuttingDown, &mut effects);
                Ok((next, effects))
            }
            V::ConnectionClosed => {
                let mut next = cell.clone();
                let mut effects = Vec::new();
                end_line(
                    &mut next,
                    NotSentReason::ProviderConnectionClosed,
                    &mut effects,
                );
                Ok((next, effects))
            }
            V::Setting { kind, event } => {
                let current =
                    cell.settings
                        .get(&kind)
                        .cloned()
                        .unwrap_or(SessionSetting::Current {
                            value: SettingValueId(String::new()),
                        });
                let (setting, _effects) = SessionSettingMachine::transition(&current, event)
                    .map_err(|refusal| lift(refusal, SessionCellRefusal::Setting))?;
                let mut next = cell.clone();
                next.settings.insert(kind, setting);
                Ok((next, Vec::new()))
            }
            V::AcceptSettings => {
                if cell.settings.values().any(|setting| {
                    matches!(
                        setting,
                        SessionSetting::Unresolved {
                            cause: UnresolvedCause::OutcomeUnknown,
                            ..
                        }
                    )
                }) {
                    return Err(TransitionError::Domain(SessionCellRefusal::Setting(
                        SessionSettingRefusal::OutcomeUnknown,
                    )));
                }
                let mut next = cell.clone();
                let mut effects = Vec::new();
                for (kind, setting) in &cell.settings {
                    if let SessionSetting::Unresolved {
                        cause: UnresolvedCause::DefiniteFailure,
                        ..
                    } = setting
                        && let Ok((accepted, _)) =
                            SessionSettingMachine::transition(setting, SessionSettingEvent::Accept)
                    {
                        next.settings.insert(*kind, accepted);
                        effects.push(SessionCellEffect::SettingAccepted { kind: *kind });
                    }
                }
                Ok((next, effects))
            }
            V::KeepFastOff(keep) => {
                let mut next = cell.clone();
                next.keep_fast_off = keep;
                Ok((next, Vec::new()))
            }
        }
    }

    /// The composite's moves are its sub-machines' declared moves, each
    /// checked by its own table test; the composite is proven by its
    /// property tests.
    fn transitions() -> &'static [Transition] {
        &[]
    }
}

#[cfg(test)]
#[path = "session_cell_machine_test.rs"]
mod tests;
