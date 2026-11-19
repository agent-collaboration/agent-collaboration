use central_storage::AlarmPublicationFailure;
use std::sync::{
    Mutex, MutexGuard, PoisonError,
    atomic::{AtomicI64, Ordering},
};

const EMPTY: i64 = i64::MIN;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DueBatchSnapshot {
    pub last_start_unix_millis: Option<i64>,
    pub leftovers_pending: bool,
}

/// Test-support receipt of one unit a due batch started, in start order
/// (acceptance R2/R3 visit order). Never built without `test-support`.
#[cfg(feature = "test-support")]
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DueUnitStart {
    /// The start time of the due batch that started this unit.
    pub batch_start_unix_millis: i64,
    pub machine_kind: String,
    pub subject_key: String,
}

/// The start and backlog bit share one CAS, so a publisher reads one coherent
/// snapshot. The portable timestamp range leaves room for the backlog bit.
pub struct DueBatchState {
    encoded: AtomicI64,
    publication_order: Mutex<()>,
    #[cfg(feature = "test-support")]
    claim_waiting: tokio::sync::watch::Sender<usize>,
    #[cfg(feature = "test-support")]
    unit_starts: Mutex<Vec<DueUnitStart>>,
    /// One entry per claimed batch, so two batches at one millisecond stay two.
    #[cfg(feature = "test-support")]
    claimed_starts: Mutex<Vec<i64>>,
}

impl Default for DueBatchState {
    fn default() -> Self {
        Self {
            encoded: AtomicI64::new(EMPTY),
            publication_order: Mutex::new(()),
            #[cfg(feature = "test-support")]
            claim_waiting: tokio::sync::watch::channel(0).0,
            #[cfg(feature = "test-support")]
            unit_starts: Mutex::new(Vec::new()),
            #[cfg(feature = "test-support")]
            claimed_starts: Mutex::new(Vec::new()),
        }
    }
}

fn snapshot(encoded: i64) -> DueBatchSnapshot {
    if encoded == EMPTY {
        DueBatchSnapshot {
            last_start_unix_millis: None,
            leftovers_pending: false,
        }
    } else {
        DueBatchSnapshot {
            last_start_unix_millis: Some(encoded.div_euclid(2)),
            leftovers_pending: encoded.rem_euclid(2) == 1,
        }
    }
}

fn encode(start: i64, leftovers: bool) -> Option<i64> {
    start.checked_mul(2)?.checked_add(i64::from(leftovers))
}

impl DueBatchState {
    pub fn snapshot(&self) -> DueBatchSnapshot {
        snapshot(self.encoded.load(Ordering::Acquire))
    }

    /// The spacing check and new start claim happen before any caller awaits.
    pub fn try_claim(
        &self,
        now_unix_millis: i64,
        spacing: chrono::Duration,
    ) -> Result<bool, AlarmPublicationFailure> {
        let _publication = self.claim_lock();
        if !(-central_engine_model::MAX_ALARM_UNIX_MILLIS
            ..=central_engine_model::MAX_ALARM_UNIX_MILLIS)
            .contains(&now_unix_millis)
        {
            return Err(AlarmPublicationFailure::NowOutOfRange);
        }
        let mut prior = self.encoded.load(Ordering::Acquire);
        loop {
            let current = snapshot(prior);
            if current.last_start_unix_millis.is_some_and(|start| {
                now_unix_millis.saturating_sub(start) < spacing.num_milliseconds()
            }) {
                return Ok(false);
            }
            let next = encode(now_unix_millis, current.leftovers_pending)
                .ok_or(AlarmPublicationFailure::NowOutOfRange)?;
            match self.encoded.compare_exchange_weak(
                prior,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    #[cfg(feature = "test-support")]
                    self.claimed_starts
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(now_unix_millis);
                    return Ok(true);
                }
                Err(actual) => prior = actual,
            }
        }
    }

    /// An old, slow batch cannot overwrite the newest batch's backlog result.
    pub fn finish(&self, started_at_unix_millis: i64, leftovers_pending: bool) -> bool {
        let _publication = self
            .publication_order
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut prior = self.encoded.load(Ordering::Acquire);
        loop {
            if snapshot(prior).last_start_unix_millis != Some(started_at_unix_millis) {
                return false;
            }
            let Some(next) = encode(started_at_unix_millis, leftovers_pending) else {
                return false;
            };
            match self.encoded.compare_exchange_weak(
                prior,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(actual) => prior = actual,
            }
        }
    }

    pub(super) fn with_publication<R>(&self, publish: impl FnOnce(DueBatchSnapshot) -> R) -> R {
        let _publication = self
            .publication_order
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        publish(self.snapshot())
    }

    fn claim_lock(&self) -> MutexGuard<'_, ()> {
        #[cfg(not(feature = "test-support"))]
        {
            self.publication_order
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
        }
        #[cfg(feature = "test-support")]
        {
            match self.publication_order.try_lock() {
                Ok(guard) => guard,
                Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    self.claim_waiting.send_modify(|count| *count += 1);
                    let guard = self
                        .publication_order
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    self.claim_waiting.send_modify(|count| *count -= 1);
                    guard
                }
            }
        }
    }

    #[cfg(feature = "test-support")]
    pub async fn until_claim_waiting(&self) {
        let mut waiting = self.claim_waiting.subscribe();
        let _ = waiting.wait_for(|count| *count > 0).await;
    }

    /// Called by the due batch as each unit starts, before its first await.
    #[cfg(feature = "test-support")]
    pub(super) fn record_unit_start(&self, start: DueUnitStart) {
        self.unit_starts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(start);
    }

    /// The start time of every due batch this object claimed, in claim order.
    #[cfg(feature = "test-support")]
    pub fn claimed_starts(&self) -> Vec<i64> {
        self.claimed_starts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Every unit started by this object's due batches, in start order.
    #[cfg(feature = "test-support")]
    pub fn unit_starts(&self) -> Vec<DueUnitStart> {
        self.unit_starts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
#[path = "due_batch_state_test.rs"]
mod tests;
