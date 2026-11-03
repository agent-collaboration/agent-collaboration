//! Bounded independent invocation barriers owned by one conformance object.
use super::{selected_calls::DispatchSelection, signal::Signal};
use crate::{
    DispatchFaultObservation, DispatchHoldObservation, DispatchKey, RuntimeControlFailure,
};
use collaboration_central_core::ObjectAddress;
use std::sync::Arc;

const MAX_DISPATCH_KEYS: usize = 4;

pub(super) struct DispatchSignals {
    pub reached: Arc<Signal>,
    pub release: Arc<Signal>,
}

enum HoldState {
    Armed,
    Held(Vec<u8>),
}
struct DispatchHold {
    key: DispatchKey,
    state: HoldState,
    reached: Arc<Signal>,
    release: Arc<Signal>,
}
impl DispatchHold {
    fn armed(key: DispatchKey) -> Self {
        Self {
            key,
            state: HoldState::Armed,
            reached: Arc::default(),
            release: Arc::default(),
        }
    }
}

#[derive(Default)]
pub(super) struct DispatchHolds(Vec<DispatchHold>);
impl DispatchHolds {
    pub(super) fn arm(&mut self, key: DispatchKey) -> Result<(), RuntimeControlFailure> {
        if self.0.iter().any(|hold| hold.key == key) {
            return Err(RuntimeControlFailure::AlreadyArmed);
        }
        if self.0.len() == MAX_DISPATCH_KEYS {
            return Err(RuntimeControlFailure::TooManyDispatchKeys);
        }
        self.0.push(DispatchHold::armed(key));
        Ok(())
    }

    pub(super) fn before_dispatch(
        &mut self,
        destination: &ObjectAddress,
        request: &[u8],
    ) -> Result<Option<DispatchSignals>, RuntimeControlFailure> {
        let mut selected = None;
        for (index, hold) in self.0.iter().enumerate() {
            if matches!(hold.state, HoldState::Armed)
                && hold.key.destination == *destination
                && DispatchSelection::Call(hold.key.call).matches(request)
                && selected.replace(index).is_some()
            {
                return Err(RuntimeControlFailure::AmbiguousDispatchKey);
            }
        }
        let Some(index) = selected else {
            return Ok(None);
        };
        let hold = self
            .0
            .get_mut(index)
            .ok_or(RuntimeControlFailure::NotArmed)?;
        hold.state = HoldState::Held(request.to_vec());
        Ok(Some(DispatchSignals {
            reached: hold.reached.clone(),
            release: hold.release.clone(),
        }))
    }

    pub(super) fn reached(&self, key: &DispatchKey) -> Result<Arc<Signal>, RuntimeControlFailure> {
        self.0
            .iter()
            .find(|hold| hold.key == *key)
            .map(|hold| hold.reached.clone())
            .ok_or(RuntimeControlFailure::NotArmed)
    }

    pub(super) fn resume(
        &mut self,
        key: &DispatchKey,
        next: Option<DispatchKey>,
    ) -> Result<Arc<Signal>, RuntimeControlFailure> {
        let index = self
            .0
            .iter()
            .position(|hold| hold.key == *key)
            .ok_or(RuntimeControlFailure::NotHeld)?;
        let held = self.0.get(index).ok_or(RuntimeControlFailure::NotHeld)?;
        if !matches!(held.state, HoldState::Held(_)) {
            return Err(RuntimeControlFailure::NotHeld);
        }
        if let Some(next) = &next
            && self
                .0
                .iter()
                .any(|hold| hold.key == *next && hold.key != *key)
        {
            return Err(RuntimeControlFailure::AlreadyArmed);
        }
        let release = self.0.remove(index).release;
        if let Some(next) = next {
            self.0.push(DispatchHold::armed(next));
        }
        Ok(release)
    }

    pub(super) fn held_request(&self, key: &DispatchKey) -> Result<Vec<u8>, RuntimeControlFailure> {
        let held = self
            .0
            .iter()
            .find(|hold| hold.key == *key)
            .ok_or(RuntimeControlFailure::NotHeld)?;
        match &held.state {
            HoldState::Held(request) => Ok(request.clone()),
            HoldState::Armed => Err(RuntimeControlFailure::NotHeld),
        }
    }

    pub(super) fn observe(&self) -> Vec<DispatchHoldObservation> {
        self.0
            .iter()
            .map(|hold| DispatchHoldObservation {
                key: hold.key.clone(),
                state: match &hold.state {
                    HoldState::Armed => DispatchFaultObservation::Armed {
                        destination: hold.key.destination.clone(),
                    },
                    HoldState::Held(request) => DispatchFaultObservation::Held {
                        destination: hold.key.destination.clone(),
                        request: request.clone(),
                    },
                },
            })
            .collect()
    }
}
