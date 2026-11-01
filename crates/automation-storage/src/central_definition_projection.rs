//! Shared projection validation never calls the admission owners.
use crate::StorageError;
use collaboration_protocol::TimingRequest;
use sqlx::SqliteConnection;

pub(crate) async fn refuse_local_projection(
    connection: &mut SqliteConnection,
    kind: &str,
    id: &str,
) -> Result<(), StorageError> {
    let authority = match kind {
        "schedule" => {
            sqlx::query_scalar!(
                "SELECT authority FROM schedule_definitions WHERE schedule_id=?",
                id
            )
            .fetch_optional(connection)
            .await?
        }
        "wake" => {
            sqlx::query_scalar!(
                "SELECT authority FROM wakeup_definitions WHERE wakeup_id=?",
                id
            )
            .fetch_optional(connection)
            .await?
        }
        _ => return Err(StorageError::InvalidRecord),
    };
    match authority
        .as_deref()
        .map(crate::automation_authority::AutomationAuthority::try_from)
        .transpose()
        .map_err(|_| StorageError::InvalidRecord)?
    {
        None | Some(crate::automation_authority::AutomationAuthority::Central) => Ok(()),
        Some(crate::automation_authority::AutomationAuthority::Local) => {
            Err(StorageError::InvalidRecord)
        }
    }
}
pub(crate) fn project_timing(
    request: &TimingRequest,
) -> Result<agent_automation::TimingRule, StorageError> {
    Ok(match request {
        TimingRequest::At { at } => agent_automation::TimingRule::At {
            at: String::from(at.clone())
                .parse()
                .map_err(|_| StorageError::InvalidRecord)?,
        },
        TimingRequest::Interval { seconds, .. } => agent_automation::TimingRule::Interval {
            seconds: u32::from(*seconds),
        },
        TimingRequest::Cron {
            expression,
            timezone,
        } => agent_automation::TimingRule::Cron {
            expression: expression.clone(),
            timezone: timezone.clone(),
        },
        TimingRequest::After { .. } => return Err(StorageError::InvalidRecord),
    })
}

pub(crate) fn validate_central_timing(
    timing: &agent_automation::TimingRule,
    kind: collaboration_protocol::AutomationKind,
    anchor_ms: i64,
) -> Result<(), StorageError> {
    if let agent_automation::TimingRule::Interval { seconds } = timing {
        match kind {
            collaboration_protocol::AutomationKind::Schedule => {
                collaboration_protocol::GridPeriod::new(*seconds)?;
            }
            collaboration_protocol::AutomationKind::Wake => {
                collaboration_protocol::GridPeriod::for_wake(*seconds)?;
            }
        }
    }
    let timing: TimingRequest = serde_json::from_value(
        serde_json::to_value(timing).map_err(|_| StorageError::InvalidRecord)?,
    )
    .map_err(|_| StorageError::InvalidRecord)?;
    collaboration_protocol::AutomationTiming::resolve(&timing, kind, anchor_ms)?;
    Ok(())
}
