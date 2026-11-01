//! Driver transitions enqueue their central result inside the driver's existing transaction.
use crate::StorageError;
use collaboration_protocol::{FiringId, FiringOutcome};
use sqlx::SqliteConnection;

pub(crate) async fn report_run_phase(
    connection: &mut SqliteConnection,
    run_id: &agent_automation::RunId,
    phase: &str,
    summary: Option<&str>,
    now_ms: i64,
) -> Result<(), StorageError> {
    let row = sqlx::query!(
        "SELECT firing_id FROM firing_receipts WHERE json_extract(local_work_ref,'$.runId')=?",
        run_id.as_str()
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let firing_id: FiringId = row
        .firing_id
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    let outcome = match phase {
        "executing" => FiringOutcome::RunStarted {
            run_id: run_id.clone(),
        },
        "finished" => {
            let worker = sqlx::query!(
                "SELECT worker_outcome_json FROM workflow_runs WHERE run_id=?",
                run_id.as_str()
            )
            .fetch_one(&mut *connection)
            .await?;
            let outcome: agent_automation::WorkerOutcome = serde_json::from_str(
                &worker
                    .worker_outcome_json
                    .ok_or(StorageError::InvalidRecord)?,
            )
            .map_err(|_| StorageError::InvalidRecord)?;
            match outcome {
                agent_automation::WorkerOutcome::Failed { explanation }
                | agent_automation::WorkerOutcome::Interrupted { explanation } => {
                    FiringOutcome::Failed {
                        reason: explanation
                            .filter(|reason| !reason.is_empty())
                            .unwrap_or_else(|| {
                                "The scheduled worker failed or was interrupted.".to_owned()
                            })
                            .try_into()
                            .map_err(|_| StorageError::InvalidRecord)?,
                    }
                }
                agent_automation::WorkerOutcome::Completed { .. }
                | agent_automation::WorkerOutcome::PeerMessageWritten { .. } => {
                    FiringOutcome::RunFinished {
                        run_id: run_id.clone(),
                        summary: summary
                            .map(|text| text.to_owned().try_into())
                            .transpose()
                            .map_err(|_| StorageError::InvalidRecord)?,
                    }
                }
            }
        }
        "preparationFailed" => FiringOutcome::Failed {
            reason: "Run preparation failed; inspect the retained local outcome."
                .to_owned()
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
        },
        "uncertain" => FiringOutcome::Uncertain {
            reason: "Native acceptance is unresolved; retained evidence must be reconciled."
                .to_owned()
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
        },
        "preparing" | "stopping" | "summaryRequired" | "summaryRunning" | "summaryBlocked" => {
            return Ok(());
        }
        _ => return Err(StorageError::InvalidRecord),
    };
    crate::central_report_outbox::queue_firing_outcome(connection, &firing_id, outcome, now_ms)
        .await
}
pub(crate) async fn report_delivery_phase(
    connection: &mut SqliteConnection,
    delivery_id: &agent_automation::DeliveryId,
    phase: &str,
    now_ms: i64,
) -> Result<(), StorageError> {
    let row = sqlx::query!(
        "SELECT firing_id FROM firing_receipts WHERE json_extract(local_work_ref,'$.deliveryId')=?",
        delivery_id.as_str()
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let firing_id: FiringId = row
        .firing_id
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    let outcome = match phase {
        "accepted" => FiringOutcome::Delivered,
        "discarded" => FiringOutcome::Expired,
        "failed" => FiringOutcome::Failed {
            reason: "Wake delivery failed; inspect its retained attempt outcome."
                .to_owned()
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
        },
        "uncertain" => FiringOutcome::Uncertain {
            reason: "Wake acceptance is unresolved; retained evidence must be reconciled."
                .to_owned()
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
        },
        "pending" | "retryable" | "dispatching" => return Ok(()),
        _ => return Err(StorageError::InvalidRecord),
    };
    crate::central_report_outbox::queue_firing_outcome(connection, &firing_id, outcome, now_ms)
        .await
}
