//! Arm existing per-object faults before any startup discovery can run.
use crate::object_keys::address_key;
use collaboration_central_conformance::{RuntimeControl, RuntimeControlRequest, RuntimeFaults};
use durable_steps::StoreError;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
pub(crate) struct PreparedRuntimeControls {
    faults: BTreeMap<String, Arc<RuntimeFaults>>,
    #[cfg(debug_assertions)]
    pub(crate) clock: Option<Arc<crate::native_objects::verification_clock::VerificationClock>>,
}

impl PreparedRuntimeControls {
    pub(crate) async fn prepare(
        requests: Vec<RuntimeControlRequest>,
        root: &std::path::Path,
    ) -> Result<Self, StoreError> {
        #[cfg(not(debug_assertions))]
        let _ = root;
        #[cfg(debug_assertions)]
        let mut initial_clock = None;
        // Reject the whole profile before creating stores or arming any fault.
        for request in &requests {
            request.validate().map_err(StoreError::backend)?;
            match request.command {
                #[cfg(debug_assertions)]
                RuntimeControl::AutomationClock(
                    collaboration_central_conformance::AutomationClockControl::Start { now },
                ) => {
                    if initial_clock.replace(now).is_some() {
                        return Err(StoreError::backend(
                            "only one verification clock may be configured",
                        ));
                    }
                }
                #[cfg(debug_assertions)]
                RuntimeControl::AutomationClock(_) => {
                    return Err(StoreError::backend("verification startup requires Start"));
                }
                RuntimeControl::SetTime { .. }
                | RuntimeControl::SmallMailboxCapacity
                | RuntimeControl::CopyFull { .. }
                | RuntimeControl::DropNextAnswer { .. }
                | RuntimeControl::HoldNextDispatch { .. }
                | RuntimeControl::HoldNextSelectedDispatch { .. }
                | RuntimeControl::DropNextObjectReply { .. }
                | RuntimeControl::DropNextSelectedObjectReply { .. } => {}
                RuntimeControl::AwaitDroppedAnswer { .. }
                | RuntimeControl::AwaitHeldDispatch
                | RuntimeControl::ResumeDispatch
                | RuntimeControl::ResumeDispatchAndHoldNext { .. }
                | RuntimeControl::ResumeDispatchAndHoldNextSelected { .. }
                | RuntimeControl::AwaitDroppedObjectReply
                | RuntimeControl::DropHeldDispatchReply
                | RuntimeControl::Observe => {
                    return Err(StoreError::backend(
                        "native conformance startup controls must only arm faults",
                    ));
                }
                #[cfg(debug_assertions)]
                RuntimeControl::AutomationStoreFailures { .. } => {
                    return Err(StoreError::backend(
                        "automation storage failures require a live initialized Fleet",
                    ));
                }
            }
        }
        let mut prepared = Self::default();
        #[cfg(debug_assertions)]
        if let Some(now) = initial_clock {
            if requests
                .iter()
                .any(|request| matches!(request.command, RuntimeControl::SetTime { .. }))
            {
                return Err(StoreError::backend(
                    "SetTime cannot override a verification clock",
                ));
            }
            prepared.clock = Some(
                crate::native_objects::verification_clock::VerificationClock::open(root, now)
                    .await?,
            );
        }
        for request in requests {
            #[cfg(debug_assertions)]
            if matches!(request.command, RuntimeControl::AutomationClock(_)) {
                continue;
            }
            let key = address_key(&request.address)?;
            let faults = prepared.faults.entry(key).or_default();
            faults
                .control(request.command)
                .await
                .map_err(StoreError::backend)?;
        }
        Ok(prepared)
    }

    pub(crate) fn faults_for(&self, object_key: &str) -> Arc<RuntimeFaults> {
        self.faults.get(object_key).cloned().unwrap_or_default()
    }
}
