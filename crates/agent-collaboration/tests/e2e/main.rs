//! Original test roots retain their case namespaces.

#[path = "../board_e2e/central_pair.rs"]
mod central_pair;
#[path = "../board_e2e/fixture.rs"]
mod fixture;
#[path = "../board_e2e/fixture_inputs.rs"]
mod fixture_inputs;
#[path = "../board_e2e/follow_fixture.rs"]
mod follow_fixture;
#[path = "../support/local_mail_native_sql.rs"]
#[expect(
    dead_code,
    reason = "The shared SQL fixture also serves integration cases with additional operations."
)]
mod native_sql;
#[path = "../board_e2e/owned_host.rs"]
mod owned_host;
#[path = "../board_e2e/records_reopen.rs"]
mod records_reopen;
#[path = "../board_e2e/takeover_readbacks.rs"]
mod takeover_readbacks;

mod board_central_e2e;
mod board_records_e2e;
mod board_takeover_e2e;
mod installation_directory_e2e;
mod local_mail_e2e;
mod mail_reconcile_e2e;
mod mail_retirement_e2e;
mod messaging_rules_e2e;
