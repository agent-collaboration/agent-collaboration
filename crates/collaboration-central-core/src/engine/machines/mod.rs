pub(crate) mod direct_message;
pub(crate) mod direct_message_store;
#[cfg(test)]
mod lifecycle_finish;
#[cfg_attr(
    not(any(test, feature = "test-support")),
    expect(
        dead_code,
        reason = "only the test-support adapter drives this machine until deliverable 4 wires the link"
    )
)]
pub(crate) mod owed_reply;
pub(crate) mod system_message_expiry;

mod message_effects;
