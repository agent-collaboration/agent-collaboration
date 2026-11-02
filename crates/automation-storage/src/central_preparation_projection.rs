//! Central snapshots satisfy native-preparation foreign keys without becoming Local definitions.
use crate::StorageError;
use collaboration_protocol::{
    EndpointRef, InstructionSnapshot, ObservationTimestamp, ScheduleSnapshot, SessionRef,
};
use sqlx::{Connection, SqliteConnection};

pub(crate) async fn project_central_preparation(
    connection: &mut SqliteConnection,
    schedule: &ScheduleSnapshot,
    instruction: &InstructionSnapshot,
) -> Result<(), StorageError> {
    if schedule.definition.instruction_id != instruction.instruction_id {
        return Err(StorageError::InvalidRecord);
    }
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    crate::central_definition_projection::refuse_local_projection(
        &mut transaction,
        "schedule",
        schedule.schedule_id.as_str(),
    )
    .await?;
    let id = instruction.instruction_id.as_str();
    sqlx::query("INSERT INTO instruction_documents(instruction_id,current_revision_id,instruction_text,updated_at_ms) VALUES (?,?,?,?) ON CONFLICT(instruction_id) DO NOTHING")
        .bind(id).bind(instruction.revision_id.as_str()).bind(instruction.text.as_str()).bind(millis(&instruction.updated_at)?).execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO instruction_revisions(revision_id,instruction_id,instruction_text,recorded_at_ms) VALUES (?,?,?,?) ON CONFLICT(revision_id) DO NOTHING")
        .bind(instruction.revision_id.as_str()).bind(id).bind(instruction.text.as_str()).bind(millis(&instruction.updated_at)?).execute(&mut *transaction).await?;
    write_projection(&mut transaction, schedule).await?;
    transaction.commit().await?;
    Ok(())
}
pub(crate) async fn write_projection(
    connection: &mut SqliteConnection,
    schedule: &ScheduleSnapshot,
) -> Result<(), StorageError> {
    crate::central_definition_projection::refuse_local_projection(
        connection,
        "schedule",
        schedule.schedule_id.as_str(),
    )
    .await?;
    let definition = agent_automation::ScheduleDefinition::<SessionRef, EndpointRef> {
        instruction_id: schedule.definition.instruction_id.clone(),
        timing: crate::central_definition_projection::project_timing(&schedule.definition.timing)?,
        enabled: schedule.definition.enabled,
        destination: serde_json::from_value(
            serde_json::to_value(&schedule.definition.destination)
                .map_err(|_| StorageError::InvalidRecord)?,
        )
        .map_err(|_| StorageError::InvalidRecord)?,
        execution_timeout_seconds: schedule.definition.execution_timeout_seconds.map(u32::from),
        model: schedule.definition.model.clone(),
        effort: schedule.definition.effort.clone(),
    };
    crate::schedule_repository::validate_definition(&definition)?;
    let continuity: agent_automation::ContinuityInput<SessionRef> = serde_json::from_value(
        serde_json::to_value(&schedule.imported_continuity)
            .map_err(|_| StorageError::InvalidRecord)?,
    )
    .map_err(|_| StorageError::InvalidRecord)?;
    let definition = serde_json::to_string(&definition).map_err(|_| StorageError::InvalidRecord)?;
    let continuity = serde_json::to_string(&continuity).map_err(|_| StorageError::InvalidRecord)?;
    sqlx::query("INSERT INTO schedule_definitions(schedule_id,change_id,instruction_id,enabled,definition_json,imported_continuity_json,created_at_ms,updated_at_ms,authority) VALUES (?,?,?,?,?,?,?,?,'central') ON CONFLICT(schedule_id) DO UPDATE SET change_id=excluded.change_id,instruction_id=excluded.instruction_id,enabled=excluded.enabled,definition_json=excluded.definition_json,imported_continuity_json=excluded.imported_continuity_json,updated_at_ms=excluded.updated_at_ms")
        .bind(schedule.schedule_id.as_str()).bind(schedule.change_id.as_str()).bind(schedule.definition.instruction_id.as_str()).bind(schedule.definition.enabled).bind(definition).bind(continuity).bind(millis(&schedule.created_at)?).bind(millis(&schedule.updated_at)?).execute(&mut *connection).await?;
    sqlx::query("INSERT INTO schedule_timing_state(schedule_id,applied_change_id,anchor_at_ms,evaluated_through_ms,next_due_at_ms) VALUES (?,?,?,?,NULL) ON CONFLICT(schedule_id) DO UPDATE SET applied_change_id=excluded.applied_change_id,next_due_at_ms=NULL")
        .bind(schedule.schedule_id.as_str()).bind(schedule.change_id.as_str()).bind(millis(&schedule.anchor_at)?).bind(millis(&schedule.updated_at)?).execute(connection).await?;
    Ok(())
}
fn millis(value: &ObservationTimestamp) -> Result<i64, StorageError> {
    chrono::DateTime::parse_from_rfc3339(&String::from(value.clone()))
        .map(|time| time.timestamp_millis())
        .map_err(|_| StorageError::InvalidRecord)
}
