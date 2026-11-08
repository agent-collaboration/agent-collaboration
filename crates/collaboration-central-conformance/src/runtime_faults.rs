//! Ephemeral fault state belongs to one runtime object, never to the core rules.

use crate::runtime_controls::{
    AnswerFaultObservation, ClockObservation, DispatchKey, ObjectReplyFaultObservation,
    RuntimeControl, RuntimeControlFailure, RuntimeFaultObservation, StorageFaultObservation,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use fleet_sync_protocol::{ConnectionId, FrameId};
use std::sync::{Arc, Mutex, MutexGuard};

#[path = "runtime_faults/session_faults.rs"]
mod session_faults;
pub use session_faults::{SessionFaultControl, SessionFaultObservation};
mod dispatch_holds;
mod signal;
use signal::Signal;
mod selected_calls;
use dispatch_holds::DispatchHolds;
use selected_calls::DispatchSelection;
#[cfg(all(test, feature = "client"))]
mod connection_drop_test;
#[cfg(all(test, feature = "client"))]
mod effect_selector_test;
#[cfg(all(test, feature = "client"))]
mod keyed_dispatch_test;
#[cfg(all(test, feature = "client"))]
mod selected_call_tests;

#[derive(Default)]
enum AnswerFault {
    #[default]
    Idle,
    Armed {
        connection: ConnectionId,
        frame: FrameId,
        observed: Arc<Signal>,
    },
    Dropped {
        connection: ConnectionId,
        frame: FrameId,
        observed: Arc<Signal>,
    },
}
#[derive(Default)]
enum ObjectReplyFault {
    #[default]
    Idle,
    Armed {
        destination: ObjectAddress,
        selection: DispatchSelection,
        observed: Arc<Signal>,
    },
    Dropped {
        destination: ObjectAddress,
        request: Vec<u8>,
        observed: Arc<Signal>,
    },
}
struct FaultState {
    clock: Option<RegistryTime>,
    storage: StorageFaultObservation,
    answer: AnswerFault,
    dispatch: DispatchHolds,
    object_reply: ObjectReplyFault,
    session: session_faults::SessionFaultState,
    organization_record: bool,
}
impl Default for FaultState {
    fn default() -> Self {
        Self {
            clock: None,
            storage: StorageFaultObservation::Idle,
            answer: AnswerFault::Idle,
            dispatch: DispatchHolds::default(),
            object_reply: ObjectReplyFault::Idle,
            session: Default::default(),
            organization_record: false,
        }
    }
}

#[derive(Default)]
pub struct RuntimeFaults(Mutex<FaultState>);
#[cfg(test)]
#[path = "runtime_faults_test.rs"]
mod tests;
impl RuntimeFaults {
    fn state(&self) -> Result<MutexGuard<'_, FaultState>, RuntimeControlFailure> {
        self.0.lock().map_err(|_| RuntimeControlFailure::Poisoned)
    }
    pub fn clock(&self) -> Option<RegistryTime> {
        self.0.lock().ok().and_then(|state| state.clock)
    }
    pub fn restore_clock(&self, now: Option<RegistryTime>) -> Result<(), RuntimeControlFailure> {
        self.state()?.clock = now;
        Ok(())
    }
    pub fn storage_armed(&self) -> Result<bool, RuntimeControlFailure> {
        Ok(matches!(
            self.state()?.storage,
            StorageFaultObservation::Armed { .. } | StorageFaultObservation::Unavailable
        ))
    }
    /// Consume an armed provisioning record fault, if any.
    pub fn take_organization_record_fault(&self) -> Result<bool, RuntimeControlFailure> {
        Ok(std::mem::take(&mut self.state()?.organization_record))
    }
    pub fn project_unavailable(&self) -> Result<bool, RuntimeControlFailure> {
        Ok(matches!(
            self.state()?.storage,
            StorageFaultObservation::Unavailable
        ))
    }
    pub fn restore_project_fault(&self, unavailable: bool) -> Result<(), RuntimeControlFailure> {
        let mut state = self.state()?;
        if unavailable {
            state.storage = StorageFaultObservation::Unavailable;
        } else if matches!(state.storage, StorageFaultObservation::Unavailable) {
            state.storage = StorageFaultObservation::Idle;
        }
        Ok(())
    }
    /// A late callback failure follows real writes and is rolled back by the real store.
    /// `written` is the measured SQLite total_changes delta, not an invented count.
    pub fn fail_after_writes(&self, written: u64) -> Result<bool, RuntimeControlFailure> {
        let mut state = self.state()?;
        match state.storage {
            StorageFaultObservation::Unavailable => Ok(true),
            StorageFaultObservation::Armed { after_writes } if written >= after_writes.get() => {
                state.storage = StorageFaultObservation::Triggered {
                    after_writes,
                    written,
                };
                Ok(true)
            }
            StorageFaultObservation::Idle
            | StorageFaultObservation::Armed { .. }
            | StorageFaultObservation::Triggered { .. } => Ok(false),
        }
    }
    pub fn drop_answer(
        &self,
        connection: ConnectionId,
        frame: FrameId,
    ) -> Result<bool, RuntimeControlFailure> {
        let observed = {
            let mut state = self.state()?;
            match &state.answer {
                AnswerFault::Armed {
                    connection: expected_connection,
                    frame: expected,
                    observed,
                } if *expected_connection == connection && *expected == frame => {
                    let observed = observed.clone();
                    state.answer = AnswerFault::Dropped {
                        connection,
                        frame,
                        observed: observed.clone(),
                    };
                    Some(observed)
                }
                AnswerFault::Idle | AnswerFault::Armed { .. } | AnswerFault::Dropped { .. } => None,
            }
        };
        match observed {
            Some(observed) => {
                observed.signal()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
    pub async fn before_dispatch(
        &self,
        destination: &ObjectAddress,
        request: &[u8],
    ) -> Result<(), RuntimeControlFailure> {
        let held = self
            .state()?
            .dispatch
            .before_dispatch(destination, request)?;
        if let Some(signals) = held {
            signals.reached.signal()?;
            signals.release.wait().await?;
        }
        Ok(())
    }

    /// Called only after transport returned a real successful object reply.
    pub fn drop_successful_object_reply(
        &self,
        destination: &ObjectAddress,
        request: &[u8],
    ) -> Result<bool, RuntimeControlFailure> {
        let observed = {
            let mut state = self.state()?;
            match &state.object_reply {
                ObjectReplyFault::Armed {
                    destination: expected,
                    selection,
                    observed,
                } if expected == destination && selection.matches(request) => {
                    let observed = observed.clone();
                    state.object_reply = ObjectReplyFault::Dropped {
                        destination: destination.clone(),
                        request: request.to_vec(),
                        observed: observed.clone(),
                    };
                    Some(observed)
                }
                ObjectReplyFault::Idle
                | ObjectReplyFault::Armed { .. }
                | ObjectReplyFault::Dropped { .. } => None,
            }
        };
        match observed {
            Some(observed) => {
                observed.signal()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
    pub fn observe(&self) -> Result<RuntimeFaultObservation, RuntimeControlFailure> {
        let state = self.state()?;
        Ok(RuntimeFaultObservation {
            clock: match state.clock {
                Some(now) => ClockObservation::Pinned { now },
                None => ClockObservation::Wall,
            },
            storage: state.storage.clone(),
            answer: match &state.answer {
                AnswerFault::Idle => AnswerFaultObservation::Idle,
                AnswerFault::Armed {
                    connection, frame, ..
                } => AnswerFaultObservation::Armed {
                    connection: *connection,
                    frame: *frame,
                },
                AnswerFault::Dropped {
                    connection, frame, ..
                } => AnswerFaultObservation::Dropped {
                    connection: *connection,
                    frame: *frame,
                },
            },
            dispatches: state.dispatch.observe(),
            object_reply: match &state.object_reply {
                ObjectReplyFault::Idle => ObjectReplyFaultObservation::Idle,
                ObjectReplyFault::Armed { destination, .. } => ObjectReplyFaultObservation::Armed {
                    destination: destination.clone(),
                },
                ObjectReplyFault::Dropped {
                    destination,
                    request,
                    ..
                } => ObjectReplyFaultObservation::Dropped {
                    destination: destination.clone(),
                    request: request.clone(),
                },
            },
        })
    }
    pub async fn control(
        &self,
        command: RuntimeControl,
    ) -> Result<RuntimeFaultObservation, RuntimeControlFailure> {
        match command {
            RuntimeControl::ProjectUnavailable => {
                self.state()?.storage = StorageFaultObservation::Unavailable
            }
            RuntimeControl::ClearProjectFault => {
                self.state()?.storage = StorageFaultObservation::Idle
            }
            RuntimeControl::SetTime { now } => self.state()?.clock = Some(now),
            RuntimeControl::CopyFull { after_writes } => {
                let mut state = self.state()?;
                if matches!(state.storage, StorageFaultObservation::Unavailable) {
                    return Err(RuntimeControlFailure::AlreadyArmed);
                }
                state.storage = StorageFaultObservation::Armed { after_writes };
            }
            RuntimeControl::DropNextAnswer { connection, frame } => {
                let mut state = self.state()?;
                if matches!(state.answer, AnswerFault::Armed { .. }) {
                    return Err(RuntimeControlFailure::AlreadyArmed);
                }
                state.answer = AnswerFault::Armed {
                    connection,
                    frame,
                    observed: Arc::default(),
                };
            }
            RuntimeControl::AwaitDroppedAnswer { connection, frame } => {
                let observed = match &self.state()?.answer {
                    AnswerFault::Armed {
                        connection: expected_connection,
                        frame: expected,
                        observed,
                    }
                    | AnswerFault::Dropped {
                        connection: expected_connection,
                        frame: expected,
                        observed,
                    } if *expected_connection == connection && *expected == frame => {
                        observed.clone()
                    }
                    AnswerFault::Idle => return Err(RuntimeControlFailure::NotArmed),
                    AnswerFault::Armed { .. } | AnswerFault::Dropped { .. } => {
                        return Err(RuntimeControlFailure::DifferentFrame);
                    }
                };
                observed.wait().await?;
            }
            RuntimeControl::HoldNextDispatch { key } => {
                self.state()?.dispatch.arm(key)?;
            }
            RuntimeControl::AwaitHeldDispatch { key } => {
                let reached = self.state()?.dispatch.reached(&key)?;
                reached.wait().await?;
            }
            RuntimeControl::ResumeDispatch { key } => {
                let release = self.state()?.dispatch.resume(&key, None)?;
                release.signal()?;
            }
            RuntimeControl::DropNextObjectReply { destination } => {
                self.arm_object_reply(destination, DispatchSelection::Any)?;
            }
            RuntimeControl::DropNextSelectedObjectReply { destination, call } => {
                self.arm_object_reply(destination, DispatchSelection::Call(call))?;
            }
            RuntimeControl::DropHeldDispatchReply { key } => self.arm_held_reply(key)?,
            RuntimeControl::ResumeDispatchAndHoldNext { key, next } => {
                let release = self.state()?.dispatch.resume(&key, Some(next))?;
                release.signal()?;
            }
            RuntimeControl::AwaitDroppedObjectReply => {
                let observed = match &self.state()?.object_reply {
                    ObjectReplyFault::Armed { observed, .. }
                    | ObjectReplyFault::Dropped { observed, .. } => observed.clone(),
                    ObjectReplyFault::Idle => return Err(RuntimeControlFailure::NotArmed),
                };
                observed.wait().await?;
            }
            RuntimeControl::FailNextOrganizationRecord => {
                self.state()?.organization_record = true;
            }
            RuntimeControl::Observe => {}
        }
        self.observe()
    }

    fn arm_object_reply(
        &self,
        destination: ObjectAddress,
        selection: DispatchSelection,
    ) -> Result<(), RuntimeControlFailure> {
        let mut state = self.state()?;
        if matches!(state.object_reply, ObjectReplyFault::Armed { .. }) {
            return Err(RuntimeControlFailure::AlreadyArmed);
        }
        state.object_reply = ObjectReplyFault::Armed {
            destination,
            selection,
            observed: Arc::default(),
        };
        Ok(())
    }

    fn arm_held_reply(&self, key: DispatchKey) -> Result<(), RuntimeControlFailure> {
        let mut state = self.state()?;
        let request = state.dispatch.held_request(&key)?;
        if matches!(state.object_reply, ObjectReplyFault::Armed { .. }) {
            return Err(RuntimeControlFailure::AlreadyArmed);
        }
        // Capture both under the fault owner's lock; a later bypassing call
        // cannot consume the lost reply even when its invocation kind matches.
        state.object_reply = ObjectReplyFault::Armed {
            destination: key.destination,
            selection: DispatchSelection::ExactRequest(request),
            observed: Arc::default(),
        };
        Ok(())
    }
}
