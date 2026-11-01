//! Each wait follows a real source-page or render event; no whole-journey budget.
use agent_session_inventory::{
    picker_runtime_status::NativeLiveReadState,
    source_inventory_request::{SourceInventoryRejection, SourceInventoryResult},
};
use futures_util::{Stream, StreamExt};
use iocraft::Canvas;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

// A causal observation must not expire before its producer's own deadline.
// Native child reads have a five-second deadline inside the existing configured
// source page's thirty-second budget. Paint guards remain separate and shorter.
const CONFIGURED_SOURCE_PAGE_BOUND: Duration = Duration::from_secs(30);

pub(super) struct SourcePageSettlement {
    native_finished: bool,
}
impl SourcePageSettlement {
    pub(super) fn from_result(result: &SourceInventoryResult) -> Self {
        // These are actual read returns. Native pages/errors follow the reader's
        // joined stdout/stderr EOF and child.wait, including explicit settlement
        // of a failed or timed-out child. A generic rejection is not that fact.
        let native_finished = match result {
            SourceInventoryResult::Rejected {
                reason: SourceInventoryRejection::InvalidNativeInventory { .. },
                ..
            } => true,
            SourceInventoryResult::Ready { snapshot, .. } => {
                !snapshot.native_live_coverage.is_empty()
                    && snapshot
                        .native_live_coverage
                        .iter()
                        .all(|part| part.state != NativeLiveReadState::Reading)
            }
            _ => false,
        };
        Self { native_finished }
    }
}

pub(super) async fn frame_until<TStream: Stream<Item = Canvas> + Unpin>(
    frames: &mut TStream,
    last: &mut String,
    waiting_for: &str,
    event_bound: Duration,
    predicate: impl Fn(&str) -> bool,
) {
    let observed = tokio::time::timeout(event_bound, async {
        if predicate(last) {
            return;
        }
        while let Some(canvas) = frames.next().await {
            *last = canvas.to_string();
            if predicate(last) {
                return;
            }
        }
        panic!("render loop ended before {waiting_for}: {last}");
    })
    .await;
    assert!(observed.is_ok(), "waiting for {waiting_for}: {last}");
}

pub(super) async fn native_inventory_settled<TStream: Stream<Item = Canvas> + Unpin>(
    frames: &mut TStream,
    settlements: &mut UnboundedReceiver<SourcePageSettlement>,
    last: &mut String,
) {
    loop {
        let observed = tokio::time::timeout(CONFIGURED_SOURCE_PAGE_BOUND, async {
            loop {
                tokio::select! {
                    page = settlements.recv() => return page.expect("real source-page observer remains open"),
                    frame = frames.next() => {
                        let canvas = frame.expect("render loop remains open until native read settlement");
                        *last = canvas.to_string();
                    }
                }
            }
        }).await;
        let page = observed.unwrap_or_else(|_| {
            panic!("waiting for actual source-page settlement and native child EOF/exit: {last}")
        });
        if page.native_finished {
            return;
        }
    }
}

pub(super) async fn render_loop_ended<TStream: Stream<Item = Canvas> + Unpin>(
    frames: &mut TStream,
    last: &mut String,
    event_bound: Duration,
) {
    let ended = tokio::time::timeout(event_bound, async {
        while let Some(canvas) = frames.next().await {
            *last = canvas.to_string();
        }
    })
    .await;
    assert!(
        ended.is_ok(),
        "waiting for picker render-loop exit after Quit: {last}"
    );
}
