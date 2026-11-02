//! Focused producer/consumer checks for each supported effectful seed, separate from native proof.
use super::*;
#[path = "account_contract_test.rs"]
mod accounts;
#[path = "controller_receipt_contract_test.rs"]
mod controller_receipt;
#[path = "refresh_contract_test.rs"]
mod refresh;
fn events() -> (tempfile::TempDir, FixtureEvents) {
    let output = tempfile::tempdir().unwrap();
    let events = FixtureEvents::new(absent_file(&output.path().join("events.jsonl")).unwrap());
    (output, events)
}

#[path = "login_parent_exit_contract_test.rs"]
mod login_parent_exit;

#[path = "grouping_reopen_contract_test.rs"]
mod grouping_reopen;

#[path = "preference_write_contract_test.rs"]
mod preference_write;

#[path = "event_fence_contract_test.rs"]
mod event_fence;
