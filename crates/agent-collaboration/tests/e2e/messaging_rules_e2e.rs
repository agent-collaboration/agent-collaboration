//! B+ process observations compose the real Router entry points with local central.
#![deny(clippy::too_many_lines)]
use crate::{central_pair, fixture, messaging_stopped_owner, owned_host, records_reopen};
#[path = "../board_e2e/messaging_capacity.rs"]
mod messaging_capacity;
#[path = "../board_e2e/messaging_expiry.rs"]
mod messaging_expiry;
#[path = "../board_e2e/messaging_fixture.rs"]
mod messaging_fixture;
#[path = "../board_e2e/messaging_recovery.rs"]
mod messaging_recovery;
#[path = "../board_e2e/messaging_replay.rs"]
mod messaging_replay;
#[path = "../board_e2e/messaging_surface.rs"]
mod messaging_surface;
#[path = "../board_e2e/messaging_uncertainty.rs"]
mod messaging_uncertainty;

use messaging_fixture::mail_scenario;
use router_peer_test_support::FixtureResult;

#[tokio::test]
async fn valid_until_expiry_is_visible_to_the_sender() -> FixtureResult {
    mail_scenario(messaging_expiry::sender_sees_expiry).await
}

#[tokio::test]
async fn full_inbox_retries_frozen_items_after_recipient_capacity_is_restored() -> FixtureResult {
    mail_scenario(messaging_capacity::full_inbox).await
}

#[tokio::test]
async fn same_item_identity_with_changed_content_is_a_conflict() -> FixtureResult {
    mail_scenario(messaging_replay::changed_content_conflicts).await
}

#[tokio::test]
async fn mailbox_hop_duplicate_retry_keeps_one_recipient_effect() -> FixtureResult {
    mail_scenario(messaging_replay::mailbox_retry_keeps_one_effect).await
}

#[tokio::test]
async fn handoff_commit_before_outbox_write_recovers_the_same_item() -> FixtureResult {
    mail_scenario(messaging_recovery::committed_handoff_before_outbox).await
}

#[tokio::test]
async fn lost_carrier_receipt_stays_uncertain() -> FixtureResult {
    mail_scenario(messaging_uncertainty::carrier_receipt_is_lost).await
}
