//! One namespace per original central runtime test root.

#[cfg(feature = "conformance")]
#[path = "../support/fleet_record_rows.rs"]
mod fleet_record_rows;

#[cfg(feature = "conformance")]
#[path = "../support/clocked_server.rs"]
mod clocked_server;
#[path = "../native_http/support.rs"]
mod configuration;
#[cfg(feature = "conformance")]
#[path = "../support/copy_read_scenarios.rs"]
mod copy_read_scenarios;
#[path = "../copy_receiver/mod.rs"]
mod copy_receiver_fixture;
#[cfg(feature = "conformance")]
#[path = "../support/copy_wait_scenarios.rs"]
mod copy_wait_scenarios;
#[cfg(feature = "conformance")]
#[path = "../support/creation_scenarios.rs"]
mod creation_scenarios;
#[cfg(feature = "conformance")]
#[path = "../directory_session/support.rs"]
mod directory_session_fixture;
#[cfg(feature = "conformance")]
#[path = "../support/fleet_cache_source.rs"]
mod fleet_cache_source;
#[cfg(feature = "conformance")]
#[path = "../support/holder_writer_retry.rs"]
mod holder_writer_retry;
#[cfg(feature = "conformance")]
#[path = "../support/imported_board_fixture.rs"]
mod imported_board_fixture;
#[cfg(feature = "conformance")]
#[path = "../support/imported_claim_scenarios.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture capabilities serve different test aggregates."
)]
mod imported_claim_scenarios;
#[cfg(feature = "conformance")]
#[path = "../../src/test_support/mail_relay_transport.rs"]
mod mail_relay_transport;
#[cfg(feature = "conformance")]
#[path = "../fleet_storage/support.rs"]
mod native_fleet;
#[path = "../native_http/platform.rs"]
mod native_http_platform;
#[cfg(feature = "conformance")]
#[path = "../support/owner_registry_scenarios.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture capabilities serve different test aggregates."
)]
mod owner_registry_scenarios;
#[cfg(feature = "conformance")]
#[path = "../mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[cfg(feature = "conformance")]
#[path = "../session_move/fixture.rs"]
mod session_move_fixture;
#[cfg(feature = "conformance")]
#[path = "../session_publication/support.rs"]
mod session_publication_fixture;
#[cfg(feature = "conformance")]
#[path = "../transfer_boundaries/fixture.rs"]
mod transfer_fixture;

mod access_carryover;
mod access_workflow;
mod admin_import;
mod admin_import_columns;
mod admin_import_reservation;
mod archive_reports;
mod archived_claim;
mod binding_attachment;
mod copy_owner_health;
mod copy_reads;
mod copy_receiver;
mod copy_receiver_session;
mod copy_rollback_reopen;
mod copy_staged_alarm;
mod copy_staged_expiry;
mod copy_suite;
mod copy_wait;
mod delivery_config;
mod directory_session;
mod fleet_cache_projection;
mod fleet_cache_records;
mod fleet_conflict;
mod fleet_creation_gap;
mod fleet_effects;
mod fleet_mail_observer;
mod fleet_metadata_records;
mod fleet_notice_outbox;
mod fleet_notices_http;
mod fleet_requests;
mod fleet_retention;
mod fleet_snapshot_merge;
mod fleet_storage;
mod holder_delivery;
mod installation_peer_directory;
mod machine_enrollment;
mod machine_notices;
mod mail_status_http;
mod mail_submit_http;
mod native_access;
mod native_access_logging;
mod native_http;
mod native_server_reopen;
mod normal_redeploy;
mod numeric_composed;
mod object_bootstrap;
mod object_identity;
mod owner_admin_sessions;
mod owner_fence_ordering;
mod owner_registry;
mod owner_takeover_recovery;
mod process_restart;
mod registration_gates;
mod revocation_carryover;
mod runtime_controls;
mod self_hosted_access;
mod session_carryover;
mod session_credentials;
mod session_inflight_replacement;
mod session_lost_row;
mod session_move;
mod session_publication;
mod session_renewal_history;
mod session_route_gaps;
mod shared_host_calls;
mod startup_controls;
mod step_transactions;
mod transaction_reopen;
mod transfer_floor_repair;
mod transfer_ordered_rows;
mod transfer_schema_support;

#[cfg(feature = "conformance")]
mod fleet_layout;

#[cfg(feature = "conformance")]
#[path = "../fleet_notice_outbox/fault_store.rs"]
mod fleet_fault_store;
#[cfg(feature = "conformance")]
mod fleet_lease_atomicity;

#[cfg(feature = "conformance")]
mod composed_fleet_fixture;

#[cfg(feature = "conformance")]
mod fleet_alarm_sql;

#[cfg(feature = "conformance")]
mod fleet_archive_columns;

#[cfg(feature = "conformance")]
mod fleet_archive_notices;

#[cfg(feature = "conformance")]
mod fleet_archive_settlement;

#[cfg(feature = "conformance")]
mod fleet_archive_snapshot;

#[cfg(feature = "conformance")]
mod fleet_claim_adapter;

#[cfg(feature = "conformance")]
mod fleet_claim_adapter_fixture;

#[cfg(feature = "conformance")]
mod fleet_claim_columns;

#[cfg(feature = "conformance")]
mod fleet_claim_receiver;

#[cfg(feature = "conformance")]
mod fleet_claim_restart;

#[cfg(feature = "conformance")]
mod fleet_claim_settlement;

#[cfg(feature = "conformance")]
mod fleet_creation_columns;

#[cfg(feature = "conformance")]
mod fleet_directory_version;

#[cfg(feature = "conformance")]
mod fleet_feed_notices;

#[cfg(feature = "conformance")]
mod fleet_feed_timestamps;

#[cfg(feature = "conformance")]
mod fleet_mail_admission;

#[cfg(feature = "conformance")]
mod fleet_mail_backpressure;

#[cfg(feature = "conformance")]
mod fleet_mail_dispatch;

#[cfg(feature = "conformance")]
mod fleet_mail_expiry;

#[cfg(feature = "conformance")]
mod fleet_mail_inbox_fixture;

#[cfg(feature = "conformance")]
mod fleet_mail_query;

#[cfg(feature = "conformance")]
mod fleet_mail_readmission;

#[cfg(feature = "conformance")]
mod fleet_mail_receipt_loss;

#[cfg(feature = "conformance")]
mod fleet_mail_reports;

#[cfg(feature = "conformance")]
mod fleet_mail_revocation;

#[cfg(feature = "conformance")]
mod fleet_mail_selector;

#[cfg(feature = "conformance")]
mod fleet_mail_settle;

#[cfg(feature = "conformance")]
mod fleet_mail_status_wire;

#[cfg(feature = "conformance")]
mod fleet_ownership_restart;

#[cfg(feature = "conformance")]
mod fleet_project_settle;

#[cfg(feature = "conformance")]
mod fleet_project_status;

#[cfg(feature = "conformance")]
mod fleet_status_stream;

#[cfg(feature = "conformance")]
mod typed_claim_observer;

#[cfg(feature = "conformance")]
#[expect(
    dead_code,
    reason = "Shared recipient helpers serve targets with different capability subsets."
)]
#[path = "../support/mail_push_inbox.rs"]
mod mail_push_inbox;

#[cfg(feature = "conformance")]
#[expect(
    dead_code,
    reason = "Shared recipient helpers serve targets with different capability subsets."
)]
#[path = "../support/mail_push_transport.rs"]
mod mail_push_transport;

#[path = "../support/fleet_ledger_rows.rs"]
#[cfg(feature = "conformance")]
mod fleet_ledger_rows;
#[cfg(feature = "conformance")]
mod revocation_clock;
#[cfg(feature = "conformance")]
mod revocation_writer_recovery;

#[cfg(test)]
#[cfg(feature = "test-support")]
mod central_object_batches;
#[cfg(test)]
#[cfg(feature = "test-support")]
mod request_pause_seam;
#[cfg(test)]
#[cfg(feature = "test-support")]
mod storage_support;
#[cfg(test)]
#[cfg(feature = "test-support")]
mod workspace_store_calls;
