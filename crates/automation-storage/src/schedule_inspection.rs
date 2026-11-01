//! Schedule inspection joins configuration and timing with Run-owned occupancy in one read transaction.
use crate::{AutomationStore, StorageError};
use agent_automation::{RunId, ScheduleId, ScheduleRecord};
use serde::de::DeserializeOwned;
use sqlx::{Connection, Row, SqliteConnection};
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScheduleInspection<TTarget, TEndpoint, TProject> {
    pub record: ScheduleRecord<TTarget, TEndpoint, TProject>,
    pub active_run_id: Option<RunId>,
    pub waiting_run_id: Option<RunId>,
}
impl AutomationStore {
    pub async fn inspect_schedule<
        TTarget: DeserializeOwned,
        TEndpoint: DeserializeOwned,
        TProject: DeserializeOwned,
    >(
        &mut self,
        id: &ScheduleId,
    ) -> Result<ScheduleInspection<TTarget, TEndpoint, TProject>, StorageError> {
        let mut transaction = self.connection.begin().await?;
        let record = read_current(&mut transaction, id).await?;
        let inventory = crate::run_inventory::load(&mut transaction, id).await?;
        transaction.commit().await?;
        Ok(ScheduleInspection {
            record,
            active_run_id: inventory.occupying,
            waiting_run_id: inventory.waiting,
        })
    }
}
pub(crate) async fn read_current<
    TTarget: DeserializeOwned,
    TEndpoint: DeserializeOwned,
    TProject: DeserializeOwned,
>(
    connection: &mut SqliteConnection,
    id: &ScheduleId,
) -> Result<ScheduleRecord<TTarget, TEndpoint, TProject>, StorageError> {
    let row=sqlx::query("SELECT s.*,t.anchor_at_ms,t.next_due_at_ms,t.applied_change_id FROM schedule_definitions s JOIN schedule_timing_state t ON s.schedule_id=t.schedule_id WHERE s.schedule_id=?").bind(id.as_str()).fetch_optional(connection).await?.ok_or(StorageError::ScheduleNotFound)?;
    let change: String = row.try_get("change_id")?;
    if change != row.try_get::<String, _>("applied_change_id")? {
        return Err(StorageError::InvalidRecord);
    }
    let definition: agent_automation::ScheduleDefinition<TTarget, TEndpoint, TProject> =
        serde_json::from_str(&row.try_get::<String, _>("definition_json")?)
            .map_err(|_| StorageError::InvalidRecord)?;
    if definition.instruction_id.as_str() != row.try_get::<String, _>("instruction_id")?
        || definition.enabled != row.try_get::<bool, _>("enabled")?
    {
        return Err(StorageError::InvalidRecord);
    }
    Ok(ScheduleRecord {
        schedule_id: id.clone(),
        change_id: change.try_into().map_err(|_| StorageError::InvalidRecord)?,
        definition,
        imported_continuity: serde_json::from_str(
            &row.try_get::<String, _>("imported_continuity_json")?,
        )
        .map_err(|_| StorageError::InvalidRecord)?,
        anchor_at_ms: row.try_get("anchor_at_ms")?,
        next_due_at_ms: row.try_get("next_due_at_ms")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}
