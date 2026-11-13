//! Original test roots retain their case namespaces.

#[path = "../upload_support/central_fixture.rs"]
mod central_fixture;
#[path = "../update_support/mod.rs"]
mod update_support;

mod command_refusals;
mod fleet_owner_routes;
mod update_completed_step_sources;
mod update_ledger_journey;
mod update_moves_journey;
mod update_proxy_store_journey;
mod update_reset_and_dead_files;
mod update_settings_journey;
mod update_source_checks;
mod update_whole_journey;
