pub(crate) mod admin;
mod alarm_source;
pub(crate) mod attempt_records;
pub(crate) mod attempt_statements;
pub(crate) mod attempt_store;
pub(crate) mod control_gate;
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
