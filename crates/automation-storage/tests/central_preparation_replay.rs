//! Persisted Bind corruption is refused; the nullable legacy receipt is never reinterpreted.
use automation_storage::{
    AutomationStore, ExternalAdmission, StorageError, StoredCentralBindRequest,
};
use collaboration_protocol::{OperationId, ScheduleBindPreparedRequest, ScheduleId};
use serde_json::json;
use sqlx::Connection;
type ProofResult = Result<(), Box<dyn std::error::Error>>;

fn bind(
    operation: &OperationId,
    schedule: &ScheduleId,
) -> Result<ScheduleBindPreparedRequest, Box<dyn std::error::Error>> {
    Ok(serde_json::from_value(json!({
        "operationId":operation,"automationId":schedule,"expectedRevision":OperationId::generate(),
        "boundThread":{"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"known-native"},"cwd":"/isolated-prepare"}
    }))?)
}
#[test]
fn bind_codec_retains_exact_request_and_rejects_invalid_or_noncanonical_stored_values()
-> ProofResult {
    let request = bind(&OperationId::generate(), &ScheduleId::generate())?;
    let encoded = StoredCentralBindRequest::encode(&request)?;
    if encoded.as_str() != serde_json::to_string(&request)?
        || serde_json::to_vec(&encoded.request()?)? != encoded.as_str().as_bytes()
    {
        return Err("stored Bind changed the exact typed request bytes".into());
    }
    let mut invalid = serde_json::to_value(&request)?;
    invalid
        .as_object_mut()
        .ok_or("Bind not object")?
        .insert("inventedField".into(), true.into());
    for malformed in [
        serde_json::to_string(&invalid)?,
        format!(" {}", encoded.as_str()),
        encoded
            .as_str()
            .replace("/isolated-prepare", "C:/wrong-host"),
        "null".into(),
    ] {
        if !matches!(
            StoredCentralBindRequest::try_from(malformed),
            Err(StorageError::InvalidRecord)
        ) {
            return Err("invalid persisted Bind was silently accepted".into());
        }
    }
    Ok(())
}
#[tokio::test]
async fn receipt_decoder_rejects_malformed_or_cross_operation_bind_and_preserves_null()
-> ProofResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let operation = OperationId::generate();
    let schedule = ScheduleId::generate();
    let canonical = b"exact caller request";
    store
        .admit_schedule_preparation::<_, serde_json::Value>(&ExternalAdmission {
            operation_id: operation.clone(),
            schedule_id: schedule.clone(),
            canonical_request: canonical.to_vec(),
            evidence: json!({"known":"native target"}),
            now_ms: 0,
        })
        .await?;
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
    )
    .await?;
    sqlx::query("UPDATE operation_receipts SET operation_status='inProgress' WHERE operation_id=?")
        .bind(operation.as_str())
        .execute(&mut connection)
        .await?;
    let null = store
        .read_schedule_preparation::<serde_json::Value, serde_json::Value>(
            &operation, &schedule, canonical,
        )
        .await?
        .ok_or("NULL receipt disappeared")?;
    if null.central_bind_request.is_some() || null.status != "inProgress" {
        return Err("NULL receipt was reinterpreted as a replayable Bind".into());
    }
    for encoded in [
        "{}".to_owned(),
        StoredCentralBindRequest::encode(&bind(&OperationId::generate(), &schedule)?)?
            .as_str()
            .to_owned(),
        StoredCentralBindRequest::encode(&bind(&operation, &ScheduleId::generate())?)?
            .as_str()
            .to_owned(),
    ] {
        sqlx::query(
            "UPDATE operation_receipts SET central_bind_request_json=? WHERE operation_id=?",
        )
        .bind(encoded)
        .bind(operation.as_str())
        .execute(&mut connection)
        .await?;
        if !matches!(
            store
                .read_schedule_preparation::<serde_json::Value, serde_json::Value>(
                    &operation, &schedule, canonical
                )
                .await,
            Err(StorageError::InvalidRecord)
        ) {
            return Err("invalid or mismatched persisted Bind passed the owning decoder".into());
        }
    }
    connection.close().await?;
    store.close().await?;
    directory.close()?;
    Ok(())
}
