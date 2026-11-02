//! Durable reminder creation and inspection use the same operation identity semantics as schedules.
use crate::local_operation_receipts::{self, CompletedLocalOperation, LocalOperation};
use crate::{AutomationStore, StorageError};
use agent_automation::{
    ChangeId, DurableMessage, EventId, ExpiryRule, OperationId, TimingRule, WakeDefinition,
    WakeRecord, WakeState, WakeupId,
};
use serde::de::DeserializeOwned;
use sqlx::{Connection, Row, SqliteConnection};

pub struct WakeCreate<TMessage> {
    pub operation_id: OperationId,
    pub requested_by: collaboration_protocol::WakeCreator,
    pub replace: bool,
    pub message: TMessage,
    pub timing: TimingRule,
    pub expiry: ExpiryRule,
    pub now_ms: i64,
}
impl AutomationStore {
    pub async fn create_wakeup<TMessage: DurableMessage + DeserializeOwned>(
        &mut self,
        request: &WakeCreate<TMessage>,
    ) -> Result<WakeRecord<TMessage, collaboration_protocol::WakeCreator>, StorageError> {
        if request.now_ms < 0 {
            return Err(StorageError::InvalidRecord);
        }
        if !matches!(request.message.delivery_mode(), "auto" | "steer" | "queue") {
            return Err(StorageError::InvalidWake {
                field: "delivery",
                reason: "must be auto, steer or queue",
            });
        }
        let canonical = serde_json::to_vec(&(
            &request.message,
            &request.timing,
            &request.expiry,
            &request.requested_by,
            request.replace,
        ))
        .map_err(|_| StorageError::InvalidRecord)?;
        let mut transaction = self.connection.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = local_operation_receipts::replay(
            &mut transaction,
            LocalOperation {
                id: &request.operation_id,
                method: "wake/send",
                canonical: &canonical,
            },
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(existing);
        }
        let anchor = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(request.now_ms)
            .ok_or(StorageError::InvalidRecord)?;
        let expiry = Some(
            crate::wake_limits::resolve_expiry(
                &request.timing,
                anchor,
                request.expiry.resolve(anchor)?,
            )?
            .timestamp_millis(),
        );
        replace_occupant(&mut transaction, request).await?;
        let due = request
            .timing
            .next_due(anchor, None)?
            .map(|time| time.timestamp_millis());
        let state = if expiry.is_some_and(|at| at <= request.now_ms) {
            WakeState::Expired
        } else {
            WakeState::Active
        };
        let next_due = if state == WakeState::Active {
            due.filter(|at| expiry.is_none_or(|end| *at < end))
        } else {
            None
        };
        let definition = WakeDefinition {
            wakeup_id: WakeupId::generate(),
            change_id: ChangeId::generate(),
            created_by: request.requested_by.clone(),
            message: request.message.clone(),
            timing: request.timing.clone(),
            anchor_at_ms: request.now_ms,
            expires_at_ms: expiry,
            created_at_ms: request.now_ms,
        };
        let encoded =
            serde_json::to_string(&definition).map_err(|_| StorageError::InvalidRecord)?;
        let status = match state {
            WakeState::Expired => "expired",
            _ => "active",
        };
        sqlx::query("INSERT INTO wakeup_definitions(wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,first_fire_json,pending_delivery_id,created_at_ms,updated_at_ms) VALUES (?,?,?,?,?,?,?,?,NULL,NULL,?,?)")
            .bind(definition.wakeup_id.as_str()).bind(definition.change_id.as_str()).bind(status).bind(encoded).bind(request.now_ms).bind(request.now_ms).bind(next_due).bind(expiry).bind(request.now_ms).bind(request.now_ms).execute(&mut *transaction).await?;
        let event = serde_json::json!({"kind":"stateChange","before":null,"after":status});
        let inserted=sqlx::query("INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES (?,'wake',?,'created',?,?)")
            .bind(EventId::generate().as_str()).bind(definition.wakeup_id.as_str()).bind(event.to_string()).bind(request.now_ms).execute(&mut *transaction).await?;
        let record = WakeRecord {
            definition,
            state,
            next_due_at_ms: next_due,
            first_fire: None,
            pending_delivery_id: None,
            latest_event_sequence: inserted.last_insert_rowid(),
        };
        let result = serde_json::to_string(&record).map_err(|_| StorageError::InvalidRecord)?;
        local_operation_receipts::record(
            &mut transaction,
            CompletedLocalOperation {
                operation: LocalOperation {
                    id: &request.operation_id,
                    method: "wake/send",
                    canonical: &canonical,
                },
                resource_id: record.definition.wakeup_id.as_str(),
                result_json: &result,
                now_ms: request.now_ms,
            },
        )
        .await?;
        transaction.commit().await?;
        Ok(record)
    }
    pub async fn read_wakeup<TMessage: DeserializeOwned>(
        &mut self,
        id: &WakeupId,
    ) -> Result<WakeRecord<TMessage, collaboration_protocol::WakeCreator>, StorageError> {
        read_current(&mut self.connection, id).await
    }
}

async fn replace_occupant<TMessage: DurableMessage + DeserializeOwned>(
    connection: &mut SqliteConnection,
    request: &WakeCreate<TMessage>,
) -> Result<(), StorageError> {
    use collaboration_protocol::{WakeCreator, WakeRefusal};
    if matches!(request.requested_by, WakeCreator::Unrecorded {}) {
        return Err(StorageError::InvalidWake {
            field: "requestedBy",
            reason: "unrecorded is reserved for upgraded wakes",
        });
    }
    let target =
        serde_json::to_string(request.message.target()).map_err(|_| StorageError::InvalidRecord)?;
    let occupant = sqlx::query_scalar!("SELECT wakeup_id FROM wakeup_definitions WHERE authority='local' AND wakeup_status IN ('active','paused') AND json_extract(definition_json,'$.message.target.endpoint.serviceId')=json_extract(?,'$.endpoint.serviceId') AND json_extract(definition_json,'$.message.target.endpoint.endpointId')=json_extract(?,'$.endpoint.endpointId') AND json_extract(definition_json,'$.message.target.sessionId')=json_extract(?,'$.sessionId') LIMIT 1", target, target, target)
        .fetch_optional(&mut *connection).await?;
    // The legacy TEXT primary key is nullable in SQLite; a matching row must
    // still carry a valid identity, rather than disappearing from the slot.
    let occupant = occupant
        .map(|id| id.ok_or(StorageError::InvalidRecord))
        .transpose()?;
    let Some(id) = occupant else {
        return Ok(());
    };
    let id: WakeupId = id.try_into().map_err(|_| StorageError::InvalidRecord)?;
    let previous: WakeRecord<TMessage, WakeCreator> = read_current(connection, &id).await?;
    let creator = &previous.definition.created_by;
    let is_target = match &request.requested_by {
        WakeCreator::Session(session) => {
            serde_json::to_string(session).map_err(|_| StorageError::InvalidRecord)? == target
        }
        _ => false,
    };
    let is_creator =
        !matches!(creator, WakeCreator::Unrecorded {}) && creator == &request.requested_by;
    if !request.replace
        || !(is_creator || is_target || matches!(request.requested_by, WakeCreator::Owner {}))
    {
        return Err(StorageError::WakeRefused(WakeRefusal::WakeExists {
            wakeup: id,
            creator: creator.clone(),
        }));
    }
    crate::wakeup_evaluation::discard_undispatched(connection, &id).await?;
    sqlx::query!("UPDATE wakeup_definitions SET wakeup_status='cancelled',next_due_at_ms=NULL,updated_at_ms=? WHERE wakeup_id=?", request.now_ms, id.as_str()).execute(&mut *connection).await?;
    crate::wakeup_evaluation::append_event(
        connection,
        crate::wakeup_evaluation::WakeEvent {
            id: &id,
            kind: "replaced",
            body: &serde_json::json!({"kind":"stateChange","before":previous.state,"after":"cancelled"}),
            now_ms: request.now_ms,
        },
    )
    .await?;
    Ok(())
}
pub(crate) async fn read_current<TMessage: DeserializeOwned>(
    connection: &mut SqliteConnection,
    id: &WakeupId,
) -> Result<WakeRecord<TMessage, collaboration_protocol::WakeCreator>, StorageError> {
    let row=sqlx::query("SELECT authority,definition_json,wakeup_status,next_due_at_ms,first_fire_json,pending_delivery_id,COALESCE((SELECT seq FROM sqlite_sequence WHERE name='automation_events'),0) AS event_sequence FROM wakeup_definitions WHERE wakeup_id=?")
        .bind(id.as_str()).fetch_optional(connection).await?.ok_or(StorageError::WakeNotFound)?;
    let authority = crate::automation_authority::AutomationAuthority::try_from(
        row.try_get::<&str, _>("authority")?,
    )
    .map_err(|_| StorageError::InvalidRecord)?;
    let definition: WakeDefinition<TMessage, collaboration_protocol::WakeCreator> =
        serde_json::from_str(&row.try_get::<String, _>("definition_json")?)
            .map_err(|_| StorageError::InvalidRecord)?;
    if authority == crate::automation_authority::AutomationAuthority::Central {
        crate::central_definition_projection::validate_central_timing(
            &definition.timing,
            collaboration_protocol::AutomationKind::Wake,
            definition.anchor_at_ms,
        )?;
    }
    if definition.wakeup_id != *id {
        return Err(StorageError::InvalidRecord);
    }
    let state = serde_json::from_value(serde_json::Value::String(row.try_get("wakeup_status")?))
        .map_err(|_| StorageError::InvalidRecord)?;
    let first = row
        .try_get::<Option<String>, _>("first_fire_json")?
        .map(|value| serde_json::from_str(&value).map_err(|_| StorageError::InvalidRecord))
        .transpose()?;
    let pending = row
        .try_get::<Option<String>, _>("pending_delivery_id")?
        .map(|value| value.try_into().map_err(|_| StorageError::InvalidRecord))
        .transpose()?;
    Ok(WakeRecord {
        definition,
        state,
        next_due_at_ms: row.try_get("next_due_at_ms")?,
        first_fire: first,
        pending_delivery_id: pending,
        latest_event_sequence: row.try_get("event_sequence")?,
    })
}
