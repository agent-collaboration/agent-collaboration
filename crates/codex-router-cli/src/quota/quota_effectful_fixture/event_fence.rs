//! Post-flush journal completion facts; these never stand in for native input receipts.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum FixtureEventKind {
    FixtureStarted,
    ViewMounted,
    PreferenceSaveReturned,
    GroupingInvocationStarted,
    GroupingInvocationSettled,
    LoginParentExited,
    AccountRunnerSettled,
    FixtureFinished,
    FixtureControlAck,
    FixtureReadback,
    FixtureControlError,
    FixtureMutation,
    FixtureGateReleased,
    LoginChildEntered,
    AccountOwnerStarted,
    AccountOwnerFinished,
    ClipboardWritten,
    RemovalBeforeOwnerHeld,
    RemovalAfterCredentialDeleteHeld,
    RefreshOwnerStarted,
    RefreshFirstRequestHeld,
    RefreshSetupHeld,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub(super) enum FixtureEventFenceResult {
    Observed {
        ordinal: u64,
        event_kind: FixtureEventKind,
    },
    EndedBeforeEvent,
}

#[derive(Clone, Default)]
struct FixtureEventProgress {
    latest: BTreeMap<FixtureEventKind, u64>,
    ended: bool,
}

#[derive(Clone)]
pub(super) struct FixtureEventCompletions(watch::Sender<FixtureEventProgress>);
impl FixtureEventCompletions {
    pub(super) fn new() -> Self {
        Self(watch::channel(FixtureEventProgress::default()).0)
    }

    pub(super) fn flushed(&self, ordinal: u64, kind: FixtureEventKind) {
        self.0.send_modify(|progress| {
            progress.latest.insert(kind, ordinal);
        });
    }

    pub(super) fn finish(&self) {
        self.0.send_modify(|progress| progress.ended = true);
    }

    pub(super) async fn await_event(
        &self,
        after_ordinal: u64,
        kind: FixtureEventKind,
    ) -> FixtureEventFenceResult {
        let mut progress = self.0.subscribe();
        loop {
            {
                let current = progress.borrow_and_update();
                if let Some(ordinal) = current.latest.get(&kind).filter(|&&n| n > after_ordinal) {
                    return FixtureEventFenceResult::Observed {
                        ordinal: *ordinal,
                        event_kind: kind,
                    };
                }
                if current.ended {
                    return FixtureEventFenceResult::EndedBeforeEvent;
                }
            }
            if progress.changed().await.is_err() {
                return FixtureEventFenceResult::EndedBeforeEvent;
            }
        }
    }
}
