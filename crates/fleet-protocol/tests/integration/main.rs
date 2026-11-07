#![allow(clippy::unwrap_used)]
mod push_wire;

mod delivery_wire_contracts;

mod wire_contract;

#[path = "../copy_certainty.rs"]
mod copy_certainty;
#[path = "../copy_gate_wire.rs"]
mod copy_gate_wire;
#[path = "../copy_migration_retry.rs"]
mod copy_migration_retry;
#[path = "../crate_call_wire.rs"]
mod crate_call_wire;
#[path = "../delivery_digest_contracts.rs"]
mod delivery_digest_contracts;
#[path = "../holder_change_reason.rs"]
mod holder_change_reason;
#[path = "../holder_change_wire.rs"]
mod holder_change_wire;
#[path = "../lifecycle_wire.rs"]
mod lifecycle_wire;
#[path = "../parsed_schema_admission.rs"]
mod parsed_schema_admission;
#[path = "../real_frame_values.rs"]
mod real_frame_values;
#[path = "../recipient_refusals.rs"]
mod recipient_refusals;
