//! Finish every held invocation owned by one fixture's physical control source.
use crate::{DispatchFaultObservation, FixtureResult, ObjectAddress, RuntimeControl, TestCentral};

impl TestCentral {
    pub async fn release_held_dispatches(&self, address: ObjectAddress) -> FixtureResult {
        let observed = self
            .runtime_control(address.clone(), RuntimeControl::Observe)
            .await?;
        let mut failures = Vec::new();
        for entry in observed.dispatches {
            if matches!(entry.state, DispatchFaultObservation::Held { .. })
                && let Err(error) = self
                    .runtime_control(
                        address.clone(),
                        RuntimeControl::ResumeDispatch {
                            key: entry.key.clone(),
                        },
                    )
                    .await
            {
                failures.push(format!("{:?}: {error}", entry.key));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}
