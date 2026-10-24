//! Sender-side immutable mail requests and owning-target observations; never local timers.
use crate::{AutomationStore, StorageError, WakeListPosition};
use agent_automation::{OperationId, WakeupId};
use collaboration_mail_protocol::WakeBody;
use collaboration_protocol::{SessionRef, WakeSnapshot};
struct StoredWakeRequest {
    target_json: String,
    canonical_request: Vec<u8>,
    body_json: String,
    valid_until_ms: i64,
    wakeup_id: Option<String>,
    snapshot_json: Option<String>,
}
struct StoredWakeBinding {
    target_json: String,
    snapshot_json: String,
}
struct StoredWakeUpper {
    created_at_ms: i64,
    wakeup_id: String,
}
struct StoredWakeKey {
    created_at_ms: i64,
    wakeup_id: String,
    remote: i64,
}

#[derive(Clone)]
pub struct RemoteWakeMail {
    pub operation_id: OperationId,
    pub target: SessionRef,
    pub canonical_request: Vec<u8>,
    pub body: WakeBody,
    pub valid_until_ms: i64,
}
/// Preparation distinguishes a new dispatch from observation of an existing operation.
pub enum RemoteWakePreparation {
    New(RemoteWakeMail),
    Replayed {
        request: RemoteWakeMail,
        snapshot: Option<Box<WakeSnapshot>>,
    },
}
pub struct WakeListingKey {
    pub wakeup_id: WakeupId,
    pub remote: bool,
}
pub struct WakeListingKeys {
    pub records: Vec<WakeListingKey>,
    pub next: Option<WakeListPosition>,
}
impl AutomationStore {
    /// Freeze relative timing once, before outbox admission; a retry uses identical bytes.
    pub async fn prepare_remote_wake(
        &mut self,
        request: RemoteWakeMail,
    ) -> Result<RemoteWakePreparation, StorageError> {
        if sqlx::query!(
            "SELECT operation_id FROM operation_receipts WHERE operation_id=?",
            request.operation_id.as_str()
        )
        .fetch_optional(&mut self.connection)
        .await?
        .is_some()
        {
            return Err(StorageError::OperationConflict);
        }
        if let Some(row) = sqlx::query_as!(StoredWakeRequest,"SELECT target_json,canonical_request,body_json,valid_until_ms,wakeup_id,snapshot_json FROM remote_wake_requests WHERE operation_id=?",request.operation_id.as_str()).fetch_optional(&mut self.connection).await? {
            let canonical: Vec<u8> = row.canonical_request;
            if canonical != request.canonical_request { return Err(StorageError::OperationConflict); }
            let request = RemoteWakeMail {
                operation_id: request.operation_id,
                target: serde_json::from_str(&row.target_json).map_err(|_|StorageError::InvalidRecord)?,
                canonical_request: canonical,
                body: serde_json::from_str(&row.body_json).map_err(|_|StorageError::InvalidRecord)?,
                valid_until_ms: row.valid_until_ms,
            };
            let snapshot = match (row.wakeup_id, row.snapshot_json) {
                (None, None) => None,
                (Some(id), Some(json)) => {
                    let snapshot: Box<WakeSnapshot> = serde_json::from_str(&json)
                        .map_err(|_| StorageError::InvalidRecord)?;
                    if snapshot.definition.wakeup_id.as_str() != id
                        || snapshot.definition.message.target != request.target
                        || !matches!(request.body.request, collaboration_mail_protocol::RemoteWakeRequest::Send { .. })
                    {
                        return Err(StorageError::InvalidRecord);
                    }
                    Some(snapshot)
                }
                _ => return Err(StorageError::InvalidRecord),
            };
            return Ok(RemoteWakePreparation::Replayed { request, snapshot });
        }
        let target =
            serde_json::to_string(&request.target).map_err(|_| StorageError::InvalidRecord)?;
        let body = serde_json::to_string(&request.body).map_err(|_| StorageError::InvalidRecord)?;
        sqlx::query!("INSERT INTO remote_wake_requests(operation_id,target_json,canonical_request,body_json,valid_until_ms) VALUES(?,?,?,?,?)",request.operation_id.as_str(),target,request.canonical_request,body,request.valid_until_ms)
            .execute(&mut self.connection).await?;
        Ok(RemoteWakePreparation::New(request))
    }

    pub async fn remember_remote_wake(
        &mut self,
        operation: &OperationId,
        snapshot: &WakeSnapshot,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(snapshot).map_err(|_| StorageError::InvalidRecord)?;
        let created = String::from(snapshot.definition.created_at.clone())
            .parse::<chrono::DateTime<chrono::Utc>>()
            .map_err(|_| StorageError::InvalidRecord)?
            .timestamp_millis();
        let target = serde_json::to_string(&snapshot.definition.message.target)
            .map_err(|_| StorageError::InvalidRecord)?;
        let result = sqlx::query!("UPDATE remote_wake_requests SET wakeup_id=?,snapshot_json=?,created_at_ms=? WHERE operation_id=? AND target_json=? AND (wakeup_id IS NULL OR wakeup_id=?)",snapshot.definition.wakeup_id.as_str(),json,created,operation.as_str(),target,snapshot.definition.wakeup_id.as_str())
            .execute(&mut self.connection).await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::InvalidRecord);
        }
        Ok(())
    }

    pub async fn remote_wake_target(
        &mut self,
        wakeup: &WakeupId,
    ) -> Result<Option<SessionRef>, StorageError> {
        let row = sqlx::query_as!(StoredWakeBinding,"SELECT target_json,snapshot_json as \"snapshot_json!\" FROM remote_wake_requests WHERE wakeup_id=?",wakeup.as_str()).fetch_optional(&mut self.connection).await?;
        row.map(|row| {
            let target: SessionRef =
                serde_json::from_str(&row.target_json).map_err(|_| StorageError::InvalidRecord)?;
            let snapshot: WakeSnapshot = serde_json::from_str(&row.snapshot_json)
                .map_err(|_| StorageError::InvalidRecord)?;
            if snapshot.definition.wakeup_id != *wakeup
                || snapshot.definition.message.target != target
            {
                return Err(StorageError::InvalidRecord);
            }
            Ok(target)
        })
        .transpose()
    }

    /// One creation-ordered snapshot membership covers both local and remote wakes.
    pub async fn wake_listing_keys(
        &mut self,
        after: Option<WakeListPosition>,
        limit: u32,
    ) -> Result<WakeListingKeys, StorageError> {
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidRecord);
        }
        let upper = match &after {
            Some(position) => (
                position.upper_created_at_ms,
                position.upper_wakeup_id.clone(),
            ),
            None => {
                let row=sqlx::query_as!(StoredWakeUpper,"SELECT created_at_ms as \"created_at_ms!\",wakeup_id as \"wakeup_id!\" FROM (SELECT created_at_ms,wakeup_id FROM wakeup_definitions UNION ALL SELECT created_at_ms,wakeup_id FROM remote_wake_requests WHERE wakeup_id IS NOT NULL) ORDER BY created_at_ms DESC,wakeup_id DESC LIMIT 1")
                    .fetch_optional(&mut self.connection).await?;
                let Some(row) = row else {
                    return Ok(WakeListingKeys {
                        records: Vec::new(),
                        next: None,
                    });
                };
                (
                    row.created_at_ms,
                    row.wakeup_id
                        .try_into()
                        .map_err(|_| StorageError::InvalidRecord)?,
                )
            }
        };
        let rows=sqlx::query_as!(StoredWakeKey,"SELECT created_at_ms as \"created_at_ms!\",wakeup_id as \"wakeup_id!\",remote as \"remote!\" FROM (SELECT created_at_ms,wakeup_id,0 AS remote FROM wakeup_definitions UNION ALL SELECT created_at_ms,wakeup_id,1 AS remote FROM remote_wake_requests WHERE wakeup_id IS NOT NULL) WHERE (created_at_ms,wakeup_id)<=(?,?) AND (? IS NULL OR (created_at_ms,wakeup_id)>(?,?)) ORDER BY created_at_ms,wakeup_id LIMIT ?",upper.0,upper.1.as_str(),after.as_ref().map(|p|p.created_at_ms),after.as_ref().map(|p|p.created_at_ms),after.as_ref().map(|p|p.wakeup_id.as_str()),i64::from(limit)+1)
            .fetch_all(&mut self.connection).await?;
        let size = usize::try_from(limit).map_err(|_| StorageError::InvalidRecord)?;
        let more = rows.len() > size;
        let mut records = Vec::new();
        let mut next = None;
        for row in rows.into_iter().take(size) {
            let id: WakeupId = row
                .wakeup_id
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?;
            let remote = match row.remote {
                0 => false,
                1 => true,
                _ => return Err(StorageError::InvalidRecord),
            };
            next = Some(WakeListPosition {
                upper_created_at_ms: upper.0,
                upper_wakeup_id: upper.1.clone(),
                created_at_ms: row.created_at_ms,
                wakeup_id: id.clone(),
            });
            records.push(WakeListingKey {
                wakeup_id: id,
                remote,
            });
        }
        Ok(WakeListingKeys {
            records,
            next: if more { next } else { None },
        })
    }
}
