//! Test seam: hold a request at a named point to force races and crashes
//! (Program Design `#schemas` proof seam, Verification O1 and O4).
//!
//! A case arms a point for one workspace; the next request that reaches it stops there
//! until the case releases it, or until the case drops the object (a simulated crash)
//! and the held request's future goes with it. Built only with `test-support`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::watch;
use workspace_ids::WorkspaceId;

/// Where a request can be held.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PausePoint {
    /// After the rule's reads, before its batch.
    AfterReads,
    /// After the store applied the batch, before the drain.
    AfterStoreApplied,
    /// After the notices committed.
    AfterNoticesCommitted,
    BeforeDrainCommit,
    AfterDrainCommit,
    BeforeTimerCommit,
    AfterTimerCommit,
    /// Engine points are keyed by instance (`hold_engine`): after `handle` loaded
    /// its instance, after the instance batch committed, before a failure write,
    /// and after a request found no live receipt.
    AfterEngineLoad,
    AfterEngineCommit,
    BeforeEngineFailureWrite,
    AfterEngineAbsentLookup,
    /// Keyed by the notice unit's kind: after its run succeeded, before it
    /// clears the control record it observed at its gate.
    BeforeControlClear,
    BeforeSignInSample,
    AfterSignInSample,
}

type PauseKey = (WorkspaceId, PausePoint);
type EngineKey = (PausePoint, String);

#[derive(Clone, Default)]
pub struct PausePoints {
    armed: Arc<Mutex<BTreeMap<PauseKey, ArmedPause>>>,
    held: Arc<Mutex<BTreeMap<WorkspaceId, Vec<PausePoint>>>>,
    central: Arc<Mutex<BTreeMap<PausePoint, ArmedPause>>>,
    /// Several holds may be armed on one key; arrivals take them in arming order,
    /// so two callers on one instance or request can each be held.
    engine: Arc<Mutex<BTreeMap<EngineKey, VecDeque<ArmedPause>>>>,
}

struct ArmedPause {
    arrived: watch::Sender<bool>,
    release: watch::Receiver<bool>,
}

/// The case's handle on one armed point. Dropping it releases the point.
pub struct HeldPause {
    arrived: watch::Receiver<bool>,
    release: watch::Sender<bool>,
}

impl HeldPause {
    /// Resolves once a request is held at this point.
    pub async fn until_held(&mut self) {
        let _ = self.arrived.wait_for(|arrived| *arrived).await;
    }

    /// Lets the held request (or the next one to arrive) continue.
    pub fn release(self) {
        drop(self);
    }
}

impl Drop for HeldPause {
    fn drop(&mut self) {
        self.release.send_replace(true);
    }
}

impl PausePoints {
    #[must_use]
    pub fn hold_central(&self, point: PausePoint) -> HeldPause {
        let (arrived_sender, arrived) = watch::channel(false);
        let (release, release_receiver) = watch::channel(false);
        self.central
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                point,
                ArmedPause {
                    arrived: arrived_sender,
                    release: release_receiver,
                },
            );
        HeldPause { arrived, release }
    }

    pub async fn reached_central(&self, point: PausePoint) {
        let armed = self
            .central
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&point);
        if let Some(mut armed) = armed {
            armed.arrived.send_replace(true);
            let _ = armed.release.wait_for(|released| *released).await;
        }
    }
    /// Arms one engine hold on `point` for `key` (an instance or request key).
    #[must_use]
    pub fn hold_engine(&self, point: PausePoint, key: &str) -> HeldPause {
        let (arrived_sender, arrived) = watch::channel(false);
        let (release, release_receiver) = watch::channel(false);
        self.engine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry((point, key.to_owned()))
            .or_default()
            .push_back(ArmedPause {
                arrived: arrived_sender,
                release: release_receiver,
            });
        HeldPause { arrived, release }
    }

    /// Called by the engine at `point` for `key`; waits only when a case armed it.
    pub async fn reached_engine(&self, point: PausePoint, key: &str) {
        let armed = {
            let mut engine = self.engine.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = (point, key.to_owned());
            let armed = engine.get_mut(&entry).and_then(VecDeque::pop_front);
            if engine.get(&entry).is_some_and(VecDeque::is_empty) {
                engine.remove(&entry);
            }
            armed
        };
        if let Some(mut armed) = armed {
            armed.arrived.send_replace(true);
            let _ = armed.release.wait_for(|released| *released).await;
        }
    }

    /// Arms `point` for `workspace`: the next request to reach it waits for the release.
    #[must_use]
    pub fn hold(&self, workspace: &WorkspaceId, point: PausePoint) -> HeldPause {
        let (arrived_sender, arrived) = watch::channel(false);
        let (release, release_receiver) = watch::channel(false);
        self.armed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                (workspace.clone(), point),
                ArmedPause {
                    arrived: arrived_sender,
                    release: release_receiver,
                },
            );
        HeldPause { arrived, release }
    }

    /// Points where a request on `workspace` is held right now, in arrival order.
    #[must_use]
    pub fn held(&self, workspace: &WorkspaceId) -> Vec<PausePoint> {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(workspace)
            .cloned()
            .unwrap_or_default()
    }

    /// Called by the request path at `point`; waits there only when a case armed it.
    pub async fn reached(&self, workspace: &WorkspaceId, point: PausePoint) {
        let armed = self
            .armed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&(workspace.clone(), point));
        let Some(mut armed) = armed else {
            return;
        };
        let _held = HeldMark::enter(self.held.clone(), workspace.clone(), point);
        armed.arrived.send_replace(true);
        let _ = armed.release.wait_for(|released| *released).await;
    }
}

/// Lists a held request while it waits, and unlists it on release or when dropped.
struct HeldMark {
    held: Arc<Mutex<BTreeMap<WorkspaceId, Vec<PausePoint>>>>,
    workspace: WorkspaceId,
    point: PausePoint,
}

impl HeldMark {
    fn enter(
        held: Arc<Mutex<BTreeMap<WorkspaceId, Vec<PausePoint>>>>,
        workspace: WorkspaceId,
        point: PausePoint,
    ) -> Self {
        held.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(workspace.clone())
            .or_default()
            .push(point);
        Self {
            held,
            workspace,
            point,
        }
    }
}

impl Drop for HeldMark {
    fn drop(&mut self) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(points) = held.get_mut(&self.workspace) {
            if let Some(position) = points.iter().position(|point| *point == self.point) {
                points.remove(position);
            }
            if points.is_empty() {
                held.remove(&self.workspace);
            }
        }
    }
}

#[cfg(test)]
#[path = "pause_points_test.rs"]
mod tests;
