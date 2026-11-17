#![deny(clippy::wildcard_enum_match_arm)]
use super::*;

fn load(id: u128) -> LoadAttempt {
    LoadAttempt(uuid::Uuid::from_u128(id))
}

fn close(id: u128) -> CloseAttempt {
    CloseAttempt(uuid::Uuid::from_u128(id))
}

fn turn(id: u128) -> TurnId {
    TurnId(uuid::Uuid::from_u128(id))
}

fn operation(name: &str) -> QueuedOperation {
    QueuedOperation(name.to_owned())
}

fn step(
    cell: &SessionCell,
    event: SessionCellEvent,
    effects: &mut Vec<SessionCellEffect>,
) -> SessionCell {
    match SessionCellMachine::transition(cell, event) {
        Ok((next, more)) => {
            effects.extend(more);
            next
        }
        Err(_) => cell.clone(),
    }
}

fn live_with_one_queued() -> SessionCell {
    let mut effects = Vec::new();
    let mut cell = step(
        &SessionCell::fresh(),
        SessionCellEvent::CreateAdmitted,
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::Enqueue {
            operation: operation("a"),
        },
        &mut effects,
    );
    step(&cell, SessionCellEvent::Claim { sequence: 0 }, &mut effects)
}

fn sends(effects: &[SessionCellEffect]) -> usize {
    effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                SessionCellEffect::Entry {
                    effect: QueuedEntryEffect::SendRequest,
                    ..
                }
            )
        })
        .count()
}

/// Oracle: design note §12.2 (R2-2). A close that lands after commit but
/// before the runner's claim settles the message not sent, with no send.
#[test]
fn a_close_before_the_claim_settles_the_committed_message_not_sent() {
    // Arrange
    let mut effects = Vec::new();
    let cell = step(
        &live_with_one_queued(),
        SessionCellEvent::Commit { sequence: 0 },
        &mut effects,
    );

    // Act
    let closing = step(
        &cell,
        SessionCellEvent::CloseRequested { attempt: close(1) },
        &mut effects,
    );
    let late_claim = SessionCellMachine::transition(
        &closing,
        SessionCellEvent::StartSendClaimed { sequence: 0 },
    );

    // Assert
    assert!(closing.entries.is_empty(), "the message left the line");
    assert!(effects.contains(&SessionCellEffect::Entry {
        sequence: 0,
        operation: operation("a"),
        effect: QueuedEntryEffect::SettleNotSent(NotSentReason::SessionClosed),
    }));
    assert!(late_claim.is_err(), "the runner's late claim finds nothing");
    assert_eq!(sends(&effects), 0);
}

/// Oracle: §12.2 and §13.2. A close during the mark withdraws the message:
/// the committed mark is undone and the message settles once, unsent.
#[test]
fn a_close_during_the_mark_withdraws_and_settles_once() {
    // Arrange
    let mut effects = Vec::new();
    let mut cell = step(
        &live_with_one_queued(),
        SessionCellEvent::Commit { sequence: 0 },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::StartSendClaimed { sequence: 0 },
        &mut effects,
    );

    // Act
    cell = step(
        &cell,
        SessionCellEvent::CloseRequested { attempt: close(1) },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::MarkCommitted {
            sequence: 0,
            transport_open: true,
            turn: turn(1),
        },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::UnmarkCommitted { sequence: 0 },
        &mut effects,
    );

    // Assert
    let settlements = effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                SessionCellEffect::Entry {
                    effect: QueuedEntryEffect::SettleNotSent(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(settlements, 1);
    assert_eq!(sends(&effects), 0);
    assert!(cell.entries.is_empty());
    assert_eq!(cell.turn, SessionTurn::Idle);
}

/// Oracle: §12.1 and §13.1. After the handover, a close cancels the turn and
/// frees the session only when that turn ends; the turn's outcome settles
/// the message first; close done ends NotLoaded and clears the fast hold.
#[test]
fn a_close_after_the_handover_waits_for_the_turn_then_frees() {
    // Arrange
    let mut effects = Vec::new();
    let mut cell = step(
        &live_with_one_queued(),
        SessionCellEvent::KeepFastOff(true),
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::Commit { sequence: 0 },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::StartSendClaimed { sequence: 0 },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::MarkCommitted {
            sequence: 0,
            transport_open: true,
            turn: turn(1),
        },
        &mut effects,
    );

    // Act
    cell = step(
        &cell,
        SessionCellEvent::CloseRequested { attempt: close(1) },
        &mut effects,
    );
    let ending_turn = cell.clone();
    cell = step(
        &cell,
        SessionCellEvent::TurnEnded {
            turn: turn(1),
            outcome: SentOutcome::Applied,
        },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::CloseDone { attempt: close(1) },
        &mut effects,
    );

    // Assert
    assert_eq!(sends(&effects), 1);
    assert!(matches!(
        ending_turn.lifecycle,
        SessionLifecycle::Closing {
            phase: crate::session_lifecycle_machine::ClosePhase::EndingTurn,
            ..
        }
    ));
    assert!(
        effects.contains(&SessionCellEffect::Turn(SessionTurnEffect::WriteCancel {
            turn: turn(1)
        }))
    );
    assert!(effects.contains(&SessionCellEffect::Entry {
        sequence: 0,
        operation: operation("a"),
        effect: QueuedEntryEffect::SettleSent(SentOutcome::Applied),
    }));
    assert!(matches!(cell.lifecycle, SessionLifecycle::NotLoaded { .. }));
    assert!(!cell.keep_fast_off, "close clears the fast hold");
}

/// Oracle: R9 (design note §12.3). An actor exit keeps the session's fast
/// hold for the next run.
#[test]
fn an_actor_exit_keeps_the_fast_hold() {
    // Arrange
    let mut effects = Vec::new();
    let cell = step(
        &live_with_one_queued(),
        SessionCellEvent::KeepFastOff(true),
        &mut effects,
    );
    let generation = match cell.lifecycle {
        SessionLifecycle::Live { generation } => generation,
        SessionLifecycle::NotLoaded { .. }
        | SessionLifecycle::Loading { .. }
        | SessionLifecycle::Closing { .. } => {
            panic!("not live")
        }
    };

    // Act
    let exited = step(
        &cell,
        SessionCellEvent::ActorExited { generation },
        &mut effects,
    );
    let reloading = step(
        &exited,
        SessionCellEvent::LoadRequested {
            via: RestoreVia::Load,
            attempt: load(1),
        },
        &mut effects,
    );

    // Assert
    assert!(exited.keep_fast_off);
    assert!(reloading.keep_fast_off);
    assert!(effects.contains(&SessionCellEffect::Entry {
        sequence: 0,
        operation: operation("a"),
        effect: QueuedEntryEffect::SettleNotSent(NotSentReason::AgentStopped),
    }));
}

fn drawn_event(case: &hegel::TestCase, minted: &mut u128) -> SessionCellEvent {
    use SessionCellEvent as V;
    use hegel::generators as gs;
    let id = u128::from(case.draw(gs::integers::<u8>().max_value(2)));
    let sequence = u64::from(case.draw(gs::integers::<u8>().max_value(4)));
    let generation =
        SessionGeneration::from_number(u64::from(case.draw(gs::integers::<u8>().max_value(4))));
    match case.draw(gs::integers::<u8>().max_value(24)) {
        0 => V::CreateAdmitted,
        1 => V::LoadRequested {
            via: RestoreVia::Load,
            attempt: load(id),
        },
        2 => V::Loaded { attempt: load(id) },
        3 => V::CloseRequested { attempt: close(id) },
        4 => V::CloseDone { attempt: close(id) },
        5 => V::CloseFailed { attempt: close(id) },
        6 => V::ActorExited { generation },
        7 | 8 => V::Enqueue {
            operation: operation(&format!(
                "op-{}",
                case.draw(gs::integers::<u8>().max_value(9))
            )),
        },
        9 => V::Claim { sequence },
        10 => V::Commit { sequence },
        11 => V::StartSendClaimed { sequence },
        12 => {
            *minted += 1;
            V::MarkCommitted {
                sequence,
                transport_open: case.draw(gs::booleans()),
                turn: turn(*minted),
            }
        }
        13 => V::MarkFailed { sequence },
        14 => V::UnmarkCommitted { sequence },
        15 => V::CancelEntry {
            operation: operation(&format!(
                "op-{}",
                case.draw(gs::integers::<u8>().max_value(9))
            )),
        },
        16 => V::TurnEnded {
            turn: turn(*minted),
            outcome: SentOutcome::Unknown,
        },
        17 => V::Stop,
        18 => V::KeepFastOff(case.draw(gs::booleans())),
        19 => V::LoadFailed { attempt: load(id) },
        20 => V::Publish { generation },
        21 => V::MarkUncertain { sequence },
        22 => V::UnmarkFailed { sequence },
        23 => V::SteerRequested {
            agent_steers: case.draw(gs::booleans()),
        },
        _ => V::CancelTurn,
    }
}

/// Oracle: R14, M3, M4 and §11.2 over any interleaving on one cell. At most
/// one message is in flight; a send happens only while Live with the line
/// open, opening a running turn in the same move; once a close is accepted,
/// no send happens until a newer run; each message settles at most once.
#[hegel::test]
fn any_interleaving_keeps_one_in_flight_and_no_send_after_close(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut cell = SessionCell::fresh();
    let mut minted: u128 = 0;
    let mut closed_generation: Option<u64> = None;
    let mut settled: Vec<(u64, QueuedOperation, u64)> = Vec::new();
    let mut epoch_of_sequence: BTreeMap<u64, u64> = BTreeMap::new();
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(80)) {
        let event = drawn_event(&case, &mut minted);
        let before = cell.clone();
        let Ok((next, effects)) = SessionCellMachine::transition(&cell, event.clone()) else {
            continue;
        };
        let in_flight = next
            .entries
            .values()
            .filter(|slot| {
                matches!(
                    slot.entry,
                    QueuedEntry::Committed
                        | QueuedEntry::Marking
                        | QueuedEntry::Withdrawing(_)
                        | QueuedEntry::Sent
                )
            })
            .count();
        assert!(in_flight <= 1, "{in_flight} messages in flight");
        if matches!(event, SessionCellEvent::CloseRequested { .. }) {
            closed_generation = before
                .live_generation()
                .or(before.run_generation())
                .map(SessionGeneration::number);
        }
        for effect in &effects {
            match effect {
                SessionCellEffect::Entry {
                    effect: QueuedEntryEffect::SendRequest,
                    ..
                } => {
                    let live = before.live_generation().expect("sent while not Live");
                    assert!(before.line_open_for(live), "sent with the line not open");
                    assert!(
                        matches!(next.turn, SessionTurn::Running { .. }),
                        "the turn didn't open with the send"
                    );
                    if let Some(closed) = closed_generation {
                        assert!(live.number() > closed, "sent in a run after its close");
                    }
                }
                SessionCellEffect::Entry {
                    sequence,
                    operation,
                    effect:
                        QueuedEntryEffect::SettleNotSent(_)
                        | QueuedEntryEffect::SettleSent(_)
                        | QueuedEntryEffect::ReadBackRow(_)
                        | QueuedEntryEffect::OverlayNotSentOverDispatch(_),
                } => {
                    let epoch = *epoch_of_sequence.entry(*sequence).or_insert(0);
                    let key = (*sequence, operation.clone(), epoch);
                    assert!(!settled.contains(&key), "a message settled twice: {key:?}");
                    settled.push(key);
                }
                SessionCellEffect::Entry { .. }
                | SessionCellEffect::Lifecycle(_)
                | SessionCellEffect::Queued { .. }
                | SessionCellEffect::Turn(_) => {}
            }
        }
        if let (
            QueueLine::Open { epoch: was, .. } | QueueLine::Ended { epoch: was, .. },
            QueueLine::Open { epoch: now, .. },
        ) = (before.line, next.line)
            && now.number() > was.number()
        {
            epoch_of_sequence.clear();
            settled.clear();
        }
        cell = next;
    }
}

/// Oracle: §11.2's linearization. With two messages queued, the second
/// can't be claimed into flight or committed while the first is in flight;
/// it becomes the head once the first's turn ends.
#[test]
fn a_second_message_waits_until_the_first_ends() {
    // Arrange
    let mut effects = Vec::new();
    let mut cell = step(
        &live_with_one_queued(),
        SessionCellEvent::Enqueue {
            operation: operation("b"),
        },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::Commit { sequence: 0 },
        &mut effects,
    );
    cell = step(&cell, SessionCellEvent::Claim { sequence: 1 }, &mut effects);

    // Act
    let while_first_in_flight =
        SessionCellMachine::transition(&cell, SessionCellEvent::Commit { sequence: 1 });
    cell = step(
        &cell,
        SessionCellEvent::StartSendClaimed { sequence: 0 },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::MarkCommitted {
            sequence: 0,
            transport_open: true,
            turn: turn(1),
        },
        &mut effects,
    );
    cell = step(
        &cell,
        SessionCellEvent::TurnEnded {
            turn: turn(1),
            outcome: SentOutcome::Applied,
        },
        &mut effects,
    );
    let after_first_ends =
        SessionCellMachine::transition(&cell, SessionCellEvent::Commit { sequence: 1 });

    // Assert
    assert_eq!(
        while_first_in_flight.map(|_| ()),
        Err(TransitionError::Domain(SessionCellRefusal::NotHead))
    );
    assert!(after_first_ends.is_ok(), "{after_first_ends:?}");
}

/// Oracle: design note §11.2 ("enqueue{op}: lifecycle Loading{g} or
/// Live{g}"; "the next accepted load opens a fresh Line{epoch: g+1}") and the
/// accepted door rule: a message arriving while the session loads joins the
/// run's line, and commits only once the run is Live.
#[test]
fn a_message_arriving_while_the_session_loads_joins_the_line_and_commits_once_live() {
    // Arrange
    let mut effects = Vec::new();
    let loading = step(
        &SessionCell::fresh(),
        SessionCellEvent::LoadRequested {
            via: RestoreVia::Load,
            attempt: load(1),
        },
        &mut effects,
    );

    // Act
    let queued = SessionCellMachine::transition(
        &loading,
        SessionCellEvent::Enqueue {
            operation: operation("a"),
        },
    );
    let (queued, _) = queued.expect("the message joins the loading run's line");
    let held = step(
        &queued,
        SessionCellEvent::Claim { sequence: 0 },
        &mut effects,
    );
    let early_commit =
        SessionCellMachine::transition(&held, SessionCellEvent::Commit { sequence: 0 });
    let live = step(
        &held,
        SessionCellEvent::Loaded { attempt: load(1) },
        &mut effects,
    );
    let commit = SessionCellMachine::transition(&live, SessionCellEvent::Commit { sequence: 0 });

    // Assert
    assert!(
        matches!(
            early_commit,
            Err(TransitionError::Domain(SessionCellRefusal::NotLive))
        ),
        "nothing commits before the run is Live: {early_commit:?}"
    );
    assert!(commit.is_ok(), "the message commits once Live: {commit:?}");
    assert_eq!(live.entries.len(), 1, "the loaded run kept the message");
}

/// Oracle: R14's one outcome per message: a load that fails ends every
/// message waiting in its run's line not sent, "the session couldn't be
/// loaded", and the session is NotLoaded.
#[test]
fn a_failed_load_ends_its_waiting_messages_not_sent() {
    // Arrange
    let mut effects = Vec::new();
    let loading = step(
        &SessionCell::fresh(),
        SessionCellEvent::LoadRequested {
            via: RestoreVia::Load,
            attempt: load(1),
        },
        &mut effects,
    );
    let queued = step(
        &loading,
        SessionCellEvent::Enqueue {
            operation: operation("a"),
        },
        &mut effects,
    );
    effects.clear();

    // Act
    let failed = step(
        &queued,
        SessionCellEvent::LoadFailed { attempt: load(1) },
        &mut effects,
    );

    // Assert
    assert!(matches!(
        failed.lifecycle,
        SessionLifecycle::NotLoaded { .. }
    ));
    assert!(failed.entries.is_empty(), "no message is left waiting");
    assert_eq!(
        effects,
        vec![SessionCellEffect::Entry {
            sequence: 0,
            operation: operation("a"),
            effect: QueuedEntryEffect::SettleNotSent(NotSentReason::LoadFailed),
        }]
    );
    assert_eq!(
        NotSentReason::LoadFailed.human_phrase(),
        "the session couldn't be loaded"
    );
}
