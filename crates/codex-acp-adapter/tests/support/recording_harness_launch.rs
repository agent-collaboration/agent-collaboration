//! Event-controlled registrar port fixture; no clocks or transport stand-ins.
use collaboration_contracts::{
    HarnessLaunchFailure, HarnessLaunchFuture, HarnessLocalSetupRefusal,
    HarnessRegistrationOutcome, HarnessRestoreAdmission, HarnessSessionBinding,
    HarnessSessionLaunchPort, HarnessStartAdmission,
};
use collaboration_protocol::OperationId;
use message_board::SessionRef;
use std::sync::Mutex;

pub(crate) struct RecordingLaunch {
    pub(crate) binding: Option<HarnessSessionBinding>,
    pub(crate) completion: HarnessRegistrationOutcome,
    pub(crate) events: Mutex<Vec<&'static str>>,
    pub(crate) created: Mutex<Vec<SessionRef>>,
    pub(crate) refused: Mutex<Vec<HarnessLocalSetupRefusal>>,
}
impl Default for RecordingLaunch {
    fn default() -> Self {
        Self {
            binding: None,
            completion: HarnessRegistrationOutcome::Registered,
            events: Mutex::new(Vec::new()),
            created: Mutex::new(Vec::new()),
            refused: Mutex::new(Vec::new()),
        }
    }
}
impl RecordingLaunch {
    pub(crate) fn event(&self, event: &'static str) -> Result<(), HarnessLaunchFailure> {
        self.events
            .lock()
            .map_err(|_| HarnessLaunchFailure::Unavailable)?
            .push(event);
        Ok(())
    }
}
impl HarnessSessionLaunchPort for RecordingLaunch {
    fn prepare_start<'a>(
        &'a self,
        _operation: &'a OperationId,
        _declared: &'a collaboration_contracts::HarnessDeclaredLaunchInput,
    ) -> HarnessLaunchFuture<'a, HarnessStartAdmission> {
        Box::pin(async move {
            match &self.binding {
                Some(binding) => {
                    self.event("prepared")?;
                    Ok(HarnessStartAdmission::Prepared(binding.clone()))
                }
                None => Err(HarnessLaunchFailure::AlreadyPrepared),
            }
        })
    }
    fn mark_dispatched<'a>(&'a self, _operation: &'a OperationId) -> HarnessLaunchFuture<'a, ()> {
        Box::pin(async move { self.event("dispatched") })
    }
    fn record_created<'a>(
        &'a self,
        _operation: &'a OperationId,
        session: &'a SessionRef,
    ) -> HarnessLaunchFuture<'a, ()> {
        Box::pin(async move {
            self.created
                .lock()
                .map_err(|_| HarnessLaunchFailure::Unavailable)?
                .push(session.clone());
            self.event("created")
        })
    }
    fn creation_refused<'a>(&'a self, _operation: &'a OperationId) -> HarnessLaunchFuture<'a, ()> {
        Box::pin(async move { self.event("notCreated") })
    }
    fn complete_setup<'a>(
        &'a self,
        _operation: &'a OperationId,
    ) -> HarnessLaunchFuture<'a, HarnessRegistrationOutcome> {
        Box::pin(async move {
            self.event(match &self.completion {
                HarnessRegistrationOutcome::Registered => "registered",
                HarnessRegistrationOutcome::Pending => "registrationPending",
                HarnessRegistrationOutcome::Refused(_) => "registrationRefused",
            })?;
            Ok(self.completion.clone())
        })
    }
    fn refuse_setup<'a>(
        &'a self,
        _operation: &'a OperationId,
        cause: HarnessLocalSetupRefusal,
    ) -> HarnessLaunchFuture<'a, ()> {
        Box::pin(async move {
            self.refused
                .lock()
                .map_err(|_| HarnessLaunchFailure::Unavailable)?
                .push(cause);
            self.event("setupRefused")
        })
    }
    fn binding_for_restore<'a>(
        &'a self,
        _session: &'a SessionRef,
    ) -> HarnessLaunchFuture<'a, HarnessRestoreAdmission> {
        Box::pin(async { Err(HarnessLaunchFailure::InactiveBinding) })
    }
}
