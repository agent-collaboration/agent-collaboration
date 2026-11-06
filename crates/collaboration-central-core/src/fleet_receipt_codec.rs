//! Historical receipt references and placements never depend on current resource rows.
use crate::{
    fleet_column_codec::{canonical, text, unsigned},
    fleet_row_codec::{check_millis, check_private_id, corrupt},
    generated_statements::StoredFleetDecisionReceipt,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::{
    ActorOrigin, DecisionEffect, FleetFeedEntry, FleetPosition, RecordKind, RecordPlacement,
    RecordRef, RequestId,
};
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};

pub(crate) fn decode(
    row: StoredFleetDecisionReceipt,
    request: &RequestId,
    ordinal: u64,
) -> Result<DecisionEffect, StoreError> {
    check_private_id(
        &text(row.receipt_id, row.receipt_id_class, "receipt.id")?,
        "receipt.id",
    )?;
    let stored: RequestId = canonical(
        text(row.request_id, row.request_id_class, "receipt.request")?,
        "receipt.request",
    )?;
    if stored != *request || unsigned(row.ordinal, row.ordinal_class, "receipt.ordinal")? != ordinal
    {
        return Err(corrupt(
            "receipt.ordinal",
            "wrong request or noncontiguous ordinal",
        ));
    }
    check_millis(row.created_at, row.created_at_class, "receipt.created_at")?;
    check_millis(row.updated_at, row.updated_at_class, "receipt.updated_at")?;
    let kind = match text(
        row.record_kind,
        row.record_kind_class,
        "receipt.record_kind",
    )?
    .as_str()
    {
        "workspace" => RecordKind::Workspace,
        "board" => RecordKind::Board,
        "adr" => RecordKind::Adr,
        "project" => RecordKind::Project,
        _ => {
            return Err(corrupt(
                "receipt.record_kind",
                "unknown or project-only record kind",
            ));
        }
    };
    let effect = (
        RecordRef {
            kind,
            id: canonical(
                text(row.record_id, row.record_id_class, "receipt.record_id")?,
                "receipt.record_id",
            )?,
        },
        RecordPlacement::Fleet(FleetFeedEntry {
            position: FleetPosition::try_from(unsigned(
                row.fleet_position,
                row.fleet_position_class,
                "receipt.position",
            )?)
            .map_err(|error| corrupt("receipt.position", error))?,
            origin: ActorOrigin {
                installation: canonical(
                    text(
                        row.origin_installation,
                        row.origin_installation_class,
                        "receipt.installation",
                    )?,
                    "receipt.installation",
                )?,
                machine: canonical(
                    text(
                        row.origin_machine,
                        row.origin_machine_class,
                        "receipt.machine",
                    )?,
                    "receipt.machine",
                )?,
            },
        }),
    );
    message_board::validate_fleet_decision_effect(&effect)
        .map_err(|error| corrupt("receipt.effect", error))?;
    Ok(effect)
}

pub(crate) async fn insert<S: TransactionScope>(
    scope: &mut S,
    request: &RequestId,
    id: durable_steps::Uuid,
    ordinal: usize,
    effect: &DecisionEffect,
    now: runtime_primitives::SyncTime,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
{
    crate::fleet_row_ids::validate_row_id(id)?;
    message_board::validate_fleet_decision_effect(effect)
        .map_err(|error| corrupt("receipt.effect", error))?;
    let kind = match effect.0.kind {
        RecordKind::Workspace => "workspace",
        RecordKind::Board => "board",
        RecordKind::Adr => "adr",
        RecordKind::Project => "project",
        _ => return Err(corrupt("receipt.record_kind", "project-only record kind")),
    };
    let RecordPlacement::Fleet(placement) = &effect.1 else {
        return Err(corrupt(
            "receipt.placement",
            "effect is not placed in Fleet",
        ));
    };
    let ordinal = i64::try_from(ordinal).map_err(StoreError::backend)?;
    let position = i64::try_from(placement.position.get()).map_err(StoreError::backend)?;
    crate::generated_statements::insert_fleet_decision_receipt(
        scope,
        id.to_string(),
        request.as_str().into(),
        ordinal.to_string(),
        kind.into(),
        effect.0.id.as_str().into(),
        position.to_string(),
        placement.origin.installation.as_str().into(),
        placement.origin.machine.as_str().into(),
        now.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
