//! Original test roots retain their case namespaces.

#[path = "../upload_support/central_fixture.rs"]
mod central_fixture;
#[path = "../message_support/mod.rs"]
mod message_support;
#[path = "../support/mod.rs"]
mod support;
#[path = "../update_support/mod.rs"]
mod update_support;
#[path = "../upload_support/mod.rs"]
mod upload_support;

mod activity_range_cutover;
mod attribution_refusals;
mod command_refusals;
mod expiry_origin_cutover;
mod fleet_bundle_limits;
mod fleet_owner_routes;
mod historical_text;
mod import_digest_identity;
mod import_http_failures;
mod import_page_refusals;
mod import_scaling;
mod imported_claims;
mod interval_and_membership_fidelity;
mod legacy_transform;
mod legacy_unknown_roles;
mod message_cutover;
mod message_cutover_cli;
mod message_cutover_crash;
mod message_report_guard;
mod native_inbox_fidelity;
mod project_description;
mod seed_integrity_refusal;
mod source_schema_refusals;
mod update_ledger_journey;
mod update_moves_journey;
mod update_reset_and_dead_files;
mod update_source_checks;
mod upgrade_singleton;
mod upload_publication;
mod watch_row_fidelity;
