/// Where a claim's settlement is owed, and how far it has got.
pub(crate) mod owed_settlement;

use crate::{registry_records::*, registry_storage::RegistryStorage, *};
use durable_steps::{AtomicStore, JournalScope, RequestKey};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn process_cancel<S, D, Settlement>(
    scope: &mut S::Scope<'_>,
    id: TransferId,
    now: SyncTime,
) -> Result<Option<HolderChangeAnswer>, LeaseError>
where
    S: AtomicStore,
    D: Database,
    Settlement: crate::OwedSettlement<S, D>,
    for<'t> S::Scope<'t>: durable_steps::storage::TransactionScope<Database = D>,
    D::Arguments: IntoArguments<D>,
    for<'value> String: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'value> Option<String>: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'row> &'row str: ColumnIndex<D::Row>,
{
    let Some(cancel) = RegistryStorage(scope).cancel(id).await? else {
        return Ok(None);
    };
    let admission =
        RegistryStorage(scope)
            .admission(id)
            .await?
            .ok_or_else(|| LeaseError::Corrupt {
                field: "cancel admission".into(),
            })?;
    crate::transfer_admission::bound_operation(
        scope,
        admission.operation,
        "fleet-sync.holder-change",
        &holder_change_request_key(id)?,
    )
    .await?;
    let mut record = match RegistryStorage(scope).transfer(id).await? {
        Some(record) => record,
        None => TransferRecord::requested(
            TransferIntent {
                request: admission.request.clone(),
                new_epoch: None,
                target_schema: admission.target_schema,
            },
            admission.admitted_at,
        ),
    };
    if let Some(answer) = record.outcome {
        return Ok(Some(answer));
    }
    if !record.phase.cancellable() {
        return Ok(None);
    }
    if record.phase == HolderChangePhase::Cancelling {
        return Ok(Some(HolderChangeAnswer::Pending { transfer: id }));
    }
    let move_owed = matches!(record.intent.request, HolderChangeRequest::Move { .. })
        && matches!(
            record.phase,
            HolderChangePhase::Releasing | HolderChangePhase::Released { .. }
        );
    let claim_owed = matches!(record.intent.request, HolderChangeRequest::Claim { .. })
        && record.intent.new_epoch.is_some();
    if move_owed {
        record.enter(HolderChangePhase::Cancelling, now)?;
        RegistryStorage(scope).save_transfer(&record, now).await?;
        let admitted = scope
            .admit::<HolderChangeCancelTask<S, D>>(&cancel_task_key(id)?, &id)
            .await
            .map_err(|error| LeaseError::InvalidInput {
                field: error.to_string(),
            })?;
        let mut admission = admission;
        admission.follow_up = Some(admitted.id());
        RegistryStorage(scope)
            .save_admission(&admission, now)
            .await?;
        return Ok(Some(HolderChangeAnswer::Pending { transfer: id }));
    }
    let answer = HolderChangeAnswer::Cancelled { by: cancel.by };
    record.outcome = Some(answer.clone());
    record.enter(HolderChangePhase::Ended, now)?;
    RegistryStorage(scope).save_transfer(&record, now).await?;
    if claim_owed {
        owe_settlement::<S, D, Settlement>(scope, id, SettleOutcome::Cancelled, now).await?;
    }
    Ok(Some(answer))
}

pub(crate) fn cancel_task_key(id: TransferId) -> Result<RequestKey, LeaseError> {
    RequestKey::new(format!("holder-change-cancel:{}", id.as_uuid())).map_err(|error| {
        LeaseError::InvalidInput {
            field: error.to_string(),
        }
    })
}

/// Owe the claim `id`'s settlement with `outcome`, through the host that owns
/// settlements.
pub(crate) async fn owe_settlement<S, D, Settlement>(
    scope: &mut S::Scope<'_>,
    id: TransferId,
    outcome: SettleOutcome,
    now: SyncTime,
) -> Result<(), LeaseError>
where
    S: AtomicStore,
    D: Database,
    Settlement: crate::OwedSettlement<S, D>,
    for<'t> S::Scope<'t>: durable_steps::storage::TransactionScope<Database = D>,
    D::Arguments: IntoArguments<D>,
    for<'value> String: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'value> Option<String>: Type<D> + Encode<'value, D> + Decode<'value, D>,
    for<'row> &'row str: ColumnIndex<D::Row>,
{
    let admission = RegistryStorage(&mut *scope)
        .admission(id)
        .await?
        .ok_or_else(|| LeaseError::Corrupt {
            field: "settlement admission".into(),
        })?;
    Settlement::owe_settlement(
        scope,
        id,
        fleet_protocol::ClaimSettlement {
            change: admission.request,
            outcome,
        },
        now,
    )
    .await
}
