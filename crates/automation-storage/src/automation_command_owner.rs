//! Local command authority and frozen handover custody select each Automation's existing owner.
use crate::{StorageError, automation_authority::AutomationAuthority};
use agent_automation::{ScheduleId, WakeupId};
use sqlx::{Row, SqliteConnection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoredAutomationOwner {
    Local,
    HandoverPending,
    Central,
}
pub(crate) enum HandoverState {
    Pending,
    Moved,
}
impl TryFrom<&str> for HandoverState {
    type Error = StorageError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pending" => Ok(Self::Pending),
            "moved" => Ok(Self::Moved),
            _ => Err(StorageError::InvalidRecord),
        }
    }
}
pub(crate) async fn read_wake_owner(
    connection: &mut SqliteConnection,
    id: &WakeupId,
) -> Result<StoredAutomationOwner, StorageError> {
    let row=sqlx::query("SELECT (SELECT authority FROM wakeup_definitions WHERE wakeup_id=?1) AS authority, (SELECT state FROM automation_handover WHERE automation_id=?1) AS handover_state")
        .bind(id.as_str()).fetch_one(connection).await?;
    decode_owner(row.try_get("authority")?, row.try_get("handover_state")?)
}
pub(crate) async fn read_schedule_owner(
    connection: &mut SqliteConnection,
    id: &ScheduleId,
) -> Result<StoredAutomationOwner, StorageError> {
    let row=sqlx::query("SELECT (SELECT authority FROM schedule_definitions WHERE schedule_id=?1) AS authority, (SELECT state FROM automation_handover WHERE automation_id=?1) AS handover_state")
        .bind(id.as_str()).fetch_one(connection).await?;
    decode_owner(row.try_get("authority")?, row.try_get("handover_state")?)
}
fn decode_owner(
    authority: Option<String>,
    handover: Option<String>,
) -> Result<StoredAutomationOwner, StorageError> {
    let authority = authority
        .as_deref()
        .map(AutomationAuthority::try_from)
        .transpose()
        .map_err(|_| StorageError::InvalidRecord)?;
    let handover = handover
        .as_deref()
        .map(HandoverState::try_from)
        .transpose()?;
    match (authority, handover) {
        (None, Some(HandoverState::Pending)) => Err(StorageError::InvalidRecord),
        (Some(_), Some(HandoverState::Pending)) => Ok(StoredAutomationOwner::HandoverPending),
        (_, Some(HandoverState::Moved)) | (None | Some(AutomationAuthority::Central), None) => {
            Ok(StoredAutomationOwner::Central)
        }
        (Some(AutomationAuthority::Local), None) => Ok(StoredAutomationOwner::Local),
    }
}
