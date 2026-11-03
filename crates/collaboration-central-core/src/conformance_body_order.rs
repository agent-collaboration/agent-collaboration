//! Read-only body progress is correlated without changing HTTP admission.

use message_board::{IdentityValidationError, RecordId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    future::poll_fn,
    num::NonZeroU64,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};

pub const BODY_ORDER_HEADER: &str = "x-central-conformance-body";

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BodyObservationId(RecordId);

impl TryFrom<String> for BodyObservationId {
    type Error = IdentityValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        RecordId::try_from(value).map(Self)
    }
}

impl BodyObservationId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BodyOrderCommand {
    Begin {
        request: BodyObservationId,
    },
    AwaitRead {
        request: BodyObservationId,
        minimum_bytes: NonZeroU64,
    },
    Finish {
        request: BodyObservationId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BodyOrderObservation {
    Reading { bytes_read: u64 },
    BodyComplete { bytes_read: u64 },
    ResponseBeforeEnd { bytes_read: u64 },
    ResponseAfterEnd { bytes_read: u64 },
    Unknown {},
    Conflict {},
    Unavailable {},
}

impl Default for BodyOrderObservation {
    fn default() -> Self {
        Self::Reading { bytes_read: 0 }
    }
}

impl BodyOrderObservation {
    #[must_use]
    pub const fn bytes_read(&self) -> Option<u64> {
        match self {
            Self::Reading { bytes_read }
            | Self::BodyComplete { bytes_read }
            | Self::ResponseBeforeEnd { bytes_read }
            | Self::ResponseAfterEnd { bytes_read } => Some(*bytes_read),
            Self::Unknown {} | Self::Conflict {} | Self::Unavailable {} => None,
        }
    }
}

#[derive(Default)]
struct BodyEntry {
    state: Mutex<BodyState>,
}

#[derive(Default)]
struct BodyState {
    observed: BodyOrderObservation,
    waiters: Vec<Waker>,
}

#[derive(Clone)]
pub struct BodyOrderCapture(Arc<BodyEntry>);

impl BodyOrderCapture {
    fn record(&self, change: impl FnOnce(&mut BodyOrderObservation)) {
        let waiters = {
            let Ok(mut state) = self.0.state.lock() else {
                return;
            };
            change(&mut state.observed);
            std::mem::take(&mut state.waiters)
        };
        for waiter in waiters {
            waiter.wake();
        }
    }

    /// Call only after the real stream yielded these bytes.
    pub fn record_read(&self, bytes: usize) {
        self.record(|observed| {
            let Some(total) = u64::try_from(bytes)
                .ok()
                .and_then(|bytes| observed.bytes_read()?.checked_add(bytes))
            else {
                *observed = BodyOrderObservation::Unavailable {};
                return;
            };
            *observed = match observed {
                BodyOrderObservation::Reading { .. } => {
                    BodyOrderObservation::Reading { bytes_read: total }
                }
                BodyOrderObservation::ResponseBeforeEnd { .. } => {
                    BodyOrderObservation::ResponseBeforeEnd { bytes_read: total }
                }
                _ => BodyOrderObservation::Unavailable {},
            };
        });
    }

    /// Call only when the real stream yielded EOF.
    pub fn record_end(&self) {
        self.record(|observed| {
            if let BodyOrderObservation::Reading { bytes_read } = *observed {
                *observed = BodyOrderObservation::BodyComplete { bytes_read };
            }
        });
    }

    pub fn record_response(&self) {
        self.record(|observed| {
            *observed = match *observed {
                BodyOrderObservation::Reading { bytes_read } => {
                    BodyOrderObservation::ResponseBeforeEnd { bytes_read }
                }
                BodyOrderObservation::BodyComplete { bytes_read } => {
                    BodyOrderObservation::ResponseAfterEnd { bytes_read }
                }
                ref other => other.clone(),
            };
        });
    }
}

#[derive(Default)]
pub struct BodyOrderProbe {
    requests: Mutex<BTreeMap<BodyObservationId, Arc<BodyEntry>>>,
}

impl BodyOrderProbe {
    /// Unregistered annotations cannot allocate probe state on the body path.
    pub fn capture(&self, request: &BodyObservationId) -> Option<BodyOrderCapture> {
        self.requests
            .lock()
            .ok()?
            .get(request)
            .cloned()
            .map(BodyOrderCapture)
    }

    pub async fn control(&self, command: BodyOrderCommand) -> BodyOrderObservation {
        match command {
            BodyOrderCommand::Begin { request } => {
                let Ok(mut requests) = self.requests.lock() else {
                    return BodyOrderObservation::Unavailable {};
                };
                match requests.entry(request) {
                    Entry::Vacant(entry) => {
                        entry.insert(Arc::default());
                        BodyOrderObservation::default()
                    }
                    Entry::Occupied(_) => BodyOrderObservation::Conflict {},
                }
            }
            BodyOrderCommand::AwaitRead {
                request,
                minimum_bytes,
            } => {
                let entry = {
                    let Ok(requests) = self.requests.lock() else {
                        return BodyOrderObservation::Unavailable {};
                    };
                    requests.get(&request).cloned()
                };
                let Some(entry) = entry else {
                    return BodyOrderObservation::Unknown {};
                };
                poll_fn(|context| {
                    let Ok(mut state) = entry.state.lock() else {
                        return Poll::Ready(BodyOrderObservation::Unavailable {});
                    };
                    if !matches!(state.observed, BodyOrderObservation::Reading { bytes_read } if bytes_read < minimum_bytes.get()) {
                        return Poll::Ready(state.observed.clone());
                    }
                    if !state.waiters.iter().any(|waiter| waiter.will_wake(context.waker())) {
                        state.waiters.push(context.waker().clone());
                    }
                    Poll::Pending
                }).await
            }
            BodyOrderCommand::Finish { request } => {
                let entry = match self.requests.lock() {
                    Ok(mut requests) => requests.remove(&request),
                    Err(_) => return BodyOrderObservation::Unavailable {},
                };
                let Some(entry) = entry else {
                    return BodyOrderObservation::Unknown {};
                };
                match entry.state.lock() {
                    Ok(state) => state.observed.clone(),
                    Err(_) => BodyOrderObservation::Unavailable {},
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::pin,
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Wake},
    };

    #[derive(Default)]
    struct WakeCount(AtomicUsize);

    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn request_id() -> BodyObservationId {
        "019f0000-0000-7000-8000-000000000540"
            .to_owned()
            .try_into()
            .expect("injected body observation ID")
    }

    fn immediate<Output>(future: impl Future<Output = Output>) -> Output {
        let waker = Waker::from(Arc::new(WakeCount::default()));
        let mut future = pin!(future);
        match future.as_mut().poll(&mut Context::from_waker(&waker)) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("this metadata operation must be immediately ready"),
        }
    }

    // The real read counter is the predicate; a read before or after the
    // observer's first poll must wake/complete it without a timer or lost event.
    #[test]
    fn conformance_body_read_event_survives_both_observer_interleavings() {
        for read_first in [false, true] {
            let probe = BodyOrderProbe::default();
            let request = request_id();
            assert_eq!(
                immediate(probe.control(BodyOrderCommand::Begin {
                    request: request.clone()
                })),
                BodyOrderObservation::Reading { bytes_read: 0 },
            );
            let capture = probe.capture(&request).expect("registered capture");
            let wakes = Arc::new(WakeCount::default());
            let waker = Waker::from(wakes.clone());
            let mut waiting = pin!(probe.control(BodyOrderCommand::AwaitRead {
                request,
                minimum_bytes: NonZeroU64::new(1).unwrap(),
            }));
            if read_first {
                capture.record_read(1);
            } else {
                assert!(
                    waiting
                        .as_mut()
                        .poll(&mut Context::from_waker(&waker))
                        .is_pending()
                );
                capture.record_read(1);
                assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
            }
            assert_eq!(
                waiting.as_mut().poll(&mut Context::from_waker(&waker)),
                Poll::Ready(BodyOrderObservation::Reading { bytes_read: 1 }),
            );
        }
    }

    // A later EOF cannot turn an actual premature response into a successful
    // ordering report, even if the rest of the upload eventually arrives.
    #[test]
    fn conformance_body_late_eof_never_redeems_an_early_response() {
        let probe = BodyOrderProbe::default();
        let request = request_id();
        immediate(probe.control(BodyOrderCommand::Begin {
            request: request.clone(),
        }));
        let capture = probe.capture(&request).unwrap();
        capture.record_read(1);
        capture.record_response();
        capture.record_read(2);
        capture.record_end();
        assert_eq!(
            immediate(probe.control(BodyOrderCommand::Finish { request })),
            BodyOrderObservation::ResponseBeforeEnd { bytes_read: 3 },
        );
    }

    #[test]
    fn conformance_body_annotations_cannot_register_their_own_capture() {
        let probe = BodyOrderProbe::default();
        let request = request_id();
        assert!(probe.capture(&request).is_none());
        assert_eq!(
            immediate(probe.control(BodyOrderCommand::Finish { request })),
            BodyOrderObservation::Unknown {},
        );
    }
}
