//! OB4 rows 5/6/7/13 through compiled Hosts, real Copy ACKs and recipient sockets.

#![deny(clippy::too_many_lines)]
use crate::owned_host;
#[path = "../board_e2e/owned_host_startup_proofs.rs"]
mod owned_host_startup_proofs;
#[path = "../board_e2e/takeover_cancels.rs"]
mod takeover_cancels;
#[path = "../board_e2e/takeover_controls.rs"]
mod takeover_controls;
#[path = "../board_e2e/takeover_follows.rs"]
mod takeover_follows;
#[path = "../board_e2e/takeover_prefixes.rs"]
mod takeover_prefixes;
#[expect(
    dead_code,
    reason = "Each e2e journey file uses a different part of the shared recipient fixtures."
)]
#[path = "../board_e2e/takeover_recipients.rs"]
mod takeover_recipients;
