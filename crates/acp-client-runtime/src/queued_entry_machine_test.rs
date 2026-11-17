#![deny(clippy::wildcard_enum_match_arm)]
use super::*;
use state_machine_core::check_state_machine_transitions;

fn every_state() -> Vec<QueuedEntry> {
    vec![
        QueuedEntry::Waiting,
        QueuedEntry::Held,
        QueuedEntry::Committed,
        QueuedEntry::Marking,
        QueuedEntry::Withdrawing(NotSentReason::SessionClosed),
        QueuedEntry::Sent,
        QueuedEntry::Done(EntryEnding::Sent(SentOutcome::Applied)),
        QueuedEntry::Done(EntryEnding::NotSent(NotSentReason::SessionClosed)),
    ]
}

fn every_event() -> Vec<QueuedEntryEvent> {
    use QueuedEntryEvent as V;
    vec![
        V::Claimed,
        V::Commit,
        V::StartSendClaimed,
        V::MarkCommitted {
            transport_open: true,
            run_live: true,
        },
        V::MarkCommitted {
            transport_open: false,
            run_live: true,
        },
        V::MarkCommitted {
            transport_open: true,
            run_live: false,
        },
        V::MarkFailed,
        V::MarkUncertain,
        V::Withdraw(NotSentReason::SessionClosed),
        V::Cancel,
        V::UnmarkCommitted,
        V::UnmarkFailed,
        V::TurnSettled(SentOutcome::Applied),
    ]
}

/// Oracle: design note §12.2 and §13.2's tables (M2).
#[test]
fn every_state_event_pair_reaches_exactly_the_declared_edges() {
    // Arrange
    let cases = every_state()
        .into_iter()
        .flat_map(|state| every_event().into_iter().map(move |event| (state, event)));

    // Act
    let checked = check_state_machine_transitions::<QueuedEntryMachine>(cases);

    // Assert
    assert_eq!(checked, Ok(()));
}

fn run(events: &[QueuedEntryEvent]) -> (QueuedEntry, Vec<QueuedEntryEffect>) {
    let mut state = QueuedEntry::Waiting;
    let mut effects = Vec::new();
    for event in events {
        if let Ok((next, more)) = QueuedEntryMachine::transition(&state, *event) {
            state = next;
            effects.extend(more);
        }
    }
    (state, effects)
}

/// Oracle: §12.2, §13.1, §13.2, §15.1. Each path from a claim ends in one
/// Done with one settlement, and only the handover sends.
#[test]
fn every_path_settles_once_and_only_the_handover_sends() {
    // Arrange
    use QueuedEntryEvent as V;
    let to_marking = [V::Claimed, V::Commit, V::StartSendClaimed];
    let open = V::MarkCommitted {
        transport_open: true,
        run_live: true,
    };
    let paths: Vec<(Vec<QueuedEntryEvent>, QueuedEntry, Vec<QueuedEntryEffect>)> = vec![
        (
            [
                &to_marking[..],
                &[open, V::TurnSettled(SentOutcome::Unknown)],
            ]
            .concat(),
            QueuedEntry::Done(EntryEnding::Sent(SentOutcome::Unknown)),
            vec![
                QueuedEntryEffect::StartSend,
                QueuedEntryEffect::WriteMark,
                QueuedEntryEffect::SendRequest,
                QueuedEntryEffect::SettleSent(SentOutcome::Unknown),
            ],
        ),
        (
            vec![
                V::Claimed,
                V::Commit,
                V::Withdraw(NotSentReason::SessionClosed),
            ],
            QueuedEntry::Done(EntryEnding::NotSent(NotSentReason::SessionClosed)),
            vec![
                QueuedEntryEffect::StartSend,
                QueuedEntryEffect::SettleNotSent(NotSentReason::SessionClosed),
            ],
        ),
        (
            [
                &to_marking[..],
                &[
                    V::Withdraw(NotSentReason::RouterShuttingDown),
                    open,
                    V::UnmarkFailed,
                ],
            ]
            .concat(),
            QueuedEntry::Done(EntryEnding::NotSent(NotSentReason::RouterShuttingDown)),
            vec![
                QueuedEntryEffect::StartSend,
                QueuedEntryEffect::WriteMark,
                QueuedEntryEffect::Unmark,
                QueuedEntryEffect::OverlayNotSentOverDispatch(NotSentReason::RouterShuttingDown),
            ],
        ),
        (
            [
                &to_marking[..],
                &[
                    V::MarkCommitted {
                        transport_open: false,
                        run_live: true,
                    },
                    V::UnmarkCommitted,
                ],
            ]
            .concat(),
            QueuedEntry::Done(EntryEnding::NotSent(
                NotSentReason::ProviderConnectionClosed,
            )),
            vec![
                QueuedEntryEffect::StartSend,
                QueuedEntryEffect::WriteMark,
                QueuedEntryEffect::Unmark,
                QueuedEntryEffect::SettleNotSent(NotSentReason::ProviderConnectionClosed),
            ],
        ),
        (
            [&to_marking[..], &[V::MarkUncertain]].concat(),
            QueuedEntry::Done(EntryEnding::NotSent(NotSentReason::DispatchRecordUncertain)),
            vec![
                QueuedEntryEffect::StartSend,
                QueuedEntryEffect::WriteMark,
                QueuedEntryEffect::ReadBackRow(NotSentReason::DispatchRecordUncertain),
            ],
        ),
    ];

    for (events, expected_state, expected_effects) in paths {
        // Act
        let (state, effects) = run(&events);

        // Assert
        assert_eq!(state, expected_state, "{events:?}");
        assert_eq!(effects, expected_effects, "{events:?}");
    }
}

fn drawn_event(case: &hegel::TestCase) -> QueuedEntryEvent {
    use QueuedEntryEvent as V;
    use hegel::generators as gs;
    let reason = match case.draw(gs::integers::<u8>().max_value(2)) {
        0 => NotSentReason::SessionClosed,
        1 => NotSentReason::RouterShuttingDown,
        _ => NotSentReason::CancelledFromQueue,
    };
    match case.draw(gs::integers::<u8>().max_value(10)) {
        0 => V::Claimed,
        1 => V::Commit,
        2 => V::StartSendClaimed,
        3 => V::MarkCommitted {
            transport_open: case.draw(gs::booleans()),
            run_live: case.draw(gs::booleans()),
        },
        4 => V::MarkFailed,
        5 => V::MarkUncertain,
        6 => V::Withdraw(reason),
        7 => V::Cancel,
        8 => V::UnmarkCommitted,
        9 => V::UnmarkFailed,
        _ => V::TurnSettled(
            [
                SentOutcome::Applied,
                SentOutcome::Refused,
                SentOutcome::Unknown,
            ][usize::from(case.draw(gs::integers::<u8>().max_value(2)))],
        ),
    }
}

/// Oracle: M8 and R14's "not sent" answers name the reason.
#[test]
fn not_sent_reasons_read_as_their_answers() {
    // Arrange
    let reasons = [
        NotSentReason::SessionClosed,
        NotSentReason::CancelledFromQueue,
        NotSentReason::RouterShuttingDown,
        NotSentReason::ProviderConnectionClosed,
        NotSentReason::DispatchNotRecorded,
        NotSentReason::DispatchRecordUncertain,
        NotSentReason::AgentStopped,
    ];

    // Act
    let phrases: Vec<&str> = reasons.iter().map(|reason| reason.human_phrase()).collect();

    // Assert
    assert_eq!(
        phrases,
        [
            "session closed",
            "cancelled from the queue",
            "Router shutting down",
            "provider connection closed",
            "the dispatch couldn't be recorded",
            "dispatch record uncertain; not sent",
            "the agent stopped",
        ]
    );
}

/// Oracle: R14's one outcome per message, over any order. At most one
/// settlement effect and at most one send; a send only from the handover with
/// the transport open and the run live; no settlement before an ending; a
/// Done entry never moves.
#[hegel::test]
fn any_event_order_settles_at_most_once_and_sends_only_on_handover(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = QueuedEntry::Waiting;
    let mut settlements = 0;
    let mut sends = 0;
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(40)) {
        let event = drawn_event(&case);
        let before = state;
        let Ok((next, effects)) = QueuedEntryMachine::transition(&state, event) else {
            continue;
        };
        assert!(!before.is_terminal(), "a Done entry moved: {before:?}");
        for effect in &effects {
            match effect {
                QueuedEntryEffect::SendRequest => {
                    sends += 1;
                    assert_eq!(before, QueuedEntry::Marking);
                    assert_eq!(
                        event,
                        QueuedEntryEvent::MarkCommitted {
                            transport_open: true,
                            run_live: true
                        }
                    );
                }
                QueuedEntryEffect::SettleNotSent(_)
                | QueuedEntryEffect::ReadBackRow(_)
                | QueuedEntryEffect::OverlayNotSentOverDispatch(_)
                | QueuedEntryEffect::SettleSent(_) => {
                    settlements += 1;
                    assert!(next.is_terminal(), "settled before an ending: {next:?}");
                }
                QueuedEntryEffect::StartSend
                | QueuedEntryEffect::WriteMark
                | QueuedEntryEffect::Unmark => {}
            }
        }
        state = next;
    }
    assert!(settlements <= 1, "settled {settlements} times");
    assert!(sends <= 1, "sent {sends} times");
    assert_eq!(settlements == 1, state.is_terminal());
}
