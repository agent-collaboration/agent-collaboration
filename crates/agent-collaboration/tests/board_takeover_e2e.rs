//! OB4 rows 5/6/7/13 through compiled Hosts, real Copy ACKs and recipient sockets.

#![deny(clippy::too_many_lines)]
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/central_pair.rs"]
mod central_pair;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/fixture.rs"]
mod fixture;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/fixture_inputs.rs"]
mod fixture_inputs;
#[path = "board_e2e/follow_fixture.rs"]
mod follow_fixture;
#[path = "board_e2e/owned_host.rs"]
mod owned_host;
#[path = "board_e2e/owned_host_startup_proofs.rs"]
mod owned_host_startup_proofs;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/records_reopen.rs"]
mod records_reopen;
#[path = "board_e2e/takeover_cancels.rs"]
mod takeover_cancels;
#[path = "board_e2e/takeover_controls.rs"]
mod takeover_controls;
#[path = "board_e2e/takeover_follows.rs"]
mod takeover_follows;
#[path = "board_e2e/takeover_prefixes.rs"]
mod takeover_prefixes;
#[path = "board_e2e/takeover_readbacks.rs"]
mod takeover_readbacks;
#[path = "board_e2e/takeover_recipients.rs"]
mod takeover_recipients;
use codex_router_test_support::FixtureResult;
use takeover_prefixes::ReplicatedPrefix;

#[tokio::test]
async fn takeover_recovers_the_same_frozen_item_after_holder_dies_with_both_commits_acknowledged()
-> FixtureResult {
    central_pair::scenario(async |fixture, second| {
        takeover_prefixes::prefix(fixture, second, ReplicatedPrefix::Both).await
    })
    .await
}

#[tokio::test]
async fn takeover_with_neither_commit_acknowledged_does_not_invent_remote_custody() -> FixtureResult
{
    central_pair::scenario(async |fixture, second| {
        takeover_prefixes::prefix(fixture, second, ReplicatedPrefix::Neither).await
    })
    .await
}

#[tokio::test]
async fn takeover_before_local_cause_capture_does_not_invent_remote_custody() -> FixtureResult {
    central_pair::scenario(takeover_prefixes::uncaptured_prefix).await
}

#[tokio::test]
async fn takeover_with_only_cause_acknowledged_derives_a_new_item_once() -> FixtureResult {
    central_pair::scenario(async |fixture, second| {
        takeover_prefixes::prefix(fixture, second, ReplicatedPrefix::CauseOnly).await
    })
    .await
}

#[tokio::test]
async fn held_local_follow_stays_prevented_when_takeover_discards_its_subscription_end()
-> FixtureResult {
    central_pair::scenario(takeover_cancels::held_local).await
}

#[tokio::test]
async fn unreplicated_subscription_end_cannot_cancel_the_successor_follow() -> FixtureResult {
    central_pair::scenario(async |fixture, second| {
        takeover_cancels::remote(
            fixture,
            second,
            takeover_cancels::RemoteEndPrefix::Discarded,
        )
        .await
    })
    .await
}

#[tokio::test]
async fn replicated_subscription_end_reuses_its_original_cancel_and_starts_no_wake() -> FixtureResult
{
    central_pair::scenario(async |fixture, second| {
        takeover_cancels::remote(
            fixture,
            second,
            takeover_cancels::RemoteEndPrefix::Retained(takeover_cancels::FrontierCut::Unsettled),
        )
        .await
    })
    .await
}

#[tokio::test]
async fn settled_subscription_end_keeps_its_frozen_cancel_without_local_replay_or_a_wake()
-> FixtureResult {
    central_pair::scenario(async |fixture, second| {
        takeover_cancels::remote(
            fixture,
            second,
            takeover_cancels::RemoteEndPrefix::Retained(takeover_cancels::FrontierCut::Settled),
        )
        .await
    })
    .await
}

#[tokio::test]
async fn lost_cancel_receipt_replays_the_same_cancel_after_takeover_without_a_recipient_effect()
-> FixtureResult {
    central_pair::scenario(async |fixture, second| {
        takeover_cancels::remote(
            fixture,
            second,
            takeover_cancels::RemoteEndPrefix::LostReceipt,
        )
        .await
    })
    .await
}

#[tokio::test]
async fn owed_follow_survives_clean_move_with_the_same_item_generation_and_range() -> FixtureResult
{
    central_pair::scenario(async |fixture, second| {
        takeover_follows::transfer(fixture, second, takeover_follows::TransferKind::Move).await
    })
    .await
}

#[tokio::test]
async fn owed_follow_survives_takeover_with_the_same_item_generation_and_range() -> FixtureResult {
    central_pair::scenario(async |fixture, second| {
        takeover_follows::transfer(fixture, second, takeover_follows::TransferKind::Takeover).await
    })
    .await
}
