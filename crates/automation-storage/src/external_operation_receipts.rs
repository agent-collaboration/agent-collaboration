//! External mutations commit admission and intent before I/O; retries inspect retained evidence.
use crate::{AutomationStore, StorageError};
use agent_automation::OperationId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sqlx::{Connection, Row};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalOperationRecord<TState, TResult> {
    pub operation_id: OperationId,
    pub method: String,
    pub resource_id: String,
    pub status: String,
    pub evidence: TState,
    pub result: Option<TResult>,
    pub failure: Option<serde_json::Value>,
    pub admitted_at_ms: i64,
    pub central_bind_request: Option<crate::StoredCentralBindRequest>,
}
pub struct ExternalAdmission<TState> {
    pub operation_id: OperationId,
    pub schedule_id: agent_automation::ScheduleId,
    pub canonical_request: Vec<u8>,
    pub evidence: TState,
    pub now_ms: i64,
}
pub enum ExternalAdmissionResult<TState, TResult> {
    New,
    Existing(ExternalOperationRecord<TState, TResult>),
}
impl AutomationStore {
    /// Inspects replay identity without admitting a new operation or dispatching an effect.
    pub async fn read_schedule_preparation<TState: DeserializeOwned, TResult: DeserializeOwned>(
        &mut self,
        operation_id: &OperationId,
        schedule_id: &agent_automation::ScheduleId,
        canonical_request: &[u8],
    ) -> Result<Option<ExternalOperationRecord<TState, TResult>>, StorageError> {
        sqlx::query("SELECT * FROM operation_receipts WHERE operation_id=?")
            .bind(operation_id.as_str())
            .fetch_optional(&mut self.connection)
            .await?
            .map(|row| decode_preparation(&row, operation_id, schedule_id, canonical_request))
            .transpose()
    }
    pub async fn admit_schedule_preparation<
        TState: Serialize + DeserializeOwned,
        TResult: DeserializeOwned,
    >(
        &mut self,
        request: &ExternalAdmission<TState>,
    ) -> Result<ExternalAdmissionResult<TState, TResult>, StorageError> {
        if request.now_ms < 0 || request.canonical_request.len() > 1_048_576 {
            return Err(StorageError::InvalidRecord);
        }
        let mut transaction = self.connection.begin_with("BEGIN IMMEDIATE").await?;
        let existing = sqlx::query("SELECT * FROM operation_receipts WHERE operation_id=?")
            .bind(request.operation_id.as_str())
            .fetch_optional(&mut *transaction)
            .await?;
        if let Some(row) = existing {
            let record = decode_preparation(
                &row,
                &request.operation_id,
                &request.schedule_id,
                &request.canonical_request,
            )?;
            transaction.commit().await?;
            return Ok(ExternalAdmissionResult::Existing(record));
        }
        let other:Option<String>=sqlx::query_scalar("SELECT operation_id FROM operation_receipts WHERE resource_id=? AND method_name='schedule/prepare' AND operation_status IN ('admitted','inProgress','uncertain') LIMIT 1").bind(request.schedule_id.as_str()).fetch_optional(&mut *transaction).await?;
        if other.is_some() {
            return Err(StorageError::OperationConflict);
        }
        let evidence =
            serde_json::to_string(&request.evidence).map_err(|_| StorageError::InvalidRecord)?;
        sqlx::query("INSERT INTO operation_receipts(operation_id,method_name,canonical_request,resource_id,operation_status,effect_evidence_json,final_result_json,final_error_json,committed_at_ms) VALUES (?,'schedule/prepare',?,?,'admitted',?,NULL,NULL,?)")
            .bind(request.operation_id.as_str()).bind(&request.canonical_request).bind(request.schedule_id.as_str()).bind(evidence).bind(request.now_ms).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(ExternalAdmissionResult::New)
    }
}
fn decode_preparation<TState: DeserializeOwned, TResult: DeserializeOwned>(
    row: &sqlx::sqlite::SqliteRow,
    operation_id: &OperationId,
    schedule_id: &agent_automation::ScheduleId,
    canonical_request: &[u8],
) -> Result<ExternalOperationRecord<TState, TResult>, StorageError> {
    if row.try_get::<String, _>("method_name")? != "schedule/prepare"
        || row.try_get::<String, _>("resource_id")? != schedule_id.as_str()
        || row.try_get::<Vec<u8>, _>("canonical_request")? != canonical_request
    {
        return Err(StorageError::OperationConflict);
    }
    let status: String = row.try_get("operation_status")?;
    let result: Option<TResult> = row
        .try_get::<Option<String>, _>("final_result_json")?
        .map(|encoded| serde_json::from_str(&encoded).map_err(|_| StorageError::InvalidRecord))
        .transpose()?;
    let failure: Option<serde_json::Value> = row
        .try_get::<Option<String>, _>("final_error_json")?
        .map(|encoded| serde_json::from_str(&encoded).map_err(|_| StorageError::InvalidRecord))
        .transpose()?;
    if !matches!(
        (status.as_str(), result.is_some(), failure.is_some()),
        ("admitted" | "inProgress", false, false)
            | ("uncertain", false, _)
            | ("succeeded", true, false)
            | ("failed", false, true)
    ) {
        return Err(StorageError::InvalidRecord);
    }
    let central_bind_request: Option<crate::StoredCentralBindRequest> = row
        .try_get::<Option<String>, _>("central_bind_request_json")?
        .map(TryInto::try_into)
        .transpose()?;
    if let Some(bind) = &central_bind_request {
        let bind = bind.request()?;
        if bind.operation_id != *operation_id
            || bind.automation_id.as_str() != schedule_id.as_str()
            || status == "admitted"
        {
            return Err(StorageError::InvalidRecord);
        }
    }
    Ok(ExternalOperationRecord {
        operation_id: operation_id.clone(),
        method: "schedule/prepare".into(),
        resource_id: schedule_id.as_str().into(),
        status,
        evidence: serde_json::from_str(&row.try_get::<String, _>("effect_evidence_json")?)
            .map_err(|_| StorageError::InvalidRecord)?,
        result,
        failure,
        admitted_at_ms: row.try_get("committed_at_ms")?,
        central_bind_request,
    })
}
