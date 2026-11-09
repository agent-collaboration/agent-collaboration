#![allow(
    clippy::indexing_slicing,
    reason = "moved consumer assertions inspect fixed fixture rows and JSON cells; their expressions are preserved"
)]
#![allow(clippy::panic, clippy::panic_in_result_fn)]

mod legacy_delivery;
mod migration_stack;

#[cfg(all(feature = "test-support", feature = "test-fault"))]
mod claim_gate;
#[cfg(feature = "test-support")]
mod consumer_checked_queries;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
#[allow(dead_code, unused_imports)]
#[path = "../support/consumer/lib.rs"]
mod consumer_fixture;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
mod engine_keys;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
mod native_bootstrap_prefix_keys;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
mod native_client_readiness;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
mod native_receiver_foreign_keys;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
mod native_registry_settlement;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
mod native_rename_adoption;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
mod native_upgrade_refusal;
#[cfg(all(feature = "test-support", feature = "test-fault"))]
#[allow(dead_code, unused_imports)]
#[path = "../support/process/mod.rs"]
mod support;
