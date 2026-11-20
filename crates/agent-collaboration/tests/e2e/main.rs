//! Original test roots retain their case namespaces.

#[path = "../board_e2e/acp_recipient.rs"]
mod acp_recipient;
#[path = "../support/local_mail_native_sql.rs"]
#[expect(
    dead_code,
    reason = "The shared SQL fixture also serves integration cases with additional operations."
)]
mod native_sql;
#[path = "../board_e2e/owned_host.rs"]
mod owned_host;

mod local_mail_e2e;

#[path = "../board_e2e/messaging_stopped_owner.rs"]
mod messaging_stopped_owner;
