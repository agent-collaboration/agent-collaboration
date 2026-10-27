//! Arm existing per-object faults before any startup discovery can run.
use crate::object_keys::address_key;
use collaboration_central_conformance::{RuntimeControl, RuntimeControlRequest, RuntimeFaults};
use durable_steps::StoreError;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
pub(crate) struct PreparedRuntimeControls(BTreeMap<String, Arc<RuntimeFaults>>);

impl PreparedRuntimeControls {
    pub(crate) async fn prepare(requests: Vec<RuntimeControlRequest>) -> Result<Self, StoreError> {
        // Reject the whole profile before creating stores or arming any fault.
        for request in &requests {
            request.validate().map_err(StoreError::backend)?;
            match request.command {
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
            }
        }
        let mut prepared = Self::default();
        for request in requests {
            let key = address_key(&request.address)?;
            let faults = prepared.0.entry(key).or_default();
            faults
                .control(request.command)
                .await
                .map_err(StoreError::backend)?;
        }
        Ok(prepared)
    }

    pub(crate) fn faults_for(&self, object_key: &str) -> Arc<RuntimeFaults> {
        self.0.get(object_key).cloned().unwrap_or_default()
    }
}
