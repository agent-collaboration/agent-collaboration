//! One Registry run acquires an incarnation without holding a guard across RPC.
use crate::RegistryIncarnation;
use durable_steps::{CallFailure, NotSentReason};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct IncarnationRun(Arc<Mutex<RunState>>);
enum RunState {
    Unacquired { floor: u64 },
    Acquiring { floor: u64 },
    Active(RegistryIncarnation),
}
impl Default for RunState {
    fn default() -> Self {
        Self::Unacquired { floor: 0 }
    }
}
pub(crate) enum RunAcquisition {
    Active(RegistryIncarnation),
    Acquire(IncarnationAcquisition),
}
pub(crate) struct IncarnationAcquisition {
    state: Arc<Mutex<RunState>>,
    pub floor: u64,
}
impl IncarnationRun {
    pub fn current(&self) -> Result<Option<RegistryIncarnation>, CallFailure> {
        let state = self.0.lock().map_err(|_| CallFailure::AnswerLost)?;
        Ok(match *state {
            RunState::Active(incarnation) => Some(incarnation),
            RunState::Unacquired { .. } | RunState::Acquiring { .. } => None,
        })
    }
    pub fn acquire(&self) -> Result<RunAcquisition, CallFailure> {
        let mut state = self.0.lock().map_err(|_| CallFailure::AnswerLost)?;
        match *state {
            RunState::Active(incarnation) => Ok(RunAcquisition::Active(incarnation)),
            RunState::Acquiring { .. } => Err(CallFailure::NotSent {
                reason: NotSentReason::Overloaded,
            }),
            RunState::Unacquired { floor } => {
                *state = RunState::Acquiring { floor };
                Ok(RunAcquisition::Acquire(IncarnationAcquisition {
                    state: self.0.clone(),
                    floor,
                }))
            }
        }
    }
    pub fn invalidate(&self, floor: u64) -> Result<(), CallFailure> {
        let mut state = self.0.lock().map_err(|_| CallFailure::AnswerLost)?;
        *state = match *state {
            RunState::Active(incarnation) if incarnation.value() > floor => {
                RunState::Active(incarnation)
            }
            RunState::Active(_) => RunState::Unacquired { floor },
            RunState::Unacquired { floor: previous } => RunState::Unacquired {
                floor: previous.max(floor),
            },
            RunState::Acquiring { floor: previous } => RunState::Acquiring {
                floor: previous.max(floor),
            },
        };
        Ok(())
    }
}
impl IncarnationAcquisition {
    pub fn activate(
        self,
        incarnation: RegistryIncarnation,
    ) -> Result<RegistryIncarnation, CallFailure> {
        let mut state = self.state.lock().map_err(|_| CallFailure::AnswerLost)?;
        let RunState::Acquiring { floor } = *state else {
            return Err(CallFailure::AnswerLost);
        };
        if incarnation.value() <= floor {
            return Err(CallFailure::AnswerLost);
        }
        *state = RunState::Active(incarnation);
        Ok(incarnation)
    }
}
impl Drop for IncarnationAcquisition {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock()
            && let RunState::Acquiring { floor } = *state
        {
            *state = RunState::Unacquired { floor };
        }
    }
}
