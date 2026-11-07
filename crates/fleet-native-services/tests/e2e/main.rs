#![allow(
    clippy::indexing_slicing,
    reason = "moved consumer assertions inspect fixed fixture rows and JSON cells; their expressions are preserved"
)]
#![allow(clippy::panic, clippy::panic_in_result_fn)]
#[cfg(feature = "test-support")]
#[allow(dead_code, unused_imports)]
#[path = "../support/consumer/lib.rs"]
mod consumer_fixture;
mod delivery_concurrent_recovery;
mod delivery_containment;
mod delivery_expiry;
mod delivery_handshake;
mod delivery_journey;
mod delivery_relay_restart;
mod delivery_service;
mod delivery_status_owner;
mod delivery_storage_retry;
#[allow(dead_code, unused_imports)]
#[path = "../support/migration_rename/mod.rs"]
mod migration_rename;
mod native_capture_restart;
mod native_client_readiness;
mod native_column_migrations;
mod native_copy_claim_retry;
mod native_copy_containment;
mod native_copy_discard;
mod native_engine_identities;
mod native_frame_shapes;
mod native_idle_successor_move;
mod native_lease_transitions;
mod native_lifecycle;
mod native_migration_rename;
mod native_move_aside;
mod native_move_copy_retry;
mod native_move_link;
mod native_reauthorization;
mod native_registration;
mod native_rename_adoption;
mod native_rename_bootstrap;
mod native_retention;
mod native_schema_admission;
mod native_schema_publication;
mod native_seed_copy_retry;
mod native_service;
mod native_snapshot;
mod native_target_capability;
mod native_writer_bootstrap;
mod native_writer_cancel;
mod native_writer_move;
mod settle_claims;
mod status_acknowledgement;
#[cfg(feature = "test-support")]
#[allow(dead_code, unused_imports)]
#[path = "../support/process/mod.rs"]
mod support;
