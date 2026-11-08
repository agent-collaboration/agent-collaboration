//! Fleet mutations and owed notices commit together; only alarms perform delivery.
#![cfg(feature = "conformance")]

#[path = "fleet_notice_outbox/reader_progress.rs"]
mod reader_progress;
use crate::session_publication_fixture as session_fixture;
#[path = "fleet_notice_outbox/support.rs"]
mod support;
