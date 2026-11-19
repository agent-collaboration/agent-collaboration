//! Run-owned state answers and accepts only the session's current run
//! (design note §11.3; Lead V3).

use super::{RunOwnedMap, RunWrite};
use crate::session_cell_bookkeeping::{admit_create, end_run_and_load_next_for_test};
use crate::session_cell_table::{EntryEffectRoute, SessionCellTable};
use provider_session_machines::session_cell_machine::SessionCellEvent;
use std::sync::Arc;

const SESSION: &str = "session-a";

/// Oracle: Lead V3, "a write from an older run is refused": an ended run's
/// write after a newer run has written changes nothing, and the newer run's
/// value survives.
#[test]
fn an_older_runs_write_after_a_newer_run_wrote_changes_nothing() {
    // Arrange
    let cells = Arc::new(SessionCellTable::default());
    let state = RunOwnedMap::new(Arc::clone(&cells));
    let first = admit_create(&cells, SESSION).expect("created");
    let second = end_run_and_load_next_for_test(&cells, SESSION, first);
    let newer = state.write(SESSION, second, "newer");

    // Act
    let older = state.write(SESSION, first, "older");

    // Assert
    assert_eq!(newer, RunWrite::Written);
    assert_eq!(older, RunWrite::StaleRun);
    assert_eq!(state.read(SESSION), Some("newer"));
}

/// Oracle: Lead V3, the held continuation: an ended run's write that lands
/// before the newer run has written anything is still never read as the
/// newer run's state.
#[test]
fn an_older_runs_late_write_is_never_read_as_the_newer_runs() {
    // Arrange
    let cells = Arc::new(SessionCellTable::default());
    let state = RunOwnedMap::new(Arc::clone(&cells));
    let first = admit_create(&cells, SESSION).expect("created");
    let _second = end_run_and_load_next_for_test(&cells, SESSION, first);

    // Act
    let late = state.write(SESSION, first, "older");
    let read = state.read(SESSION);

    // Assert
    assert_eq!(late, RunWrite::StaleRun);
    assert_eq!(read, None);
}

/// Oracle: design note §11.3: a run reads its own state, a cleared entry
/// reads as absent, and once the run ends nothing of it is read.
#[test]
fn a_run_reads_its_own_state_until_it_ends() {
    // Arrange
    let cells = Arc::new(SessionCellTable::default());
    let state = RunOwnedMap::new(Arc::clone(&cells));
    let first = admit_create(&cells, SESSION).expect("created");

    // Act
    let written = state.write(SESSION, first, 1_u32);
    let read_live = state.read(SESSION);
    let changed = state.write_with(SESSION, first, |entry| {
        if let Some(value) = entry {
            *value += 1;
        }
    });
    let read_changed = state.read(SESSION);
    let cleared = state.clear(SESSION, first);
    let read_cleared = state.read(SESSION);
    let _rewritten = state.write(SESSION, first, 5);
    cells
        .transition(
            SESSION,
            SessionCellEvent::ActorExited { generation: first },
            EntryEffectRoute::Sink,
        )
        .expect("the run ends");
    let read_after_end = state.read(SESSION);
    state.end_run(SESSION, first);

    // Assert
    assert_eq!(written, RunWrite::Written);
    assert_eq!(read_live, Some(1));
    assert_eq!(changed, RunWrite::Written);
    assert_eq!(read_changed, Some(2));
    assert_eq!(cleared, RunWrite::Written);
    assert_eq!(read_cleared, None);
    assert_eq!(read_after_end, None);
}

/// Oracle: the connection-wide case: the agent's auth status reaches every
/// session's report, whatever its run.
#[test]
fn a_connection_wide_change_reaches_every_entry() {
    // Arrange
    let cells = Arc::new(SessionCellTable::default());
    let state = RunOwnedMap::new(Arc::clone(&cells));
    let first = admit_create(&cells, SESSION).expect("created");
    let _written = state.write(SESSION, first, 1_u32);

    // Act
    state.update_every(|_session_id, value| *value = 7);

    // Assert
    assert_eq!(state.read(SESSION), Some(7));
}

/// Oracle: review F5 and §17.1 ("a write from a non-current run changes
/// nothing"; wrong build: the writer checks its run under the cell lock but
/// writes the map after releasing it): while a writer changes the map, its
/// run can't end, so the end that moves the cell can never fall between the
/// writer's check and its write; once the write is done the run ends and its
/// entry goes.
#[test]
fn a_runs_end_never_falls_between_a_writers_check_and_its_write() {
    // Arrange
    let cells = Arc::new(SessionCellTable::default());
    let state = RunOwnedMap::new(Arc::clone(&cells));
    let first = admit_create(&cells, SESSION).expect("created");
    let mut ended_during_write = false;

    // Act: the run's end tries to land while the write is in progress.
    let wrote = state.write_with(SESSION, first, |entry| {
        ended_during_write = matches!(
            cells.try_transition_for_test(
                SESSION,
                SessionCellEvent::ActorExited { generation: first },
            ),
            Some(Ok(_))
        );
        *entry = Some("late");
    });
    if !ended_during_write {
        cells
            .transition(
                SESSION,
                SessionCellEvent::ActorExited { generation: first },
                EntryEffectRoute::Sink,
            )
            .expect("the run ends once the write is done");
    }
    state.end_run(SESSION, first);

    // Assert
    assert!(
        !ended_during_write,
        "the run ended between the writer's check and its write"
    );
    assert_eq!(wrote, RunWrite::Written);
    assert_eq!(
        state.read_in(SESSION, first),
        None,
        "the ended run's entry stayed"
    );
}
