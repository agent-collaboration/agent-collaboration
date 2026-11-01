//! The bound machine replaces its work protocols in the pull transaction.
use crate::{
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, timestamp, timestamp_millis,
    },
    generated_automation_capability as queries,
};
use collaboration_protocol::{MachineWorkProtocol, UuidIdentity};
use durable_steps::storage::TransactionScope;
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn write<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: ServiceId,
    protocols: &[MachineWorkProtocol],
    now_ms: i64,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    timestamp(now_ms)?;
    let protocols = serde_json::to_string(protocols)
        .map_err(|_| AutomationStoreError::InvalidRecord("work_protocols"))?;
    queries::write_work_capability(
        scope,
        installation.as_str().to_owned(),
        protocols,
        now_ms.to_string(),
    )
    .await?;
    Ok(())
}

pub(crate) async fn eligible<Scope: TransactionScope>(
    scope: &mut Scope,
    service: &UuidIdentity,
) -> AutomationStoreResult<bool>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let service = String::from(service.clone());
    let Some(row) = queries::read_work_capability(scope, service.clone()).await? else {
        return Ok(false);
    };
    let stored = required(row.service_id, "machine_work_capabilities.service_id")?;
    let _: ServiceId = stored
        .clone()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("machine_work_capabilities.service_id"))?;
    if stored != service {
        return Err(AutomationStoreError::InvalidRecord(
            "machine_work_capabilities.service_id",
        ));
    }
    timestamp_millis(
        row.observed_at_ms,
        "machine_work_capabilities.observed_at_ms",
    )?;
    let protocols: Vec<MachineWorkProtocol> = serde_json::from_str(&required(
        row.protocols_json,
        "machine_work_capabilities.protocols_json",
    )?)
    .map_err(|_| AutomationStoreError::InvalidRecord("machine_work_capabilities.protocols_json"))?;
    Ok(protocols.contains(&MachineWorkProtocol::MachineWorkV1))
}
