//! A subscription Batch for a Claude Code peer that isn't running is refused at once,
//! whatever its idle policy: nothing will start the terminal or say it started. The posts
//! stay unread on the board, and once the peer runs again a later post reaches it.

#[path = "../support/claude_peer_subscription_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod fixture;
