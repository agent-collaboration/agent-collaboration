//! Frozen handover bytes enter through ordinary authenticated machine sessions.
use crate::{ConformanceClient, ConformanceResult, automation_catalog_scenarios::open_machine};
mod carried_cases;
mod external_due_case;
mod fixtures;
mod lifecycle_cases;
mod occupancy_cases;
mod receipt_cases;
mod repair_cases;
mod restart_cases;
mod validation_cases;
mod wake_cases;
pub use carried_cases::run_automation_handover_carried_conformance;
pub use external_due_case::run_automation_handover_external_due_conformance;
pub use lifecycle_cases::run_automation_handover_lifecycle_conformance;
pub use occupancy_cases::run_automation_handover_external_conformance;
pub use repair_cases::run_automation_handover_repair_conformance;
pub use restart_cases::{
    AutomationHandoverRestartCase, prepare_automation_handover_restart,
    resume_automation_handover_restart,
};
pub use validation_cases::run_automation_handover_validation_conformance;
pub use wake_cases::run_automation_handover_wake_conformance;

pub async fn run_automation_handover_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let importer = durable_steps::Uuid::now_v7().to_string();
    let foreign = durable_steps::Uuid::now_v7().to_string();
    let mut first = open_machine(client, &importer).await?;
    let mut second = match open_machine(client, &foreign).await {
        Ok(second) => second,
        Err(error) => {
            first.close().await?;
            return Err(error);
        }
    };
    let proof = receipt_cases::run(client, &mut first, &mut second, &importer, &foreign).await;
    let first_closed = first.close().await;
    let second_closed = second.close().await;
    proof?;
    first_closed?;
    second_closed?;
    Ok(())
}
