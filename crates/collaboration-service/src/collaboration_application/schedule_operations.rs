//! Native preparation selects the existing Local or central command owner.
use super::AutomationOperations;
use collaboration_protocol::{SchedulePrepareRequest, ScheduleSnapshot};

impl AutomationOperations {
    /// Prepares the schedule's reuse-thread destination; a retry returns the retained outcome.
    pub async fn schedule_prepare(
        &self,
        request: SchedulePrepareRequest,
    ) -> Result<ScheduleSnapshot, crate::ScheduleCommandFailure> {
        if self
            .schedule_preparation_owner(&request.operation_id, &request.schedule_id)
            .await?
            == automation_storage::StoredAutomationOwner::Central
        {
            return self.prepare_central_schedule(request).await;
        }
        crate::schedule_preparation::prepare_schedule(
            &self.configuration,
            self.identity.service_id(),
            self.scheduled_run_execution.as_ref(),
            self.automation.as_ref(),
            request,
        )
        .await
        .map_err(Into::into)
    }
}
