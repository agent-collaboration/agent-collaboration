//! The actual project Outbox entry preserves historical authority on replay.
use super::*;
use collaboration_mail_protocol::MailItemId;
use fleet_sync_protocol::ItemId;

#[path = "project_mail_cancel_admission_tests.rs"]
mod cancellation;
#[path = "project_mail_crash_tests.rs"]
mod crash;

fn mail_id(item: ItemId) -> MailItemId {
    item.as_uuid().to_string().try_into().unwrap()
}
