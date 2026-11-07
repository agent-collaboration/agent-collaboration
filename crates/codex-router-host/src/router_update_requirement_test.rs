use super::{RequiredRouterUpdate, RouterUpdateRequirement, requirement_for_ledger};
use message_board_storage::{
    StoredUpdateLedger, UpdateLedgerEntry, UpdateSourceProgress, UpdateStepNumber,
    UpdateStepProgress,
};
use std::path::PathBuf;

fn store() -> PathBuf {
    PathBuf::from("/root/agent-communication/fleet-cache.sqlite")
}

fn step_entry(number: u16, progress: UpdateStepProgress) -> UpdateLedgerEntry {
    UpdateLedgerEntry::Step {
        step: UpdateStepNumber::new(number),
        progress,
    }
}

fn unfinished(numbers: &[u16]) -> RouterUpdateRequirement {
    RouterUpdateRequirement::Required(RequiredRouterUpdate::UnfinishedSteps {
        store: store(),
        steps: numbers.iter().copied().map(UpdateStepNumber::new).collect(),
    })
}

#[test]
fn a_root_whose_host_never_opened_the_fleet_store_needs_no_update() {
    let never_opened = requirement_for_ledger(store(), &StoredUpdateLedger::MissingDatabase);

    assert_eq!(never_opened, RouterUpdateRequirement::NotRequired);
}

#[test]
fn an_empty_ledger_means_an_interrupted_update() {
    // A store the Host creates records every step done, so an empty ledger is an
    // update (or a Host's first open) that stopped before recording anything.
    let empty = requirement_for_ledger(store(), &StoredUpdateLedger::Present(vec![]));

    assert_eq!(empty, unfinished(&[1, 5, 10]));
}

#[test]
fn a_store_from_an_older_router_requires_the_update() {
    let requirement = requirement_for_ledger(store(), &StoredUpdateLedger::WithoutLedger);

    assert_eq!(
        requirement,
        RouterUpdateRequirement::Required(RequiredRouterUpdate::StoreWithoutLedger {
            store: store()
        })
    );
}

#[test]
fn a_ledger_with_every_known_step_done_needs_no_update() {
    let ledger = StoredUpdateLedger::Present(vec![
        step_entry(1, UpdateStepProgress::Done),
        step_entry(5, UpdateStepProgress::Done),
        step_entry(10, UpdateStepProgress::Done),
    ]);

    assert_eq!(
        requirement_for_ledger(store(), &ledger),
        RouterUpdateRequirement::NotRequired
    );
}

#[test]
fn a_started_update_missing_a_known_step_requires_the_update() {
    // The ledger has a row, so the update ran here, but no step is done.
    let in_progress =
        StoredUpdateLedger::Present(vec![step_entry(1, UpdateStepProgress::InProgress)]);
    let only_a_source = StoredUpdateLedger::Present(vec![UpdateLedgerEntry::Source {
        step: UpdateStepNumber::new(5),
        source: "fleet-outbox.sqlite".to_owned(),
        progress: UpdateSourceProgress::Written,
    }]);

    assert_eq!(
        requirement_for_ledger(store(), &in_progress),
        unfinished(&[1, 5, 10])
    );
    assert_eq!(
        requirement_for_ledger(store(), &only_a_source),
        unfinished(&[1, 5, 10])
    );
}

#[test]
fn a_step_left_in_progress_counts_even_when_this_router_does_not_know_it() {
    let ledger = StoredUpdateLedger::Present(vec![
        step_entry(1, UpdateStepProgress::Done),
        step_entry(5, UpdateStepProgress::Done),
        step_entry(10, UpdateStepProgress::Done),
        step_entry(42, UpdateStepProgress::InProgress),
    ]);

    assert_eq!(requirement_for_ledger(store(), &ledger), unfinished(&[42]));
}
