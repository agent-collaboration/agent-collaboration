//! A causal read/commit gap and read-only counters for the real Copy wait.

use durable_steps::StoreError;
use futures_channel::oneshot;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopyWaitTarget {
    pub copy: fleet_sync_protocol::ReceiverId,
    pub database: fleet_sync_protocol::DatabaseId,
}
impl durable_steps::CallTarget for CopyWaitTarget {
    const KIND: &'static str = "central.conformance-copy-wait";
    type Request = CopyWaitControl;
    type Answer = CopyWaitObservation;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopyWaitInput {
    pub copy: fleet_sync_protocol::ReceiverId,
    pub database: fleet_sync_protocol::DatabaseId,
    pub control: CopyWaitControl,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct ControlCopyWait;
impl durable_steps::CallTarget for ControlCopyWait {
    const KIND: &'static str = "central.conformance-control-copy-wait";
    type Request = CopyWaitInput;
    type Answer = CopyWaitObservation;
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum CopyWaitControl {
    Observe,
    ArmReadGap,
    ReleaseReadGap,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopyWaitObservation {
    pub waiting: usize,
    pub reads: u64,
    pub attempts: u64,
    pub notifications: u64,
    pub read_gap_reached: bool,
    pub calls: Vec<durable_steps::CallId>,
    pub deadlines: Vec<CopyWaitDeadline>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyWaitDeadline {
    pub armed_at: crate::RegistryTime,
    pub deadline: crate::RegistryTime,
}

#[derive(Default)]
struct ProbeState {
    reads: u64,
    attempts: u64,
    calls: Vec<durable_steps::CallId>,
    deadlines: Vec<CopyWaitDeadline>,
    read_gap_reached: bool,
    read_gap: Option<oneshot::Receiver<()>>,
    release: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
pub(crate) struct CopyWaitProbe {
    state: Mutex<ProbeState>,
}
impl CopyWaitProbe {
    pub(crate) fn record_call(&self, identity: durable_steps::CallId) -> Result<(), StoreError> {
        self.state
            .lock()
            .map_err(StoreError::backend)?
            .calls
            .push(identity);
        Ok(())
    }
    pub(crate) fn enter_attempt(
        &self,
        armed_at: runtime_primitives::SyncTime,
        deadline: runtime_primitives::SyncTime,
    ) -> Result<(), StoreError> {
        let actual = CopyWaitDeadline {
            armed_at: crate::conformance_clock::observed_time(armed_at)?,
            deadline: crate::conformance_clock::observed_time(deadline)?,
        };
        let mut state = self.state.lock().map_err(StoreError::backend)?;
        state.attempts = state.attempts.saturating_add(1);
        state.deadlines.push(actual);
        Ok(())
    }

    pub(crate) async fn after_status_read(&self) -> Result<(), StoreError> {
        let gap = {
            let mut state = self.state.lock().map_err(StoreError::backend)?;
            state.reads = state.reads.saturating_add(1);
            let gap = state.read_gap.take();
            if gap.is_some() {
                state.read_gap_reached = true;
            }
            gap
        };
        if let Some(gap) = gap {
            eprintln!("central conformance: copy read gap reached");
            gap.await.map_err(StoreError::backend)?;
        }
        Ok(())
    }

    pub(crate) fn control(
        &self,
        control: CopyWaitControl,
        waiting: usize,
        notifications: u64,
    ) -> Result<CopyWaitObservation, StoreError> {
        let mut state = self.state.lock().map_err(StoreError::backend)?;
        let release = match control {
            CopyWaitControl::Observe => None,
            CopyWaitControl::ArmReadGap => {
                if state.release.is_some() || state.read_gap.is_some() {
                    return Err(StoreError::Aborted {
                        reason: "copy read gap is already armed".into(),
                    });
                }
                let (release, gap) = oneshot::channel();
                state.release = Some(release);
                state.read_gap = Some(gap);
                state.read_gap_reached = false;
                None
            }
            CopyWaitControl::ReleaseReadGap => {
                if !state.read_gap_reached {
                    return Err(StoreError::Aborted {
                        reason: "copy read gap has not been reached".into(),
                    });
                }
                state.release.take()
            }
        };
        let observation = CopyWaitObservation {
            waiting,
            reads: state.reads,
            attempts: state.attempts,
            notifications,
            read_gap_reached: state.read_gap_reached,
            calls: state.calls.clone(),
            deadlines: state.deadlines.clone(),
        };
        drop(state);
        if let Some(release) = release {
            let _ = release.send(());
        }
        Ok(observation)
    }
}
