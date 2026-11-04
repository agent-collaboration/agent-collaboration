//! One namespace per original central runtime test root.

#[cfg(feature = "conformance")]
#[path = "../support/fleet_ledger_rows.rs"]
#[expect(
    dead_code,
    reason = "This aggregate only needs the connection snapshot; integration also uses the borrowed scope."
)]
mod fleet_ledger_rows;

#[cfg(feature = "conformance")]
#[path = "../native_http/support.rs"]
mod configuration;
#[cfg(feature = "conformance")]
#[path = "../support/creation_scenarios.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture capabilities serve different test aggregates."
)]
mod creation_scenarios;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_deadline_boundaries.rs"]
mod forwarded_post_deadline_boundaries;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_lifecycle_oracles.rs"]
mod forwarded_post_lifecycle_oracles;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_native_cases.rs"]
mod forwarded_post_native_cases;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_oracles.rs"]
mod forwarded_post_oracles;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_route_overlap.rs"]
mod forwarded_post_route_overlap;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_scenarios.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture capabilities serve different test aggregates."
)]
mod forwarded_post_scenarios;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_state_cases.rs"]
mod forwarded_post_state_cases;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_state_oracles.rs"]
mod forwarded_post_state_oracles;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_terminal_slots.rs"]
mod forwarded_post_terminal_slots;
#[cfg(feature = "conformance")]
#[path = "../support/forwarded_post_wire.rs"]
mod forwarded_post_wire;
#[cfg(feature = "conformance")]
#[path = "../support/holder_writer_retry.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture capabilities serve different test aggregates."
)]
mod holder_writer_retry;
#[cfg(feature = "conformance")]
#[path = "../support/mail_relay_transport.rs"]
mod mail_relay_transport;
#[cfg(feature = "conformance")]
#[path = "../mail_submit_http/registry_setup.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture capabilities serve different test aggregates."
)]
mod registry_setup;
#[cfg(feature = "conformance")]
#[path = "../session_publication/support.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture capabilities serve different test aggregates."
)]
mod session_publication_fixture;

mod fleet_client_cache;
mod forwarded_posts;
mod forwarding_real_timer;
mod mailbox_rules;

#[cfg(feature = "conformance")]
#[path = "../support/mail_push_inbox.rs"]
mod mail_push_inbox;

#[cfg(feature = "conformance")]
#[path = "../support/mail_push_transport.rs"]
mod mail_push_transport;
