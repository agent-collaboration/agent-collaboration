mod alarm_decision;
mod alarm_source;
#[cfg(feature = "test-support")]
pub(crate) mod commit_fault;
pub use alarm_source::{AlarmNotPublished, AlarmResult, EventCompletion};
pub(crate) mod admin;
pub(crate) mod attempt_records;
pub(crate) mod attempt_statements;
pub(crate) mod attempt_store;
pub(crate) mod control_gate;
mod due_batch_state;
pub(crate) mod effects;
pub(crate) mod failures;
pub(crate) mod handle;
pub(crate) mod legacy_units;
#[cfg(test)]
pub(crate) mod machine_property_support;
pub(crate) mod machines;
#[cfg(feature = "test-support")]
pub(crate) mod owed_reply_store;
pub(crate) mod port;
#[cfg(feature = "test-support")]
pub(crate) mod probe_machine;
#[cfg(feature = "test-support")]
pub(crate) mod probe_store;
pub(crate) mod query_registry;
#[cfg_attr(
    not(feature = "test-support"),
    expect(
        dead_code,
        reason = "only the test machine's requests use the engine request entry in deliverable 1 (P7); wakes move onto it in deliverable 2"
    )
)]
pub(crate) mod receipts;
#[cfg_attr(
    not(feature = "test-support"),
    expect(
        dead_code,
        reason = "only the test machine's requests use the engine request entry in deliverable 1 (P7); wakes move onto it in deliverable 2"
    )
)]
pub(crate) mod requests;
pub(crate) mod source;
pub(crate) mod store_adapter;

pub use alarm_decision::engine_alarm_decider;
#[cfg(feature = "test-support")]
pub use due_batch_state::DueUnitStart;
pub use due_batch_state::{DueBatchSnapshot, DueBatchState};
