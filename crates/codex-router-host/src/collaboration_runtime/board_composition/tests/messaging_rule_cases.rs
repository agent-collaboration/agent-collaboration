//! Real owner takeover and acknowledged prefixes decide handoff recovery.
#![deny(clippy::too_many_lines)]

#[path = "messaging_cancel_cases.rs"]
mod cancel_cases;
#[path = "messaging_local_cancel_case.rs"]
mod local_cancel_case;
#[path = "messaging_rule_fixture.rs"]
mod rule_fixture;
#[path = "messaging_settlement_test.rs"]
mod settlement;
