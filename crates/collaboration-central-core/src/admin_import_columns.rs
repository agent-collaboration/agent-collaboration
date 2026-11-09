//! Decode an import's immutable header only after its exact ordered manifest is present.
use crate::{
    admin_import_contract::{AdminImportHeader, AdminImportPhase, AdminImportRecord},
    fleet_column_codec::{canonical, decimal, optional_text, text, unsigned},
    fleet_row_codec::{check_millis, check_private_id, corrupt},
    generated_admin_imports::{StoredAdminImport, StoredAdminManifest},
};
use board_sync_protocol::BoardAnnotation;
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_protocol::{
    CommitSequence, DatabaseId, Epoch, ImportDigest, ReplicationPosition, RequestId,
    SchemaIdentity, SnapshotManifest, TableDigest, TableManifest, TableName,
};
use message_board::RecordTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<S: TransactionScope>(
    scope: &mut S,
    database: DatabaseId,
    digest: &ImportDigest,
) -> Result<Option<AdminImportRecord>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let Some(row) = crate::generated_admin_imports::read_admin_import(
        scope,
        database.to_string(),
        digest.as_str().into(),
    )
    .await
    .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    let root = ImportRoot::decode(row)?;
    if root.record.header.database() != database || &root.record.header.digest != digest {
        return Err(corrupt(
            "admin_import.lookup",
            "lookup returned another import",
        ));
    }
    let limit = root
        .table_count
        .checked_add(1)
        .ok_or_else(|| corrupt("admin_import.tables", "manifest count overflow"))?;
    let rows = crate::generated_admin_imports::read_admin_manifest(
        scope,
        root.record.import_id.to_string(),
        limit.to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    root.finish(rows).map(Some)
}

struct ImportRoot {
    record: AdminImportRecord,
    table_count: u64,
}
impl ImportRoot {
    fn decode(row: StoredAdminImport) -> Result<Self, StoreError> {
        let id = text(row.import_id, row.import_id_class, "admin_import.id")?;
        check_private_id(&id, "admin_import.id")?;
        let import_id =
            RequestId::try_from(id).map_err(|error| corrupt("admin_import.id", error))?;
        let project = text(row.project_id, row.project_id_class, "admin_import.project")?;
        check_private_id(&project, "admin_import.project")?;
        let database = DatabaseId::try_from(project)
            .map_err(|error| corrupt("admin_import.project", error))?;
        let raw_digest = text(row.digest, row.digest_class, "admin_import.digest")?;
        let digest = ImportDigest::new(raw_digest.clone())
            .map_err(|error| corrupt("admin_import.digest", error))?;
        if digest.as_str() != raw_digest {
            return Err(corrupt("admin_import.digest", "noncanonical digest"));
        }
        check_millis(
            row.created_at,
            row.created_at_class,
            "admin_import.created_at",
        )?;
        check_millis(
            row.updated_at,
            row.updated_at_class,
            "admin_import.updated_at",
        )?;
        let position = ReplicationPosition::new(
            Epoch::new(decimal(
                &text(
                    row.manifest_epoch,
                    row.manifest_epoch_class,
                    "admin_import.epoch",
                )?,
                "admin_import.epoch",
            )?)
            .map_err(|error| corrupt("admin_import.epoch", error))?,
            CommitSequence::new(decimal(
                &text(
                    row.manifest_sequence,
                    row.manifest_sequence_class,
                    "admin_import.sequence",
                )?,
                "admin_import.sequence",
            )?),
        );
        let pages = u32::try_from(unsigned(row.pages, row.pages_class, "admin_import.pages")?)
            .map_err(|error| corrupt("admin_import.pages", error))?;
        if pages == 0 || position.epoch.value() != 1 {
            return Err(corrupt(
                "admin_import.header",
                "invalid pages or import epoch",
            ));
        }
        let schema = SchemaIdentity::new(
            decimal(
                &text(
                    row.schema_version,
                    row.schema_version_class,
                    "admin_import.schema_version",
                )?,
                "admin_import.schema_version",
            )?,
            text(
                row.schema_fingerprint,
                row.schema_fingerprint_class,
                "admin_import.schema_fingerprint",
            )?,
        )
        .map_err(|error| corrupt("admin_import.schema", error))?;
        let summary_kind = text(
            row.summary_kind,
            row.summary_kind_class,
            "admin_import.summary_kind",
        )?;
        if summary_kind != "project_summary" {
            return Err(corrupt("admin_import.summary_kind", "unknown summary kind"));
        }
        let time = decode_time(
            text(
                row.summary_time_basis,
                row.summary_time_basis_class,
                "admin_import.time_basis",
            )?,
            optional_text(
                row.summary_record_at,
                row.summary_record_at_class,
                "admin_import.record_at",
            )?,
        )?;
        let phase = match (
            text(
                row.phase_kind,
                row.phase_kind_class,
                "admin_import.phase_kind",
            )?
            .as_str(),
            optional_text(
                row.published_epoch,
                row.published_epoch_class,
                "admin_import.published_epoch",
            )?,
            optional_text(
                row.published_sequence,
                row.published_sequence_class,
                "admin_import.published_sequence",
            )?,
        ) {
            ("reserved", None, None) => AdminImportPhase::Reserved,
            (kind @ ("published" | "complete"), Some(epoch), Some(sequence)) => {
                let published = ReplicationPosition::new(
                    Epoch::new(decimal(&epoch, "admin_import.published_epoch")?)
                        .map_err(|error| corrupt("admin_import.published_epoch", error))?,
                    CommitSequence::new(decimal(&sequence, "admin_import.published_sequence")?),
                );
                if published != position {
                    return Err(corrupt(
                        "admin_import.phase",
                        "publication differs from manifest",
                    ));
                }
                if kind == "published" {
                    AdminImportPhase::Published {
                        position: published,
                    }
                } else {
                    AdminImportPhase::Complete {
                        position: published,
                    }
                }
            }
            _ => {
                return Err(corrupt(
                    "admin_import.phase",
                    "unknown phase or partial/inactive publication",
                ));
            }
        };
        Ok(Self {
            table_count: unsigned(
                row.manifest_table_count,
                row.manifest_table_count_class,
                "admin_import.table_count",
            )?,
            record: AdminImportRecord {
                import_id,
                phase,
                header: AdminImportHeader {
                    digest,
                    pages,
                    summary: BoardAnnotation::ProjectSummary {
                        latest_record_at: time,
                    },
                    provenance: fleet_admission::ImportProvenance {
                        installation: canonical(
                            text(
                                row.provenance_installation,
                                row.provenance_installation_class,
                                "admin_import.installation",
                            )?,
                            "admin_import.installation",
                        )?,
                        machine: canonical(
                            text(
                                row.provenance_machine,
                                row.provenance_machine_class,
                                "admin_import.machine",
                            )?,
                            "admin_import.machine",
                        )?,
                    },
                    declared_owner: canonical(
                        text(
                            row.declared_owner,
                            row.declared_owner_class,
                            "admin_import.owner",
                        )?,
                        "admin_import.owner",
                    )?,
                    manifest: SnapshotManifest {
                        database,
                        position,
                        schema,
                        tables: Vec::new(),
                    },
                },
            },
        })
    }
    fn finish(mut self, rows: Vec<StoredAdminManifest>) -> Result<AdminImportRecord, StoreError> {
        if u64::try_from(rows.len()).ok() != Some(self.table_count) {
            return Err(corrupt(
                "admin_import.tables",
                "missing or extra manifest rows",
            ));
        }
        for (ordinal, row) in rows.into_iter().enumerate() {
            let id = text(row.row_id, row.row_id_class, "admin_import.table.id")?;
            check_private_id(&id, "admin_import.table.id")?;
            let parent = text(
                row.import_id,
                row.import_id_class,
                "admin_import.table.parent",
            )?;
            check_private_id(&parent, "admin_import.table.parent")?;
            if parent != self.record.import_id.to_string()
                || unsigned(row.ordinal, row.ordinal_class, "admin_import.table.ordinal")?
                    != u64::try_from(ordinal).map_err(StoreError::backend)?
            {
                return Err(corrupt(
                    "admin_import.table.ordinal",
                    "wrong parent or noncontiguous order",
                ));
            }
            check_millis(
                row.created_at,
                row.created_at_class,
                "admin_import.table.created_at",
            )?;
            check_millis(
                row.updated_at,
                row.updated_at_class,
                "admin_import.table.updated_at",
            )?;
            let raw = text(
                row.table_digest,
                row.table_digest_class,
                "admin_import.table.digest",
            )?;
            let digest = TableDigest::new(raw.clone())
                .map_err(|error| corrupt("admin_import.table.digest", error))?;
            if digest.as_str() != raw {
                return Err(corrupt(
                    "admin_import.table.digest",
                    "noncanonical table digest",
                ));
            }
            self.record.header.manifest.tables.push(TableManifest {
                table: TableName::new(text(
                    row.table_name,
                    row.table_name_class,
                    "admin_import.table.name",
                )?)
                .map_err(|error| corrupt("admin_import.table.name", error))?,
                row_count: decimal(
                    &text(
                        row.row_count,
                        row.row_count_class,
                        "admin_import.table.count",
                    )?,
                    "admin_import.table.count",
                )?,
                digest,
            });
        }
        Ok(self.record)
    }
}

fn decode_time(basis: String, at: Option<String>) -> Result<RecordTime, StoreError> {
    match (basis.as_str(), at) {
        ("legacy_unknown", None) => Ok(RecordTime::LegacyUnknown),
        ("router", Some(value)) => {
            let time: chrono::DateTime<chrono::Utc> = value
                .parse()
                .map_err(|error| corrupt("admin_import.record_at", error))?;
            if time.to_rfc3339() != value
                && time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true) != value
            {
                return Err(corrupt(
                    "admin_import.record_at",
                    "noncanonical record time",
                ));
            }
            Ok(RecordTime::Router(time))
        }
        _ => Err(corrupt(
            "admin_import.time",
            "unknown time basis or inactive record time",
        )),
    }
}
