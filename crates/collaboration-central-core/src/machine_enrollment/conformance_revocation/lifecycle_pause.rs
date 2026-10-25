//! A conformance pause holds a lifecycle checkpoint outside the store boundary.

use durable_steps::StoreError;
use futures_channel::oneshot;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecyclePauseObservation {
    pub armed: bool,
    pub reached: bool,
}

#[derive(Default)]
struct PauseState {
    observation: LifecyclePauseObservation,
    release: Vec<oneshot::Sender<()>>,
}

#[derive(Default)]
pub(crate) struct LifecyclePause {
    state: Mutex<PauseState>,
}

impl LifecyclePause {
    /// Leave the operation unfinished while returning its alarm drive to the host.
    pub(crate) fn defer_completion(&self) -> Result<bool, StoreError> {
        let mut state = self.state.lock().map_err(StoreError::backend)?;
        if state.observation.armed {
            state.observation.reached = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(super) fn arm(&self) -> Result<(), StoreError> {
        let mut state = self.state.lock().map_err(StoreError::backend)?;
        if state.observation.armed {
            return Err(StoreError::Aborted {
                reason: "lifecycle checkpoint already armed".into(),
            });
        }
        state.observation = LifecyclePauseObservation {
            armed: true,
            reached: false,
        };
        Ok(())
    }

    pub(super) fn release(&self) -> Result<(), StoreError> {
        let releases = {
            let mut state = self.state.lock().map_err(StoreError::backend)?;
            state.observation.armed = false;
            std::mem::take(&mut state.release)
        };
        for release in releases {
            let _ = release.send(());
        }
        Ok(())
    }

    pub(crate) async fn pause(&self) -> Result<(), StoreError> {
        let pending = {
            let mut state = self.state.lock().map_err(StoreError::backend)?;
            if state.observation.armed {
                state.observation.reached = true;
                let (release, pending) = oneshot::channel();
                state.release.push(release);
                Some(pending)
            } else {
                None
            }
        };
        if let Some(pending) = pending {
            pending.await.map_err(StoreError::backend)?;
        }
        Ok(())
    }

    pub(super) fn observation(&self) -> Result<LifecyclePauseObservation, StoreError> {
        Ok(self.state.lock().map_err(StoreError::backend)?.observation)
    }
}
