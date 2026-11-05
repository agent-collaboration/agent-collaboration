//! Reserve a complete header atomically; phase changes never rewrite its manifest.
use crate::{
    admin_import_contract::{AdminImportPhase, AdminImportRecord},
    fleet_row_codec::corrupt,
};
use durable_steps::{StoreError, storage::TransactionScope};
use runtime_primitives::SyncTime;
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};

pub(crate) async fn insert<S: TransactionScope>(
    scope: &mut S,
    record: &AdminImportRecord,
    now: SyncTime,
    ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Encode<'v, S::Database>,
{
    let header = &record.header;
    if header.pages == 0
        || header.manifest.position.epoch.value() != 1
        || !matches!(record.phase, AdminImportPhase::Reserved)
    {
        return Err(corrupt(
            "admin_import.header",
            "new header requires valid Reserved state",
        ));
    }
    fleet_sync_protocol::SchemaIdentity::new(
        header.manifest.schema.version,
        header.manifest.schema.fingerprint.clone(),
    )
    .map_err(|error| corrupt("admin_import.schema", error))?;
    let board_sync_protocol::BoardAnnotation::ProjectSummary { latest_record_at } = header.summary;
    let (basis, at) = match latest_record_at {
        message_board::RecordTime::LegacyUnknown => ("legacy_unknown", None),
        message_board::RecordTime::Router(time) => ("router", Some(time.to_rfc3339())),
    };
    let count = i64::try_from(header.manifest.tables.len()).map_err(StoreError::backend)?;
    crate::generated_admin_imports::insert_admin_import(
        scope,
        record.import_id.to_string(),
        header.database().to_string(),
        header.digest.as_str().into(),
        header.provenance.installation.as_str().into(),
        header.provenance.machine.as_str().into(),
        header.declared_owner.as_str().into(),
        header.manifest.position.epoch.value().to_string(),
        header.manifest.position.sequence.value().to_string(),
        header.manifest.schema.version.to_string(),
        header.manifest.schema.fingerprint.clone(),
        header.pages.to_string(),
        count.to_string(),
        "project_summary".into(),
        basis.into(),
        at,
        "reserved".into(),
        None,
        None,
        now.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    for (ordinal, table) in header.manifest.tables.iter().enumerate() {
        crate::generated_admin_imports::insert_admin_manifest(
            scope,
            ids.next()?.to_string(),
            record.import_id.to_string(),
            ordinal.to_string(),
            table.table.as_str().into(),
            table.row_count.to_string(),
            table.digest.as_str().into(),
            now.unix_millis().to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
    }
    Ok(())
}

pub(crate) async fn update_phase<S: TransactionScope>(
    scope: &mut S,
    record: &AdminImportRecord,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Encode<'v, S::Database>,
{
    let (kind, position) = match record.phase {
        AdminImportPhase::Reserved => ("reserved", None),
        AdminImportPhase::Published { position } => ("published", Some(position)),
        AdminImportPhase::Complete { position } => ("complete", Some(position)),
    };
    if position.is_some_and(|position| position != record.header.manifest.position) {
        return Err(corrupt(
            "admin_import.phase",
            "publication differs from immutable manifest",
        ));
    }
    crate::generated_admin_imports::update_admin_import(
        scope,
        kind.into(),
        position.map(|position| position.epoch.value().to_string()),
        position.map(|position| position.sequence.value().to_string()),
        now.unix_millis().to_string(),
        record.import_id.to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
