//! Runtime prefix ownership uses real central creation; due work remains held on the fixture clock.
use super::{RuntimeCleanupFixture, ensure};
use crate::proof_context::ProofResult;
use collaboration_client::protocol::{
    AutomationConfigureRequest, DestinationPreparation, InstructionCreateParams, OperationId,
    ScheduleCreateRequest, ScheduleEnableRequest, SchedulePrepareRequest, WakeSendRequest,
    WakeState,
};
use serde_json::json;
impl RuntimeCleanupFixture {
    pub(super) async fn create_wake(&mut self) -> ProofResult<()> {
        let target = self
            .peer
            .as_ref()
            .ok_or("peer was not retained")?
            .target
            .clone();
        let owned = self
            .owned
            .as_mut()
            .ok_or("actual cleanup client was not retained")?;
        let operation_id = OperationId::generate();
        owned
            .resources
            .record_wake_submission(operation_id.clone())?;
        let request: WakeSendRequest = serde_json::from_value(json!({
            "operationId":operation_id,"requestedBy":{"kind":"owner"},"replace":false,"message":{"target":target,
                "content":{"kind":"humanUser","text":"Synthetic Runtime cleanup wake"},
                "delivery":"auto","generationGuard":null},
            "timing":{"kind":"at","at":"2030-01-01T01:00:00Z"},"expiry":{"kind":"after","seconds":7200}
        }))?;
        let created = owned.client.send_wakeup(request).await?;
        let collaboration_client::WakeCreateResponse::Central { value: created } = created else {
            return Err("new Runtime cleanup Wake changed to Local".into());
        };
        owned
            .resources
            .record_wake_receipt(&operation_id, created.definition.wakeup_id.clone())?;
        self.expected.wake = Some((operation_id, created.definition.wakeup_id));
        ensure(
            created.state == WakeState::Active
                && created.next_due_at.is_some()
                && created.first_firing.is_none(),
            "real wake was not active and unfired before cleanup",
        )
    }

    pub(super) async fn create_schedule(&mut self) -> ProofResult<()> {
        let owned = self
            .owned
            .as_mut()
            .ok_or("actual cleanup client was not retained")?;
        let instruction_operation = OperationId::generate();
        owned
            .resources
            .record_instruction_submission(instruction_operation.clone())?;
        let instruction = owned
            .client
            .create_instruction(InstructionCreateParams {
                operation_id: instruction_operation.clone(),
                text: "Synthetic Runtime schedule".to_owned().try_into()?,
            })
            .await?;
        owned.resources.record_instruction_receipt(
            &instruction_operation,
            instruction.instruction_id.clone(),
        )?;
        self.expected.instruction = Some(instruction.instruction_id.clone());
        let operation_id = OperationId::generate();
        owned
            .resources
            .record_schedule_submission(operation_id.clone())?;
        let request: ScheduleCreateRequest = serde_json::from_value(json!({
            "operationId":operation_id,"definition":{"instructionId":instruction.instruction_id,
                "timing":{"kind":"at","at":"2030-01-01T01:00:00Z"},"enabled":false,
                "destination":{"kind":"unprepared"},"executionTimeoutSeconds":120,
                "model":"fixture-model","effort":"medium"}
        }))?;
        let created = owned.client.create_schedule(request).await?;
        owned
            .resources
            .record_schedule_receipt(&operation_id, created.schedule_id.clone())?;
        self.expected.schedule = Some((operation_id, created.schedule_id));
        ensure(
            !created.definition.enabled && created.next_due_at.is_none(),
            "real schedule was not disabled before preparation",
        )
    }

    pub(super) async fn prepare_and_enable(&self) -> ProofResult<()> {
        let owned = self
            .owned
            .as_ref()
            .ok_or("actual cleanup client was not retained")?;
        let (_, schedule_id) = self
            .expected
            .schedule
            .as_ref()
            .ok_or("actual schedule receipt missing")?;
        let target = self
            .peer
            .as_ref()
            .ok_or("peer was not retained")?
            .target
            .clone();
        owned
            .client
            .configure_automation(AutomationConfigureRequest {
                operation_id: OperationId::generate(),
                execution_timeout_seconds: 120.try_into()?,
                summary_timeout_seconds: 30.try_into()?,
            })
            .await?;
        owned
            .client
            .prepare_schedule(SchedulePrepareRequest {
                operation_id: OperationId::generate(),
                schedule_id: schedule_id.clone(),
                destination: DestinationPreparation::Existing {
                    target,
                    cwd: self.directory.display().to_string(),
                },
            })
            .await?;
        let enabled = owned
            .client
            .enable_schedule(ScheduleEnableRequest {
                operation_id: OperationId::generate(),
                schedule_id: schedule_id.clone(),
            })
            .await?;
        let collaboration_client::ScheduleMutationResponse::Central(enabled) = enabled else {
            return Err("new Runtime Schedule enable changed to Local".into());
        };
        let collaboration_client::CatalogAutomationResult::Schedule(enabled) = enabled.automation
        else {
            return Err("Runtime enable returned another automation kind".into());
        };
        ensure(
            enabled.schedule_id == *schedule_id
                && enabled.definition.enabled
                && enabled.next_due_at.is_some(),
            "real peer schedule did not become enabled before cleanup",
        )
    }
}
