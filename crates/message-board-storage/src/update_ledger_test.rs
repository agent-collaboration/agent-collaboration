use super::{
    StoredLedgerRow, UpdateLedgerEntry, UpdateSourceProgress, UpdateStepNumber, UpdateStepProgress,
    decode_entry,
};
use crate::RouterLocalStorageError;

fn row(step: i64, source: &str, state: &str) -> StoredLedgerRow {
    StoredLedgerRow {
        step,
        source: source.to_owned(),
        state: state.to_owned(),
    }
}

fn refused_field(result: Result<UpdateLedgerEntry, RouterLocalStorageError>) -> &'static str {
    match result {
        Err(RouterLocalStorageError::InvalidRecordKey { field }) => field,
        other => panic!("expected a refusal naming the field, got {other:?}"),
    }
}

#[test]
fn step_and_source_rows_decode_to_their_own_progress_kinds() {
    let step = UpdateStepNumber::new(5);

    let done = decode_entry(row(5, "", "done")).expect("a done step row decodes");
    let written = decode_entry(row(5, "fleet-outbox.sqlite", "written"))
        .expect("a written source row decodes");

    assert_eq!(
        done,
        UpdateLedgerEntry::Step {
            step,
            progress: UpdateStepProgress::Done
        }
    );
    assert_eq!(
        written,
        UpdateLedgerEntry::Source {
            step,
            source: "fleet-outbox.sqlite".to_owned(),
            progress: UpdateSourceProgress::Written
        }
    );
}

#[test]
fn a_state_belonging_to_the_other_row_kind_is_refused_by_name() {
    // A step can't be "written", and a source can't be "done": coercing either would
    // let a half-moved source read as a finished step.
    let step_marked_written = decode_entry(row(5, "", "written"));
    let source_marked_done = decode_entry(row(5, "fleet-outbox.sqlite", "done"));

    assert_eq!(refused_field(step_marked_written), "update_steps.state");
    assert_eq!(refused_field(source_marked_done), "update_steps.state");
}

#[test]
fn unknown_states_and_impossible_step_numbers_are_refused_by_name() {
    let unknown_state = decode_entry(row(1, "", "renaming"));
    let zero_step = decode_entry(row(0, "", "done"));
    let oversized_step = decode_entry(row(70_000, "", "done"));

    assert_eq!(refused_field(unknown_state), "update_steps.state");
    assert_eq!(refused_field(zero_step), "update_steps.step");
    assert_eq!(refused_field(oversized_step), "update_steps.step");
}
