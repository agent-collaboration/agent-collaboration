//! Controlled native timer; the ordinary alarm owner still owns every callback.
use super::SharedClock;
use collaboration_central_conformance::{
    AutomationAlarmReceipt, AutomationAlarmResult, AutomationClockObservation,
};
use collaboration_central_core::RegistryTime;
use durable_steps::StoreError;
use runtime_primitives::{Clock, SyncTime, TokioWakeTimer, WakeFailure, WakeScheduler};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};
use tokio::sync::{Mutex, Notify};

impl super::NativeObjects {
    pub(super) async fn automation_clock_control(
        self: &Arc<Self>,
        command: collaboration_central_conformance::AutomationClockControl,
    ) -> Result<collaboration_central_conformance::RuntimeFaultObservation, StoreError> {
        use collaboration_central_conformance::AutomationClockControl;
        let clock =
            self.startup_controls.clock.as_ref().ok_or_else(|| {
                StoreError::backend("verification clock not configured at startup")
            })?;
        let _advance = clock.advance.lock().await;
        let fleet = self
            .slot(collaboration_central_core::ObjectAddress::Fleet)
            .await?;
        // Finish callbacks that already started before changing their clock.
        self.drain_alarm_owners().await?;
        match command {
            AutomationClockControl::Start { .. } => {
                return Err(StoreError::backend("Start is a startup setting"));
            }
            AutomationClockControl::Advance { now } => clock.set(now).await?,
            AutomationClockControl::InvokeAlarm => fleet
                .io
                .wake
                .wake_at(clock.now())
                .await
                .map_err(StoreError::backend)?,
            AutomationClockControl::RemoveAlarm | AutomationClockControl::Observe => {}
        }
        self.drain_alarm_owners().await?;
        if matches!(command, AutomationClockControl::RemoveAlarm) {
            fleet.io.wake.remove().await?;
        }
        let now = clock.observation_time()?;
        let mut observed = fleet.io.faults.observe().map_err(StoreError::backend)?;
        observed.clock = collaboration_central_conformance::ClockObservation::Pinned { now };
        observed.automation_clock = Some(fleet.io.wake.observe(now).await?);
        Ok(observed)
    }

    async fn drain_alarm_owners(&self) -> Result<(), StoreError> {
        // No business call repairs the Fleet. The startup-discovered owners and
        // their existing callbacks drain through completion signals.
        loop {
            let slots: Vec<_> = self.slots.lock().await.values().cloned().collect();
            let count = slots.len();
            for slot in slots {
                slot.io.wake.drain().await?;
            }
            let slots: Vec<_> = self.slots.lock().await.values().cloned().collect();
            let mut drained = slots.len() == count;
            for slot in slots {
                drained &= slot.io.wake.is_drained().await?;
            }
            if drained {
                break;
            }
        }
        Ok(())
    }
}

const CLOCK_FILE: &str = "_automation-verification-clock.json";
const MAXIMUM_CALLBACK_RECEIPTS: usize = 10_000;

pub(crate) struct VerificationClock {
    now_ms: AtomicI64,
    path: PathBuf,
    pub(super) advance: Mutex<()>,
}
impl VerificationClock {
    pub(crate) async fn open(root: &Path, initial: RegistryTime) -> Result<Arc<Self>, StoreError> {
        tokio::fs::create_dir_all(root)
            .await
            .map_err(StoreError::backend)?;
        let path = root.join(CLOCK_FILE);
        let now = match tokio::fs::read(&path).await {
            Ok(bytes) => {
                serde_json::from_slice::<RegistryTime>(&bytes).map_err(StoreError::backend)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => initial,
            Err(error) => return Err(StoreError::backend(error)),
        };
        let clock = Arc::new(Self {
            now_ms: AtomicI64::new(now.0.timestamp_millis()),
            path,
            advance: Mutex::new(()),
        });
        clock.persist(now).await?;
        Ok(clock)
    }
    pub(super) async fn set(&self, now: RegistryTime) -> Result<(), StoreError> {
        if now.0.timestamp_millis() < self.now().unix_millis() {
            return Err(StoreError::backend(
                "verification clock cannot move backwards",
            ));
        }
        self.persist(now).await?;
        self.now_ms
            .store(now.0.timestamp_millis(), Ordering::SeqCst);
        Ok(())
    }
    async fn persist(&self, now: RegistryTime) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec(&now).map_err(StoreError::backend)?;
        let temporary = self.path.with_extension("json.next");
        tokio::fs::write(&temporary, bytes)
            .await
            .map_err(StoreError::backend)?;
        tokio::fs::rename(temporary, &self.path)
            .await
            .map_err(StoreError::backend)
    }
    pub(super) fn observation_time(&self) -> Result<RegistryTime, StoreError> {
        chrono::DateTime::from_timestamp_millis(self.now().unix_millis())
            .map(RegistryTime)
            .ok_or_else(|| StoreError::backend("invalid verification time"))
    }
}
impl Clock for VerificationClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.now_ms.load(Ordering::SeqCst))
    }
}

pub(super) enum NativeWake {
    Wall(TokioWakeTimer<SharedClock>),
    Controlled(ControlledWake),
}
impl NativeWake {
    pub(super) fn new(clock: SharedClock, controlled: bool, startup_callback: bool) -> Self {
        if controlled {
            Self::Controlled(ControlledWake {
                clock,
                state: Mutex::new(ControlledState {
                    deadline: None,
                    flight: if startup_callback {
                        CallbackFlight::Driving
                    } else {
                        CallbackFlight::Idle
                    },
                    receipts: Vec::new(),
                }),
                requested: Notify::new(),
                settled: Notify::new(),
            })
        } else {
            Self::Wall(TokioWakeTimer::new(clock))
        }
    }
    pub(super) async fn wait(&self) {
        match self {
            Self::Wall(wake) => {
                wake.wait().await;
            }
            Self::Controlled(wake) => wake.wait().await,
        }
    }
    pub(super) async fn completed(&self, failed: bool) {
        if let Self::Controlled(wake) = self {
            let mut state = wake.state.lock().await;
            state.flight = CallbackFlight::Idle;
            let next_alarm_ms = state.deadline.map(SyncTime::unix_millis);
            if state.receipts.len() < MAXIMUM_CALLBACK_RECEIPTS {
                state.receipts.push(AutomationAlarmReceipt {
                    at_ms: wake.clock.now().unix_millis(),
                    result: if failed {
                        AutomationAlarmResult::Failed
                    } else {
                        AutomationAlarmResult::Completed
                    },
                    next_alarm_ms,
                });
            }
            if state.receipts.len() >= MAXIMUM_CALLBACK_RECEIPTS {
                state.flight = CallbackFlight::Exhausted;
            }
            wake.settled.notify_waiters();
        }
    }
    pub(super) async fn drain(&self) -> Result<(), StoreError> {
        let Self::Controlled(wake) = self else {
            return Err(StoreError::backend("controlled timer not configured"));
        };
        loop {
            let changed = wake.settled.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let state = wake.state.lock().await;
                if state.receipts.len() >= MAXIMUM_CALLBACK_RECEIPTS {
                    return Err(StoreError::backend("verification callback limit reached"));
                }
                if matches!(state.flight, CallbackFlight::Idle)
                    && !state.deadline.is_some_and(|at| at <= wake.clock.now())
                {
                    return Ok(());
                }
            }
            // Also wakes the owner if advancing made its saved deadline due.
            wake.requested.notify_one();
            changed.await;
        }
    }
    pub(super) async fn remove(&self) -> Result<(), StoreError> {
        let Self::Controlled(wake) = self else {
            return Err(StoreError::backend("controlled timer not configured"));
        };
        wake.state.lock().await.deadline = None;
        wake.requested.notify_one();
        Ok(())
    }
    async fn is_drained(&self) -> Result<bool, StoreError> {
        let Self::Controlled(wake) = self else {
            return Err(StoreError::backend("controlled timer not configured"));
        };
        let state = wake.state.lock().await;
        Ok(matches!(state.flight, CallbackFlight::Idle)
            && !state.deadline.is_some_and(|at| at <= wake.clock.now()))
    }
    pub(super) async fn observe(
        &self,
        now: RegistryTime,
    ) -> Result<AutomationClockObservation, StoreError> {
        let Self::Controlled(wake) = self else {
            return Err(StoreError::backend("controlled timer not configured"));
        };
        let state = wake.state.lock().await;
        Ok(AutomationClockObservation {
            now,
            next_alarm_ms: state.deadline.map(SyncTime::unix_millis),
            callbacks: state.receipts.clone(),
        })
    }
}
impl WakeScheduler for NativeWake {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        match self {
            Self::Wall(wake) => wake.wake_at(at).await,
            Self::Controlled(wake) => {
                let mut state = wake.state.lock().await;
                state.deadline = Some(state.deadline.map_or(at, |saved| saved.min(at)));
                wake.requested.notify_one();
                Ok(())
            }
        }
    }
}
pub(super) struct ControlledWake {
    clock: SharedClock,
    state: Mutex<ControlledState>,
    requested: Notify,
    settled: Notify,
}
enum CallbackFlight {
    Idle,
    Driving,
    Exhausted,
}
struct ControlledState {
    deadline: Option<SyncTime>,
    flight: CallbackFlight,
    receipts: Vec<AutomationAlarmReceipt>,
}
impl ControlledWake {
    async fn wait(&self) {
        loop {
            let changed = self.requested.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let mut state = self.state.lock().await;
                if matches!(state.flight, CallbackFlight::Idle)
                    && state.deadline.is_some_and(|at| at <= self.clock.now())
                {
                    state.deadline = None;
                    state.flight = CallbackFlight::Driving;
                    return;
                }
            }
            changed.await;
        }
    }
}
