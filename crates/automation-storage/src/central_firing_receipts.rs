//! One transaction binds a central firing to one local obligation before custody is acknowledged.
use crate::{CentralFiringReceipt, CentralFiringRequest, CentralLocalWork, StorageError};
#[cfg(debug_assertions)]
use agent_automation::{DeliveryId, RunId};
use collaboration_protocol::{FiringId, FiringOutcome, FiringReport, WorkItemReport};
use sqlx::{Connection, SqliteConnection};

struct FiringReceiptRow {
    firing_id: String,
    batch_id: String,
    local_work_ref: String,
    state: String,
    report_seq: i64,
}
impl FiringReceiptRow {
    fn decode(self) -> Result<CentralFiringReceipt, StorageError> {
        let receipt = CentralFiringReceipt {
            firing_id: self
                .firing_id
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
            batch_id: self
                .batch_id
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
            local_work: serde_json::from_str(&self.local_work_ref)
                .map_err(|_| StorageError::InvalidRecord)?,
            outcome: serde_json::from_str(&self.state).map_err(|_| StorageError::InvalidRecord)?,
            report_seq: u64::try_from(self.report_seq)
                .map_err(|_| StorageError::InvalidRecord)?
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
        };
        let valid = match (&receipt.local_work, &receipt.outcome) {
            (CentralLocalWork::ExpiredWake, FiringOutcome::Expired) => true,
            (CentralLocalWork::ExpiredWake, _) => false,
            (
                CentralLocalWork::Schedule { run_id },
                FiringOutcome::RunStarted { run_id: reported }
                | FiringOutcome::RunFinished {
                    run_id: reported, ..
                },
            ) => run_id == reported,
            (
                CentralLocalWork::Schedule { .. },
                FiringOutcome::Delivered | FiringOutcome::Expired,
            ) => false,
            (
                CentralLocalWork::Wake { .. },
                FiringOutcome::RunStarted { .. } | FiringOutcome::RunFinished { .. },
            ) => false,
            (CentralLocalWork::Schedule { .. } | CentralLocalWork::Wake { .. }, _) => true,
        };
        if !valid || u64::from(receipt.report_seq) == 0 {
            return Err(StorageError::InvalidRecord);
        }
        Ok(receipt)
    }
}
pub(crate) async fn admit_central_firing(
    connection: &mut SqliteConnection,
    request: &CentralFiringRequest,
    reports_changed: &tokio::sync::Notify,
    #[cfg(debug_assertions)] faults: Option<&dyn crate::AutomationFaultObserver>,
) -> Result<CentralFiringReceipt, StorageError> {
    if request.now_ms < request.firing.due_at.unix_millis()
        || request.content.reference != request.firing.content
    {
        return Err(StorageError::InvalidRecord);
    }
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    if let Some(receipt) = read_receipt(&mut transaction, &request.firing.firing_id).await? {
        if receipt.batch_id != request.batch_id {
            return Err(StorageError::OperationConflict);
        }
        transaction.commit().await?;
        return Ok(receipt);
    }
    let (local_work, outcome) =
        crate::central_firing_projection::admit_work(&mut transaction, request).await?;
    let receipt = CentralFiringReceipt {
        firing_id: request.firing.firing_id.clone(),
        batch_id: request.batch_id.clone(),
        local_work,
        outcome,
        report_seq: 1_u64.try_into().map_err(|_| StorageError::InvalidRecord)?,
    };
    let local_work =
        serde_json::to_string(&receipt.local_work).map_err(|_| StorageError::InvalidRecord)?;
    let state = serde_json::to_string(&receipt.outcome).map_err(|_| StorageError::InvalidRecord)?;
    sqlx::query!("INSERT INTO firing_receipts(firing_id,batch_id,local_work_ref,state,report_seq) VALUES (?,?,?,?,1)",
            receipt.firing_id.as_str(), receipt.batch_id.as_str(),local_work,state).execute(&mut *transaction).await?;
    crate::central_report_outbox::insert_report(
        &mut transaction,
        &WorkItemReport::Firing(FiringReport {
            firing_id: receipt.firing_id.clone(),
            seq: receipt.report_seq,
            outcome: receipt.outcome.clone(),
        }),
        request.now_ms,
    )
    .await?;
    #[cfg(debug_assertions)]
    if let Some(faults) = faults {
        faults
            .hold(
                crate::AutomationFaultPoint::EntryAdmissionBeforeCommit,
                crate::AutomationFaultEvidence {
                    batch_id: Some(receipt.batch_id.clone()),
                    firing_id: Some(receipt.firing_id.clone()),
                    run_id: match &receipt.local_work {
                        CentralLocalWork::Schedule { run_id } => Some(run_id.clone()),
                        _ => None,
                    },
                    ..crate::AutomationFaultEvidence::default()
                },
            )
            .await;
    }
    transaction.commit().await?;
    reports_changed.notify_one();
    Ok(receipt)
}
pub(crate) async fn read_receipt(
    connection: &mut SqliteConnection,
    firing_id: &FiringId,
) -> Result<Option<CentralFiringReceipt>, StorageError> {
    sqlx::query_as!(FiringReceiptRow,"SELECT firing_id,batch_id,local_work_ref,state,report_seq FROM firing_receipts WHERE firing_id=?",firing_id.as_str())
        .fetch_optional(connection).await?.map(FiringReceiptRow::decode).transpose()
}

#[cfg(debug_assertions)]
pub(crate) async fn central_run_fault_evidence(
    connection: &mut SqliteConnection,
    run_id: &RunId,
) -> Result<Option<crate::AutomationFaultEvidence>, StorageError> {
    let row = sqlx::query!(
        "SELECT firing_id FROM firing_receipts WHERE json_extract(local_work_ref,'$.runId')=?",
        run_id.as_str()
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let firing_id: FiringId = row
        .firing_id
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    let receipt = read_receipt(&mut *connection, &firing_id)
        .await?
        .ok_or(StorageError::InvalidRecord)?;
    Ok(Some(crate::AutomationFaultEvidence {
        batch_id: Some(receipt.batch_id),
        firing_id: Some(firing_id),
        run_id: Some(run_id.clone()),
        ..Default::default()
    }))
}
#[cfg(debug_assertions)]
pub(crate) async fn central_delivery_fault_evidence(
    connection: &mut SqliteConnection,
    delivery_id: &DeliveryId,
) -> Result<Option<crate::AutomationFaultEvidence>, StorageError> {
    let row = sqlx::query!(
        "SELECT firing_id FROM firing_receipts WHERE json_extract(local_work_ref,'$.deliveryId')=?",
        delivery_id.as_str()
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let firing_id: FiringId = row
        .firing_id
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    let receipt = read_receipt(&mut *connection, &firing_id)
        .await?
        .ok_or(StorageError::InvalidRecord)?;
    Ok(Some(crate::AutomationFaultEvidence {
        batch_id: Some(receipt.batch_id),
        firing_id: Some(firing_id),
        ..Default::default()
    }))
}
