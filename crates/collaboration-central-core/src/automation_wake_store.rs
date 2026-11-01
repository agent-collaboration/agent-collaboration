//! One Fleet transaction owns wake slots, replacements and original results.
use crate::{
    automation_capability_store as capabilities,
    automation_catalog_record::{
        AutomationRecord, BoundAutomationCreator, CatalogState, StoredAutomationDefinition,
        StoredWakeDefinition, timestamp_millis, wake_slot,
    },
    automation_catalog_store as catalog, automation_content_store as contents,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, row_id, timestamp},
    automation_wake_validation as validation,
    fleet_row_ids::FleetRowIds,
    generated_automation_catalog as queries,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationId, AutomationKind, AutomationRefusal, AutomationTiming,
    CatalogAutomation, CentralWakeSnapshot, FiringMessageOrigin, MessageContent, SavedMessage,
    WakeCreator, WakeDefinition, WakeMutationRequest, WakeSendRequest, WakeShowRequest, WakeState,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) enum WakeAction {
    Pause,
    Resume,
    Cancel,
}

pub(crate) async fn create<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &WakeSendRequest,
    installation: ServiceId,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let origin = validation::origin(
        installation.clone(),
        &request.requested_by,
        &request.message,
    )?;
    let timing = AutomationTiming::resolve(&request.timing, AutomationKind::Wake, now_ms)?;
    let expires_ms = validation::expiry(&request.expiry, &request.timing, &timing, now_ms)?;
    if !capabilities::eligible(scope, &request.message.target.endpoint.service_id).await? {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::TargetNeedsUpgrade {
                service_id: request.message.target.endpoint.service_id.clone(),
            },
        ));
    }
    let slot = wake_slot(&request.message.target)?;
    let previous = queries::find_wake_slot(scope, slot)
        .await?
        .map(catalog::decode)
        .transpose()?;
    let previous = authorize_replacement(previous, request, &installation, now_ms, row_ids)?;
    let now = timestamp(now_ms)?;
    let id: AutomationId = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?;
    let revision = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?;
    let text = match &request.message.content {
        MessageContent::Agent { text, .. }
        | MessageContent::HumanUser { text }
        | MessageContent::Router { text } => text.as_str(),
    };
    let text: collaboration_protocol::InstructionText = text
        .to_owned()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("wake.text"))?;
    let content = contents::prepare(&text, row_ids)?;
    let record = AutomationRecord {
        id,
        revision,
        state: if expires_ms <= now_ms {
            CatalogState::Expired
        } else {
            CatalogState::Active
        },
        creator: BoundAutomationCreator {
            installation,
            declared: request.requested_by.clone(),
        },
        next_due: Some(timing.first_due())
            .filter(|due| expires_ms > now_ms && due.unix_millis() < expires_ms),
        definition: StoredAutomationDefinition::Wake(StoredWakeDefinition {
            target: request.message.target.clone(),
            origin,
            delivery: request.message.delivery,
            generation_guard: request.message.generation_guard.clone(),
            content: content.clone(),
            timing_request: request.timing.clone(),
            timing,
            anchor_at: now.clone(),
            expires_at: timestamp(expires_ms)?,
            created_at: now,
        }),
    };
    let answer = AutomationAnswer::Wake(project(&record, request.message.clone(), None)?);
    let owner = row_id(record.id.as_str(), "automation_id")?;
    // Every ID, domain value and answer is prepared before the first write.
    contents::insert_prepared(scope, owner, &content, &text, now_ms).await?;
    if let Some(previous) = previous {
        catalog::update(scope, &previous).await.map_err(rollback)?;
    }
    catalog::insert(scope, &record).await.map_err(rollback)?;
    Ok(answer)
}

fn authorize_replacement(
    previous: Option<AutomationRecord>,
    request: &WakeSendRequest,
    installation: &ServiceId,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<Option<AutomationRecord>> {
    let Some(mut previous) = previous else {
        return Ok(None);
    };
    let StoredAutomationDefinition::Wake(wake) = &previous.definition else {
        return Err(AutomationStoreError::InvalidRecord("slot_session_key"));
    };
    if timestamp_millis(&wake.expires_at)? <= now_ms {
        previous.state = CatalogState::Expired;
    } else {
        let is_target =
            matches!(&request.requested_by,WakeCreator::Session(session) if *session==wake.target);
        let is_creator = match &previous.creator.declared {
            WakeCreator::Unrecorded {} => false,
            WakeCreator::Router {} => {
                previous.creator.installation == *installation
                    && previous.creator.declared == request.requested_by
            }
            creator => *creator == request.requested_by,
        };
        if !request.replace
            || !(is_target || is_creator || matches!(request.requested_by, WakeCreator::Owner {}))
        {
            return Err(AutomationStoreError::Refused(
                AutomationRefusal::WakeExists {
                    creator: previous.creator.declared,
                },
            ));
        }
        previous.state = CatalogState::Cancelled;
    }
    previous.next_due = None;
    previous.revision = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?;
    Ok(Some(previous))
}

pub(crate) async fn show<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &WakeShowRequest,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let id: AutomationId = request
        .wakeup_id
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?;
    let Some(record) = catalog::read(scope, &id).await? else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    if !matches!(record.definition, StoredAutomationDefinition::Wake(_)) {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    }
    Ok(AutomationAnswer::Wake(snapshot(scope, &record).await?))
}

pub(crate) async fn change<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &WakeMutationRequest,
    action: WakeAction,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    timestamp(now_ms)?;
    let id: AutomationId = request
        .wakeup_id
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?;
    let Some(mut record) = catalog::read(scope, &id).await? else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    let StoredAutomationDefinition::Wake(wake) = &record.definition else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    let expiry = timestamp_millis(&wake.expires_at)?;
    let before = record.state;
    if before.owns_wake_slot() && expiry <= now_ms {
        record.state = CatalogState::Expired;
        record.next_due = None;
    } else {
        match (action, before) {
            (WakeAction::Pause, CatalogState::Active) => {
                record.state = CatalogState::Paused;
                record.next_due = None;
            }
            (WakeAction::Pause, CatalogState::Paused)
            | (WakeAction::Resume, CatalogState::Active) => {}
            (WakeAction::Cancel, state) if state.owns_wake_slot() => {
                record.state = CatalogState::Cancelled;
                record.next_due = None;
            }
            (WakeAction::Cancel, _) => {}
            (WakeAction::Resume, CatalogState::Paused) => {
                let next = wake.timing.next_after(now_ms)?;
                record.state = if next.is_some() {
                    CatalogState::Active
                } else {
                    CatalogState::Finished
                };
                record.next_due = next.filter(|due| due.unix_millis() < expiry);
            }
            _ => {
                return Err(AutomationStoreError::Refused(
                    AutomationRefusal::WakeLifecycleConflict {
                        state: state(before)?,
                    },
                ));
            }
        }
    }
    if before != record.state {
        record.revision = row_ids
            .next()?
            .to_string()
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?;
    }
    let answer = AutomationAnswer::Mutation {
        already_sent: crate::automation_firing_store::sent_firings(scope, &record.id).await?,
        automation: CatalogAutomation::Wake(snapshot(scope, &record).await?),
    };
    catalog::update(scope, &record).await?;
    Ok(answer)
}

pub(crate) async fn snapshot<Scope: TransactionScope>(
    scope: &mut Scope,
    record: &AutomationRecord,
) -> AutomationStoreResult<CentralWakeSnapshot>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let StoredAutomationDefinition::Wake(wake) = &record.definition else {
        return Err(AutomationStoreError::InvalidRecord("wake.kind"));
    };
    let content = contents::read_wake(scope, &wake.content.content_id)
        .await?
        .ok_or(AutomationStoreError::InvalidRecord("content_id"))?;
    if content.reference != wake.content
        || content.owner_id != row_id(record.id.as_str(), "automation_id")?
    {
        return Err(AutomationStoreError::InvalidRecord("wake.content"));
    }
    let text = String::from(content.text)
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("wake.text"))?;
    let message = SavedMessage {
        target: wake.target.clone(),
        delivery: wake.delivery,
        generation_guard: wake.generation_guard.clone(),
        content: match &wake.origin {
            FiringMessageOrigin::Agent { sender } => MessageContent::Agent {
                sender: sender.clone(),
                text,
            },
            FiringMessageOrigin::HumanUser => MessageContent::HumanUser { text },
            FiringMessageOrigin::Router => MessageContent::Router { text },
        },
    };
    let first = crate::automation_firing_store::first(scope, &record.id).await?;
    project(record, message, first)
}
fn project(
    record: &AutomationRecord,
    message: SavedMessage,
    first_firing: Option<collaboration_protocol::FiringSnapshot>,
) -> AutomationStoreResult<CentralWakeSnapshot> {
    let StoredAutomationDefinition::Wake(wake) = &record.definition else {
        return Err(AutomationStoreError::InvalidRecord("wake.kind"));
    };
    Ok(CentralWakeSnapshot {
        definition: WakeDefinition {
            wakeup_id: record
                .id
                .as_str()
                .to_owned()
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("wakeup_id"))?,
            change_id: record.revision.clone(),
            created_by: record.creator.declared.clone(),
            message,
            timing: wake.timing_request.clone(),
            anchor_at: wake.anchor_at.clone(),
            expires_at: Some(wake.expires_at.clone()),
            created_at: wake.created_at.clone(),
        },
        state: state(record.state)?,
        next_due_at: record
            .next_due
            .map(|due| timestamp(due.unix_millis()))
            .transpose()?,
        first_firing,
    })
}
fn state(state: CatalogState) -> AutomationStoreResult<WakeState> {
    match state {
        CatalogState::Active => Ok(WakeState::Active),
        CatalogState::Paused => Ok(WakeState::Paused),
        CatalogState::Cancelled => Ok(WakeState::Cancelled),
        CatalogState::Expired => Ok(WakeState::Expired),
        CatalogState::Finished => Ok(WakeState::Finished),
        _ => Err(AutomationStoreError::InvalidRecord("wake.state")),
    }
}
fn rollback(error: AutomationStoreError) -> AutomationStoreError {
    AutomationStoreError::Storage(StoreError::backend(error))
}
