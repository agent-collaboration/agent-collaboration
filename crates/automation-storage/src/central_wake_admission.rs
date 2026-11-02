//! A central wake creates the same mailbox obligation and typed push as local evaluation.
use crate::{CentralFiringRequest, CentralLocalWork, StorageError};
use agent_automation::{DeliveryId, FirstFire, WakeDefinition, WakeupId};
use collaboration_protocol::{
    FiringMessageOrigin, FiringOutcome, FiringWork, MessageContent, PushHeaderFacts, PushId,
    PushKind, PushOrigin, PushRecordDraft, RouterOriginRef, SavedMessage,
};
use sqlx::SqliteConnection;

pub(crate) async fn admit_wake(
    connection: &mut SqliteConnection,
    request: &CentralFiringRequest,
) -> Result<(CentralLocalWork, FiringOutcome), StorageError> {
    let FiringWork::Wake {
        created_by,
        timing,
        anchor_at,
        created_at,
        target,
        origin,
        delivery,
        generation_guard,
        expires_at,
    } = &request.firing.work
    else {
        return Err(StorageError::InvalidRecord);
    };
    let expiry = timestamp_millis(expires_at)?;
    if expiry <= request.now_ms {
        return Ok((CentralLocalWork::ExpiredWake, FiringOutcome::Expired));
    }
    let wakeup_id: WakeupId = request
        .firing
        .automation_id
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    crate::central_definition_projection::refuse_local_projection(
        connection,
        "wake",
        wakeup_id.as_str(),
    )
    .await?;
    let text = request
        .content
        .text
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| StorageError::InvalidRecord)?;
    let content = match origin {
        FiringMessageOrigin::Agent { sender } => MessageContent::Agent {
            sender: sender.clone(),
            text,
        },
        FiringMessageOrigin::HumanUser => MessageContent::HumanUser { text },
        FiringMessageOrigin::Router => MessageContent::Router { text },
    };
    let message = SavedMessage {
        target: target.clone(),
        content,
        delivery: *delivery,
        generation_guard: generation_guard.clone(),
    };
    let anchor_at_ms = timestamp_millis(anchor_at)?;
    let created_at_ms = timestamp_millis(created_at)?;
    let definition = WakeDefinition {
        wakeup_id: wakeup_id.clone(),
        change_id: request.firing.revision.clone(),
        created_by: created_by.clone(),
        message: message.clone(),
        timing: crate::central_definition_projection::project_timing(timing)?,
        anchor_at_ms,
        expires_at_ms: Some(expiry),
        created_at_ms,
    };
    let fire = FirstFire {
        wakeup_id: wakeup_id.clone(),
        occurrence_id: request
            .firing
            .firing_id
            .as_str()
            .to_owned()
            .try_into()
            .map_err(|_| StorageError::InvalidRecord)?,
        due_at_ms: request.firing.due_at.unix_millis(),
        fired_at_ms: request.now_ms,
    };
    let delivery_id = DeliveryId::generate();
    let definition_json =
        serde_json::to_string(&definition).map_err(|_| StorageError::InvalidRecord)?;
    let first_fire = serde_json::to_string(&fire).map_err(|_| StorageError::InvalidRecord)?;
    sqlx::query!("INSERT INTO wakeup_definitions(wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,first_fire_json,pending_delivery_id,created_at_ms,updated_at_ms,authority) VALUES (?,?,'active',?,?,?,NULL,?,?,?, ?,?,'central') ON CONFLICT(wakeup_id) DO UPDATE SET change_id=excluded.change_id,wakeup_status='active',definition_json=excluded.definition_json,expires_at_ms=excluded.expires_at_ms,first_fire_json=coalesce(wakeup_definitions.first_fire_json,excluded.first_fire_json),pending_delivery_id=excluded.pending_delivery_id,updated_at_ms=excluded.updated_at_ms",
        wakeup_id.as_str(),request.firing.revision.as_str(),definition_json,anchor_at_ms,fire.due_at_ms,expiry,first_fire,delivery_id.as_str(),created_at_ms,request.now_ms).execute(&mut *connection).await?;
    let target_json = serde_json::to_string(target).map_err(|_| StorageError::InvalidRecord)?;
    let content_json =
        serde_json::to_string(&message.content).map_err(|_| StorageError::InvalidRecord)?;
    let guard_json = generation_guard
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| StorageError::InvalidRecord)?;
    let mode = match delivery {
        collaboration_protocol::MessageDelivery::Auto => "auto",
        collaboration_protocol::MessageDelivery::Queue => "queue",
        collaboration_protocol::MessageDelivery::Steer => "steer",
    };
    sqlx::query!("INSERT INTO mailbox_deliveries(delivery_id,wakeup_id,occurrence_id,due_at_ms,fired_at_ms,target_json,message_json,delivery_mode,generation_guard_json,delivery_status,eligible_at_ms,expires_at_ms,created_at_ms) VALUES (?,?,?,?,?,?,?,?,?,'pending',?,?,?)",
        delivery_id.as_str(),wakeup_id.as_str(),fire.occurrence_id.as_str(),fire.due_at_ms,request.now_ms,target_json,content_json,mode,guard_json,request.now_ms,expiry,request.now_ms).execute(&mut *connection).await?;
    let origin_router_ref = RouterOriginRef::Wake {
        wakeup_id,
        occurrence_id: fire.occurrence_id,
    }
    .canonical_string()
    .map_err(|_| StorageError::InvalidRecord)?;
    let created_at = chrono::DateTime::from_timestamp_millis(request.now_ms)
        .ok_or(StorageError::InvalidRecord)?;
    let draft = PushRecordDraft {
        mode: None,
        guard: None,
        push_id: PushId::try_from(request.firing.firing_id.as_str().to_owned())
            .map_err(|_| StorageError::InvalidRecord)?,
        kind: PushKind::Wake,
        origin: PushOrigin::Router(PushKind::Wake),
        origin_router_ref: Some(origin_router_ref),
        target: target.clone(),
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::Wake,
        body: Some(request.content.text.as_str().to_owned()),
        activity: None,
        created_at,
    };
    crate::owed_push_operations::insert_owed_push_operation(
        connection,
        crate::PushOperation::Admit {
            push: Box::new(draft),
        },
        created_at,
    )
    .await?;
    Ok((
        CentralLocalWork::Wake { delivery_id },
        FiringOutcome::Received,
    ))
}
fn timestamp_millis(
    timestamp: &collaboration_protocol::ObservationTimestamp,
) -> Result<i64, StorageError> {
    String::from(timestamp.clone())
        .parse::<chrono::DateTime<chrono::Utc>>()
        .map(|value| value.timestamp_millis())
        .map_err(|_| StorageError::InvalidRecord)
}
