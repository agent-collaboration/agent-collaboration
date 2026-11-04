//! Repository associations retain the provenance of the decision that attached them.

use crate::{
    fleet_column_codec::{canonical, optional_text, text},
    fleet_record_codec::{
        FleetRecordHeader, check_millis, check_private_id, corrupt, decode_digest,
    },
    generated_fleet_repositories as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::FleetImportDigest;
use message_board::{RecordTime, RepositoryRef, WorkspaceId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    workspace: &WorkspaceId,
) -> Result<Vec<RepositoryRef>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut records = queries::read_repository_attachments(scope, workspace.as_str().into())
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            check_private_id(
                &text(row.attachment_id, row.attachment_id_class, "attachment_id")?,
                "attachment_id",
            )?;
            if text(row.workspace_id, row.workspace_id_class, "workspace_id")? != workspace.as_str()
            {
                return Err(corrupt(
                    "repository.workspace",
                    "attachment belongs to another workspace",
                ));
            }
            check_millis(
                row.created_at,
                row.created_at_class,
                "repository.created_at",
            )?;
            check_millis(
                row.updated_at,
                row.updated_at_class,
                "repository.updated_at",
            )?;
            let header = FleetRecordHeader::decode(
                optional_text(
                    row.router_time,
                    row.router_time_class,
                    "repository.router_time",
                )?,
                Some(text(
                    row.time_basis,
                    row.time_basis_class,
                    "repository.time_basis",
                )?),
                Some(text(
                    row.origin_installation,
                    row.origin_installation_class,
                    "repository.origin_installation",
                )?),
                Some(text(
                    row.origin_machine,
                    row.origin_machine_class,
                    "repository.origin_machine",
                )?),
            )?;
            let imported = decode_digest(optional_text(
                row.import_digest,
                row.import_digest_class,
                "repository.import_digest",
            )?)?;
            if header.time == RecordTime::LegacyUnknown && imported.is_none() {
                return Err(corrupt(
                    "repository.time",
                    "unknown time requires import provenance",
                ));
            }
            let reference = match (
                text(
                    row.reference_kind,
                    row.reference_kind_class,
                    "repository.kind",
                )?
                .as_str(),
                optional_text(
                    row.normalized_origin,
                    row.normalized_origin_class,
                    "repository.origin",
                )?,
                optional_text(row.service_id, row.service_id_class, "repository.service")?,
                optional_text(
                    row.common_directory,
                    row.common_directory_class,
                    "repository.directory",
                )?,
            ) {
                ("origin", Some(origin), None, None) => RepositoryRef::Origin {
                    normalized_origin: canonical(origin, "repository.origin")?,
                },
                ("local", None, Some(service), Some(directory)) => RepositoryRef::Local {
                    service_id: canonical(service, "repository.service")?,
                    common_directory: canonical(directory, "repository.directory")?,
                },
                _ => {
                    return Err(corrupt(
                        "repository.reference",
                        "unknown kind or fields outside its active variant",
                    ));
                }
            };
            // Preserve the previous association-index ordering, whose key was
            // the canonical consumer serialization. No JSON value is stored.
            Ok((
                serde_json::to_string(&reference).map_err(StoreError::backend)?,
                reference,
            ))
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    records.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(records
        .into_iter()
        .map(|(_, reference)| reference)
        .collect())
}

pub(crate) async fn attach<Scope: TransactionScope>(
    scope: &mut Scope,
    workspace: &WorkspaceId,
    repository: &RepositoryRef,
    header: &FleetRecordHeader,
    imported: Option<&FleetImportDigest>,
    attachment_id: durable_steps::Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    if header.time == RecordTime::LegacyUnknown && imported.is_none() {
        return Err(corrupt(
            "repository.time",
            "live attachment requires Router time",
        ));
    }
    crate::fleet_row_ids::validate_row_id(attachment_id)?;
    let columns = ReferenceColumns::from(repository);
    queries::attach_repository(
        scope,
        attachment_id.to_string(),
        workspace.as_str().into(),
        columns.kind.into(),
        columns.origin,
        columns.service,
        columns.directory,
        header.router_time(),
        header.time_basis(),
        header.origin.installation.as_str().into(),
        header.origin.machine.as_str().into(),
        imported.map(|digest| digest.as_str().into()),
        now.timestamp_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

pub(crate) async fn detach<Scope: TransactionScope>(
    scope: &mut Scope,
    workspace: &WorkspaceId,
    repository: &RepositoryRef,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let columns = ReferenceColumns::from(repository);
    queries::detach_repository(
        scope,
        workspace.as_str().into(),
        columns.kind.into(),
        columns.origin,
        columns.service,
        columns.directory,
    )
    .await
    .map_err(StoreError::backend)
}

struct ReferenceColumns {
    kind: &'static str,
    origin: Option<String>,
    service: Option<String>,
    directory: Option<String>,
}
impl From<&RepositoryRef> for ReferenceColumns {
    fn from(reference: &RepositoryRef) -> Self {
        match reference {
            RepositoryRef::Origin { normalized_origin } => Self {
                kind: "origin",
                origin: Some(normalized_origin.as_str().into()),
                service: None,
                directory: None,
            },
            RepositoryRef::Local {
                service_id,
                common_directory,
            } => Self {
                kind: "local",
                origin: None,
                service: Some(service_id.as_str().into()),
                directory: Some(common_directory.as_str().into()),
            },
        }
    }
}
