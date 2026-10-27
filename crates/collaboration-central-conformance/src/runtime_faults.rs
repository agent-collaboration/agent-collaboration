//! Ephemeral fault state belongs to one runtime object, never to the core rules.

use crate::runtime_controls::*;
use collaboration_central_core::{ObjectAddress, RegistryTime};
use fleet_sync_protocol::FrameId;
use std::{
    future::poll_fn,
    sync::{Arc, Mutex, MutexGuard},
    task::{Poll, Waker},
};

#[path = "runtime_faults/session_faults.rs"]
mod session_faults;
pub use session_faults::{SessionFaultControl, SessionFaultObservation};
mod selected_calls;
use selected_calls::DispatchSelection;
#[cfg(all(test, feature = "client"))]
mod selected_call_tests;

#[derive(Default)]
struct SignalState {
    signalled: bool,
    waiters: Vec<Waker>,
}
#[derive(Default)]
struct Signal(Mutex<SignalState>);
impl Signal {
    fn signal(&self) -> Result<(), RuntimeControlFailure> {
        let waiters = {
            let mut state = self.0.lock().map_err(|_| RuntimeControlFailure::Poisoned)?;
            state.signalled = true;
            std::mem::take(&mut state.waiters)
        };
        for waiter in waiters {
            waiter.wake();
        }
        Ok(())
    }
    async fn wait(&self) -> Result<(), RuntimeControlFailure> {
        poll_fn(|context| {
            let Ok(mut state) = self.0.lock() else {
                return Poll::Ready(Err(RuntimeControlFailure::Poisoned));
            };
            if state.signalled {
                return Poll::Ready(Ok(()));
            }
            if !state
                .waiters
                .iter()
                .any(|waiter| waiter.will_wake(context.waker()))
            {
                state.waiters.push(context.waker().clone());
            }
            Poll::Pending
        })
        .await
    }
}

#[derive(Default)]
enum AnswerFault {
    #[default]
    Idle,
    Armed {
        frame: FrameId,
        observed: Arc<Signal>,
    },
    Dropped {
        frame: FrameId,
        observed: Arc<Signal>,
    },
}
#[derive(Default)]
enum DispatchFault {
    #[default]
    Idle,
    Armed {
        destination: ObjectAddress,
        selection: DispatchSelection,
        reached: Arc<Signal>,
        release: Arc<Signal>,
    },
    Held {
        destination: ObjectAddress,
        request: Vec<u8>,
        reached: Arc<Signal>,
        release: Arc<Signal>,
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
    small_mailbox_capacity: bool,
    storage: StorageFaultObservation,
    answer: AnswerFault,
    dispatch: DispatchFault,
    object_reply: ObjectReplyFault,
    session: session_faults::SessionFaultState,
}
impl Default for FaultState {
    fn default() -> Self {
        Self {
            clock: None,
            small_mailbox_capacity: false,
            storage: StorageFaultObservation::Idle,
            answer: AnswerFault::Idle,
            dispatch: DispatchFault::Idle,
            object_reply: ObjectReplyFault::Idle,
            session: Default::default(),
        }
    }
}

#[derive(Default)]
pub struct RuntimeFaults(Mutex<FaultState>);
impl RuntimeFaults {
    pub fn small_mailbox_capacity(&self) -> Result<bool, RuntimeControlFailure> {
        Ok(self.state()?.small_mailbox_capacity)
    }
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
            StorageFaultObservation::Armed { .. }
        ))
    }
    /// A late callback failure follows real writes and is rolled back by the real store.
    /// `written` is the measured SQLite total_changes delta, not an invented count.
    pub fn fail_after_writes(&self, written: u64) -> Result<bool, RuntimeControlFailure> {
        let mut state = self.state()?;
        match state.storage {
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
    pub fn drop_answer(&self, frame: FrameId) -> Result<bool, RuntimeControlFailure> {
        let observed = {
            let mut state = self.state()?;
            match &state.answer {
                AnswerFault::Armed {
                    frame: expected,
                    observed,
                } if *expected == frame => {
                    let observed = observed.clone();
                    state.answer = AnswerFault::Dropped {
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
        let held = {
            let mut state = self.state()?;
            match &state.dispatch {
                DispatchFault::Armed {
                    destination: expected,
                    selection,
                    reached,
                    release,
                } if expected == destination && selection.matches(request) => {
                    let reached = reached.clone();
                    let release = release.clone();
                    state.dispatch = DispatchFault::Held {
                        destination: destination.clone(),
                        request: request.to_vec(),
                        reached: reached.clone(),
                        release: release.clone(),
                    };
                    Some((reached, release))
                }
                DispatchFault::Idle | DispatchFault::Armed { .. } | DispatchFault::Held { .. } => {
                    None
                }
            }
        };
        if let Some((reached, release)) = held {
            reached.signal()?;
            release.wait().await?;
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
                AnswerFault::Armed { frame, .. } => AnswerFaultObservation::Armed { frame: *frame },
                AnswerFault::Dropped { frame, .. } => {
                    AnswerFaultObservation::Dropped { frame: *frame }
                }
            },
            dispatch: match &state.dispatch {
                DispatchFault::Idle => DispatchFaultObservation::Idle,
                DispatchFault::Armed { destination, .. } => DispatchFaultObservation::Armed {
                    destination: destination.clone(),
                },
                DispatchFault::Held {
                    destination,
                    request,
                    ..
                } => DispatchFaultObservation::Held {
                    destination: destination.clone(),
                    request: request.clone(),
                },
            },
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
            // Startup and live controls select the same fixed quota at the platform owner.
            RuntimeControl::SmallMailboxCapacity => self.state()?.small_mailbox_capacity = true,
            RuntimeControl::SetTime { now } => self.state()?.clock = Some(now),
            RuntimeControl::CopyFull { after_writes } => {
                self.state()?.storage = StorageFaultObservation::Armed { after_writes };
            }
            RuntimeControl::DropNextAnswer { frame } => {
                let mut state = self.state()?;
                if matches!(state.answer, AnswerFault::Armed { .. }) {
                    return Err(RuntimeControlFailure::AlreadyArmed);
                }
                state.answer = AnswerFault::Armed {
                    frame,
                    observed: Arc::default(),
                };
            }
            RuntimeControl::AwaitDroppedAnswer { frame } => {
                let observed = match &self.state()?.answer {
                    AnswerFault::Armed {
                        frame: expected,
                        observed,
                    }
                    | AnswerFault::Dropped {
                        frame: expected,
                        observed,
                    } if *expected == frame => observed.clone(),
                    AnswerFault::Idle => return Err(RuntimeControlFailure::NotArmed),
                    AnswerFault::Armed { .. } | AnswerFault::Dropped { .. } => {
                        return Err(RuntimeControlFailure::DifferentFrame);
                    }
                };
                observed.wait().await?;
            }
            RuntimeControl::HoldNextDispatch { destination } => {
                self.arm_dispatch(destination, DispatchSelection::Any)?;
            }
            RuntimeControl::HoldNextSelectedDispatch { destination, call } => {
                self.arm_dispatch(destination, DispatchSelection::Call(call))?;
            }
            RuntimeControl::AwaitHeldDispatch => {
                let reached = match &self.state()?.dispatch {
                    DispatchFault::Armed { reached, .. } | DispatchFault::Held { reached, .. } => {
                        reached.clone()
                    }
                    DispatchFault::Idle => return Err(RuntimeControlFailure::NotArmed),
                };
                reached.wait().await?;
            }
            RuntimeControl::ResumeDispatch => {
                let release = {
                    let mut state = self.state()?;
                    match &state.dispatch {
                        DispatchFault::Held { release, .. } => {
                            let release = release.clone();
                            state.dispatch = DispatchFault::Idle;
                            release
                        }
                        DispatchFault::Idle | DispatchFault::Armed { .. } => {
                            return Err(RuntimeControlFailure::NotHeld);
                        }
                    }
                };
                release.signal()?;
            }
            RuntimeControl::DropNextObjectReply { destination } => {
                self.arm_object_reply(destination, DispatchSelection::Any)?;
            }
            RuntimeControl::DropNextSelectedObjectReply { destination, call } => {
                self.arm_object_reply(destination, DispatchSelection::Call(call))?;
            }
            RuntimeControl::DropHeldDispatchReply => self.arm_held_reply()?,
            RuntimeControl::ResumeDispatchAndHoldNext { destination } => {
                self.resume_and_hold(destination, DispatchSelection::Any)?;
            }
            RuntimeControl::ResumeDispatchAndHoldNextSelected { destination, call } => {
                self.resume_and_hold(destination, DispatchSelection::Call(call))?;
            }
            RuntimeControl::AwaitDroppedObjectReply => {
                let observed = match &self.state()?.object_reply {
                    ObjectReplyFault::Armed { observed, .. }
                    | ObjectReplyFault::Dropped { observed, .. } => observed.clone(),
                    ObjectReplyFault::Idle => return Err(RuntimeControlFailure::NotArmed),
                };
                observed.wait().await?;
            }
            RuntimeControl::Observe => {}
        }
        self.observe()
    }

    fn arm_dispatch(
        &self,
        destination: ObjectAddress,
        selection: DispatchSelection,
    ) -> Result<(), RuntimeControlFailure> {
        let mut state = self.state()?;
        if !matches!(state.dispatch, DispatchFault::Idle) {
            return Err(RuntimeControlFailure::AlreadyArmed);
        }
        state.dispatch = DispatchFault::Armed {
            destination,
            selection,
            reached: Arc::default(),
            release: Arc::default(),
        };
        Ok(())
    }

    fn resume_and_hold(
        &self,
        destination: ObjectAddress,
        selection: DispatchSelection,
    ) -> Result<(), RuntimeControlFailure> {
        let release = {
            let mut state = self.state()?;
            let release = match &state.dispatch {
                DispatchFault::Held { release, .. } => release.clone(),
                DispatchFault::Idle | DispatchFault::Armed { .. } => {
                    return Err(RuntimeControlFailure::NotHeld);
                }
            };
            // Install the next selection before releasing the current caller.
            state.dispatch = DispatchFault::Armed {
                destination,
                selection,
                reached: Arc::default(),
                release: Arc::default(),
            };
            release
        };
        release.signal()
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

    fn arm_held_reply(&self) -> Result<(), RuntimeControlFailure> {
        let mut state = self.state()?;
        let DispatchFault::Held {
            destination,
            request,
            ..
        } = &state.dispatch
        else {
            return Err(RuntimeControlFailure::NotHeld);
        };
        if matches!(state.object_reply, ObjectReplyFault::Armed { .. }) {
            return Err(RuntimeControlFailure::AlreadyArmed);
        }
        // Capture both under the fault owner's lock; a later bypassing call
        // cannot consume the lost reply even when its invocation kind matches.
        state.object_reply = ObjectReplyFault::Armed {
            destination: destination.clone(),
            selection: DispatchSelection::ExactRequest(request.clone()),
            observed: Arc::default(),
        };
        Ok(())
    }
}
