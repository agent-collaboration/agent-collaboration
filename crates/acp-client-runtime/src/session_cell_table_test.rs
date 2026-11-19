//! The cell table answers the sink: a publication's run claim is decided by
//! the session's cell (D1 step 6).

use super::*;
use crate::SessionRuns;
use crate::session_cell_table::EntryEffectRoute;
use crate::session_event_sink::GenerationPublication;
use provider_session_machines::SessionGeneration;
use provider_session_machines::session_cell_machine::SessionCellEvent;
use provider_session_machines::session_lifecycle_machine::SessionLifecycle;
use provider_session_machines::session_lifecycle_machine::{CloseAttempt, LoadAttempt, RestoreVia};
use provider_session_machines::session_run_claims::RunClaim;
use std::cell::RefCell;

const SESSION: &str = "session-one";

/// One recorded publication: its label and the run it claimed.
type Recorded = RefCell<Vec<(&'static str, RunClaim)>>;

fn move_cell(cells: &SessionCellTable, event: SessionCellEvent) {
    let _moved = cells.transition(SESSION, event, EntryEffectRoute::Caller);
}

fn publish(
    cells: &SessionCellTable,
    recorded: &Recorded,
    label: &'static str,
    claim: RunClaim,
) -> GenerationPublication {
    cells
        .publish_if(SESSION, claim, &mut || {
            recorded.borrow_mut().push((label, claim));
            Ok(())
        })
        .expect("the pipe is open")
}

fn generation(number: u64) -> SessionGeneration {
    SessionGeneration::from_number(number)
}

fn close(cells: &SessionCellTable, attempt: u128) {
    let attempt = CloseAttempt(uuid::Uuid::from_u128(attempt));
    move_cell(cells, SessionCellEvent::CloseRequested { attempt });
    move_cell(cells, SessionCellEvent::CloseDone { attempt });
}

fn load(cells: &SessionCellTable, attempt: u128) {
    let attempt = LoadAttempt(uuid::Uuid::from_u128(attempt));
    move_cell(
        cells,
        SessionCellEvent::LoadRequested {
            via: RestoreVia::Load,
            attempt,
        },
    );
    move_cell(cells, SessionCellEvent::Loaded { attempt });
}

/// Oracle (the Lead's step 6 conditions, written by hand): run 1's events,
/// including those while it closes, all land before its Closed; a late
/// event of the ended run is dropped.
#[test]
fn a_runs_events_land_before_its_closed_and_late_ones_are_dropped() {
    // Arrange
    let cells = SessionCellTable::default();
    let recorded = Recorded::default();
    move_cell(&cells, SessionCellEvent::CreateAdmitted);

    // Act
    publish(&cells, &recorded, "first", RunClaim::Exact(generation(1)));
    let attempt = CloseAttempt(uuid::Uuid::from_u128(7));
    move_cell(&cells, SessionCellEvent::CloseRequested { attempt });
    publish(
        &cells,
        &recorded,
        "while closing",
        RunClaim::Exact(generation(1)),
    );
    move_cell(&cells, SessionCellEvent::CloseDone { attempt });
    let closed = publish(
        &cells,
        &recorded,
        "Closed",
        RunClaim::Terminal(generation(1)),
    );
    let late = publish(&cells, &recorded, "late", RunClaim::Exact(generation(1)));

    // Assert
    let labels: Vec<_> = recorded.borrow().iter().map(|(label, _)| *label).collect();
    assert_eq!(labels, ["first", "while closing", "Closed"]);
    assert_eq!(closed, GenerationPublication::Published);
    assert_eq!(late, GenerationPublication::Ended);
}

/// Oracle: an actor that exits on its own ends its run (S2): its Unloaded
/// passes as the run's terminal fact, and the ended run's late events are
/// dropped (M4; today's table let them through until a Closed).
#[test]
fn an_unloaded_run_passes_its_terminal_fact_and_drops_late_events() {
    // Arrange
    let cells = SessionCellTable::default();
    let recorded = Recorded::default();
    move_cell(&cells, SessionCellEvent::CreateAdmitted);

    // Act
    move_cell(
        &cells,
        SessionCellEvent::ActorExited {
            generation: generation(1),
        },
    );
    let unloaded = publish(
        &cells,
        &recorded,
        "Unloaded",
        RunClaim::Terminal(generation(1)),
    );
    let late = publish(&cells, &recorded, "late", RunClaim::Exact(generation(1)));
    let connection_wide = publish(&cells, &recorded, "auth", RunClaim::Current);

    // Assert
    assert_eq!(unloaded, GenerationPublication::Published);
    assert_eq!(late, GenerationPublication::Ended);
    assert_eq!(connection_wide, GenerationPublication::Ended);
}

/// Oracle: a terminal fact passes only for the run that ended last, never an
/// older one; a newer run's events aren't blocked by the old run's end.
#[test]
fn an_older_runs_terminal_fact_is_refused_and_a_newer_run_publishes() {
    // Arrange
    let cells = SessionCellTable::default();
    let recorded = Recorded::default();
    move_cell(&cells, SessionCellEvent::CreateAdmitted);
    close(&cells, 1);

    // Act
    load(&cells, 2);
    let newer = publish(&cells, &recorded, "run 2", RunClaim::Exact(generation(2)));
    let old_event = publish(&cells, &recorded, "run 1", RunClaim::Exact(generation(1)));
    close(&cells, 3);
    let older_closed = publish(
        &cells,
        &recorded,
        "Closed 1",
        RunClaim::Terminal(generation(1)),
    );
    let latest_closed = publish(
        &cells,
        &recorded,
        "Closed 2",
        RunClaim::Terminal(generation(2)),
    );

    // Assert
    assert_eq!(
        (newer, old_event, older_closed, latest_closed),
        (
            GenerationPublication::Published,
            GenerationPublication::Ended,
            GenerationPublication::Ended,
            GenerationPublication::Published,
        )
    );
}

/// Oracle: the step 6 rules over any history of the session's cell: nothing
/// claiming a run lands after that run's terminal fact; an exact claim lands
/// exactly while its run is loading, live or closing; a terminal fact lands
/// exactly when its run is the one that ended last.
#[hegel::test]
fn publications_follow_the_cell_over_any_history(case: hegel::TestCase) {
    use hegel::generators as gs;
    let cells = SessionCellTable::default();
    let recorded = Recorded::default();
    let mut attempts: u128 = 0;
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(40)) {
        attempts += 1;
        let load_attempt = LoadAttempt(uuid::Uuid::from_u128(attempts));
        let close_attempt = CloseAttempt(uuid::Uuid::from_u128(attempts));
        let claimed = generation(case.draw(gs::integers::<u64>().max_value(4)));
        let before = cells.read(SESSION, |cell| cell.lifecycle);
        match case.draw(gs::integers::<u8>().max_value(8)) {
            0 => move_cell(&cells, SessionCellEvent::CreateAdmitted),
            1 => load(&cells, attempts),
            2 => move_cell(
                &cells,
                SessionCellEvent::LoadRequested {
                    via: RestoreVia::Load,
                    attempt: load_attempt,
                },
            ),
            3 => close(&cells, attempts),
            4 => move_cell(
                &cells,
                SessionCellEvent::CloseRequested {
                    attempt: close_attempt,
                },
            ),
            5 => move_cell(
                &cells,
                SessionCellEvent::ActorExited {
                    generation: cells.current_generation(SESSION),
                },
            ),
            6 => {
                let landed = publish(&cells, &recorded, "event", RunClaim::Exact(claimed));
                let live_run = match before {
                    SessionLifecycle::Loading { generation, .. }
                    | SessionLifecycle::Live { generation }
                    | SessionLifecycle::Closing { generation, .. } => Some(generation),
                    SessionLifecycle::NotLoaded { .. } => None,
                };
                assert_eq!(
                    landed == GenerationPublication::Published,
                    live_run == Some(claimed),
                    "{before:?} {claimed:?}"
                );
            }
            7 => {
                let landed = publish(&cells, &recorded, "terminal", RunClaim::Terminal(claimed));
                assert_eq!(
                    landed == GenerationPublication::Published,
                    before == SessionLifecycle::NotLoaded { last: claimed },
                    "{before:?} {claimed:?}"
                );
            }
            _ => {
                publish(&cells, &recorded, "connection-wide", RunClaim::Current);
            }
        }
    }
    let log = recorded.borrow();
    for (index, (_, claim)) in log.iter().enumerate() {
        if let RunClaim::Terminal(ended) = claim {
            assert!(
                !log[index + 1..]
                    .iter()
                    .any(|(_, later)| *later == RunClaim::Exact(*ended)),
                "run {ended:?} published after its terminal fact: {log:?}"
            );
        }
    }
}
