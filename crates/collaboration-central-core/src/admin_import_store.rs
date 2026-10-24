//! Fleet-owned continuation rows do not confer registry or copy authority.

use crate::{
    admin_import_contract::{AdminImportHeader, AdminImportPhase, AdminImportRecord},
    generated_admin_imports,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{DatabaseId, ImportDigest, ReplicationPosition, RequestId};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum AdminImportCommand {
    Reserve {
        header: Box<AdminImportHeader>,
        candidate: RequestId,
    },
    Read {
        database: DatabaseId,
        digest: ImportDigest,
    },
    Published {
        database: DatabaseId,
        digest: ImportDigest,
        import_id: RequestId,
        position: ReplicationPosition,
    },
    Complete {
        database: DatabaseId,
        digest: ImportDigest,
        import_id: RequestId,
    },
}

impl AdminImportCommand {
    pub(crate) fn database(&self) -> DatabaseId {
        match self {
            Self::Reserve { header, .. } => header.database(),
            Self::Read { database, .. }
            | Self::Published { database, .. }
            | Self::Complete { database, .. } => *database,
        }
    }
    fn digest(&self) -> &ImportDigest {
        match self {
            Self::Reserve { header, .. } => &header.digest,
            Self::Read { digest, .. }
            | Self::Published { digest, .. }
            | Self::Complete { digest, .. } => digest,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AdminImportRefusal {
    NotFound,
    ImportAttemptInProgress {
        provenance: fleet_admission::ImportProvenance,
        #[serde(rename = "declaredOwner")]
        declared_owner: message_board::HumanId,
    },
    InvalidHeader,
    IdentityMismatch,
    NotPublished,
    PositionMismatch,
    PageRefused(fleet_sync_protocol::SeedPageAnswer),
    SeedRefused(fleet_sync_protocol::SeedAnswer),
    CopyRefused(fleet_sync_protocol::PieceRefusal),
}

pub(crate) struct AdminImportStep(pub AdminImportCommand);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdminImportStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Result<AdminImportRecord, AdminImportRefusal>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let database = self.0.database();
            let digest = self.0.digest();
            let stored = generated_admin_imports::read_admin_import(
                scope,
                database.to_string(),
                digest.as_str().to_owned(),
            )
            .await
            .map_err(StoreError::backend)?;
            let mut record = stored.map(decode_record).transpose()?;
            if record.as_ref().is_some_and(|record| {
                record.header.database() != database || &record.header.digest != digest
            }) {
                return Err(corrupt("admin import lookup identity"));
            }
            let outcome = match self.0 {
                AdminImportCommand::Reserve { header, candidate } => {
                    if header.pages == 0 || header.manifest.position.epoch.value() != 1 {
                        Err(AdminImportRefusal::InvalidHeader)
                    } else if let Some(saved) = record {
                        if !matches!(saved.phase, AdminImportPhase::Reserved)
                            || saved.header == *header
                        {
                            Ok(saved)
                        } else {
                            Err(AdminImportRefusal::ImportAttemptInProgress {
                                provenance: saved.header.provenance,
                                declared_owner: saved.header.declared_owner,
                            })
                        }
                    } else {
                        let saved = AdminImportRecord {
                            import_id: candidate,
                            header: *header,
                            phase: AdminImportPhase::Reserved,
                        };
                        generated_admin_imports::insert_admin_import(
                            scope,
                            saved.import_id.to_string(),
                            database.to_string(),
                            saved.header.digest.as_str().to_owned(),
                            serde_json::to_string(&saved.header).map_err(StoreError::backend)?,
                            serde_json::to_string(&saved.phase).map_err(StoreError::backend)?,
                        )
                        .await
                        .map_err(StoreError::backend)?;
                        Ok(saved)
                    }
                }
                AdminImportCommand::Read { .. } => record.ok_or(AdminImportRefusal::NotFound),
                command => match record.as_mut() {
                    None => Err(AdminImportRefusal::NotFound),
                    Some(saved) => {
                        let transition = match command {
                            AdminImportCommand::Published {
                                import_id,
                                position,
                                ..
                            } => {
                                if saved.import_id != import_id {
                                    Err(AdminImportRefusal::IdentityMismatch)
                                } else if !saved.publish(position) {
                                    Err(AdminImportRefusal::PositionMismatch)
                                } else {
                                    Ok(())
                                }
                            }
                            AdminImportCommand::Complete { import_id, .. } => {
                                if saved.import_id != import_id {
                                    Err(AdminImportRefusal::IdentityMismatch)
                                } else if !saved.complete() {
                                    Err(AdminImportRefusal::NotPublished)
                                } else {
                                    Ok(())
                                }
                            }
                            AdminImportCommand::Reserve { .. }
                            | AdminImportCommand::Read { .. } => {
                                return Err(corrupt("unexpected admin import transition"));
                            }
                        };
                        match transition {
                            Err(refusal) => Err(refusal),
                            Ok(()) => {
                                generated_admin_imports::update_admin_import(
                                    scope,
                                    serde_json::to_string(&saved.phase)
                                        .map_err(StoreError::backend)?,
                                    saved.import_id.to_string(),
                                )
                                .await
                                .map_err(StoreError::backend)?;
                                Ok(saved.clone())
                            }
                        }
                    }
                },
            };
            Ok((outcome, Vec::new()))
        })
    }
}

fn decode_record(
    row: generated_admin_imports::StoredAdminImport,
) -> Result<AdminImportRecord, StoreError> {
    let required =
        |value: Option<String>| value.ok_or_else(|| corrupt("missing admin import field"));
    let import_id = RequestId::try_from(required(row.import_id)?).map_err(StoreError::backend)?;
    let header: AdminImportHeader =
        serde_json::from_str(&required(row.header)?).map_err(StoreError::backend)?;
    let phase: AdminImportPhase =
        serde_json::from_str(&required(row.phase)?).map_err(StoreError::backend)?;
    if required(row.project_id)? != header.database().to_string()
        || required(row.digest)? != header.digest.as_str()
        || header.pages == 0
        || header.manifest.position.epoch.value() != 1
        || match phase {
            AdminImportPhase::Reserved => false,
            AdminImportPhase::Published { position } | AdminImportPhase::Complete { position } => {
                position != header.manifest.position
            }
        }
    {
        return Err(corrupt("invalid admin import record"));
    }
    Ok(AdminImportRecord {
        import_id,
        header,
        phase,
    })
}

fn corrupt(message: &str) -> StoreError {
    StoreError::Corrupt {
        field: "admin_imports".to_owned(),
        message: message.to_owned(),
    }
}
