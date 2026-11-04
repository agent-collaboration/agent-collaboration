//! Fleet-owned continuation rows do not confer registry or copy authority.

use crate::{
    admin_import_contract::{
        AdminImportHeader, AdminImportPhase, AdminImportRecord, AdminImportRefusal,
    },
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

pub(crate) struct AdminImportStep {
    pub command: AdminImportCommand,
    pub now: runtime_primitives::SyncTime,
    pub row_ids: crate::fleet_row_ids::FleetRowIds,
}
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
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let database = self.command.database();
            let digest = self.command.digest();
            let mut record = crate::admin_import_columns::read(scope, database, digest).await?;
            let outcome = match self.command {
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
                        crate::admin_import_writes::insert(
                            scope,
                            &saved,
                            self.now,
                            &mut self.row_ids,
                        )
                        .await?;
                        Ok(saved)
                    }
                }
                AdminImportCommand::Read { .. } => record.ok_or(AdminImportRefusal::NotFound),
                command => match record.as_mut() {
                    None => Err(AdminImportRefusal::NotFound),
                    Some(saved) => {
                        let previous_phase = saved.phase.clone();
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
                                if saved.phase != previous_phase {
                                    crate::admin_import_writes::update_phase(
                                        scope, saved, self.now,
                                    )
                                    .await?;
                                }
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

fn corrupt(message: &str) -> StoreError {
    StoreError::Corrupt {
        field: "admin_imports".to_owned(),
        message: message.to_owned(),
    }
}
