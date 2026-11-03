//! Fleet mutations and owed notices commit together; only alarms perform delivery.
#![cfg(feature = "conformance")]

#[path = "fleet_notice_outbox/archive_notices.rs"]
mod archive_notices;
#[path = "fleet_notice_outbox/reader_progress.rs"]
mod reader_progress;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "session_publication/support.rs"]
mod session_fixture;
#[path = "fleet_notice_outbox/support.rs"]
mod support;
