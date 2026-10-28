//! Admin continuation composes the shared Copy seed entry and its committed summary.

use crate::{
    CallerContext, CallerKind, CopyKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime,
    PlatformIo,
    admin_import_contract::AdminImportRecord,
    admin_import_fleet::AdminImportWorkflow,
    admin_import_store::AdminImportCommand,
    caller_context::CallAuthority,
    copy_write_calls::{AdminCopySeedRequest, CopyAdminSeed},
    object_call_envelope::{ObjectCallEnvelope, encode_shared_call},
    object_runtime::ObjectWork,
};
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, NotSentReason, OperationId, StepName,
    storage::TransactionScope,
};
use fleet_sync_protocol::{
    CopyStatusAnswer, DatabaseId, PageDigest, ReplicationAnswer, ReplicationPosition, RequestId,
    SeedAnswer, SeedAttemptId, SeedComplete, SeedPage, SnapshotPage,
};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Serialize, Deserialize)]
pub(crate) struct AdminCopySeed {
    pub database: DatabaseId,
}
impl CallTarget for AdminCopySeed {
    const KIND: &'static str = "central.admin-copy-seed";
    type Request = AdminSeedRequest;
    type Answer = AdminSeedAnswer;
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum AdminSeedRequest {
    Begin {
        import_id: RequestId,
    },
    Page {
        import_id: RequestId,
        page: u32,
        contents: Box<SnapshotPage>,
        digest: PageDigest,
    },
    Complete(Box<AdminImportRecord>),
    ReconcilePublication(Box<AdminImportRecord>),
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum AdminSeedAnswer {
    Replication(ReplicationAnswer),
    Unpublished,
    Published { position: ReplicationPosition },
    Retired,
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<CopyKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> f64:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Vec<u8>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn admin_copy_seed(
        &self,
        mut call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source_face != CallerKind::Edge
            || call.source != ObjectAddress::Registry
            || !matches!(call.authority, CallAuthority::Owner(_))
        {
            return Err(refused());
        }
        let target: AdminCopySeed =
            serde_json::from_value(call.target.clone()).map_err(|_| refused())?;
        let request: AdminSeedRequest =
            serde_json::from_value(call.request.clone()).map_err(|_| refused())?;
        let ObjectWork::Copy { database, .. } = &self.work else {
            return Err(refused());
        };
        if target.database != *database {
            return Err(refused());
        }
        let answer = match request {
            AdminSeedRequest::Begin { import_id } => AdminSeedAnswer::Replication(
                self.shared_admin_seed(
                    &mut call,
                    AdminCopySeedRequest::Begin {
                        database: *database,
                        attempt: seed_attempt(import_id)?,
                    },
                )
                .await?,
            ),
            AdminSeedRequest::Page {
                import_id,
                page,
                contents,
                digest,
            } => {
                match self
                    .shared_admin_seed(
                        &mut call,
                        AdminCopySeedRequest::Begin {
                            database: *database,
                            attempt: seed_attempt(import_id)?,
                        },
                    )
                    .await?
                {
                    ReplicationAnswer::SeedStarted { seed } => AdminSeedAnswer::Replication(
                        self.shared_admin_seed(
                            &mut call,
                            AdminCopySeedRequest::Page(Box::new(SeedPage {
                                seed,
                                page,
                                contents: *contents,
                                digest,
                            })),
                        )
                        .await?,
                    ),
                    answer => AdminSeedAnswer::Replication(answer),
                }
            }
            AdminSeedRequest::ReconcilePublication(record) => {
                if record.header.database() != *database {
                    return Err(refused());
                }
                match self
                    .read_copy_status()
                    .await
                    .map_err(|error| error.call_failure())?
                {
                    CopyStatusAnswer::Retired => AdminSeedAnswer::Retired,
                    CopyStatusAnswer::NotInitialized => AdminSeedAnswer::Unpublished,
                    CopyStatusAnswer::Status(status) => {
                        if status.acked < record.header.manifest.position {
                            return Err(CallFailure::AnswerLost);
                        }
                        // A holder may already have advanced the mutable summary.
                        // The shared receiver retains and checks the exact published
                        // SeedComplete, so replay it without publishing a second seed.
                        if !matches!(self.complete_import_seed(&mut call, &record).await?,
                            ReplicationAnswer::SeedComplete(SeedAnswer::Published { position })
                            if position == record.header.manifest.position)
                        {
                            return Err(CallFailure::AnswerLost);
                        }
                        self.record_publication(&record, record.header.manifest.position)
                            .await?;
                        AdminSeedAnswer::Published {
                            position: record.header.manifest.position,
                        }
                    }
                }
            }
            AdminSeedRequest::Complete(record) => {
                if record.header.database() != *database {
                    return Err(refused());
                }
                let answer = self.complete_import_seed(&mut call, &record).await?;
                if let ReplicationAnswer::SeedComplete(SeedAnswer::Published { position }) = answer
                {
                    if position != record.header.manifest.position {
                        return Err(CallFailure::AnswerLost);
                    }
                    self.record_publication(&record, position).await?;
                    AdminSeedAnswer::Published { position }
                } else {
                    AdminSeedAnswer::Replication(answer)
                }
            }
        };
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }

    async fn complete_import_seed(
        &self,
        call: &mut ObjectCallEnvelope,
        record: &AdminImportRecord,
    ) -> Result<ReplicationAnswer, CallFailure> {
        match self
            .shared_admin_seed(
                call,
                AdminCopySeedRequest::Begin {
                    database: record.header.database(),
                    attempt: seed_attempt(record.import_id)?,
                },
            )
            .await?
        {
            ReplicationAnswer::SeedStarted { seed } => {
                let manifest = &record.header.manifest;
                self.shared_admin_seed(
                    call,
                    AdminCopySeedRequest::Complete {
                        done: SeedComplete {
                            database: record.header.database(),
                            seed,
                            position: manifest.position,
                            pages: record.header.pages,
                            schema: manifest.schema.clone(),
                            table_digests: manifest
                                .tables
                                .iter()
                                .map(|table| (table.table.clone(), table.digest.clone()))
                                .collect(),
                        },
                        summary: Some(record.header.summary.clone()),
                    },
                )
                .await
            }
            answer => Ok(answer),
        }
    }

    async fn shared_admin_seed(
        &self,
        call: &mut ObjectCallEnvelope,
        request: AdminCopySeedRequest,
    ) -> Result<ReplicationAnswer, CallFailure> {
        let ObjectWork::Copy {
            database, receiver, ..
        } = &self.work
        else {
            return Err(refused());
        };
        // Preserve the verified owner edge envelope. The shared entry owns all
        // receiver mutations, rollback classification and follow-up admission.
        let invocation = ObjectCallEnvelope {
            source: call.source.clone(),
            source_face: call.source_face,
            authority: call.authority.clone(),
            destination: call.destination.clone(),
            destination_face: call.destination_face,
            identity: call.identity.clone(),
            kind: CopyAdminSeed::KIND.to_owned(),
            target: serde_json::to_value(CopyAdminSeed {
                database: *database,
                copy: *receiver,
            })
            .map_err(|_| refused())?,
            request: serde_json::to_value(request).map_err(|_| refused())?,
        };
        let bytes = self.admin_seed_call(invocation).await?;
        serde_json::from_slice(&bytes).map_err(|_| CallFailure::AnswerLost)
    }

    async fn record_publication(
        &self,
        record: &AdminImportRecord,
        position: ReplicationPosition,
    ) -> Result<(), CallFailure> {
        let ObjectWork::Copy { database, io, .. } = &self.work else {
            return Err(refused());
        };
        let caller = CallerContext::<CopyKind>::for_object(*database);
        let identity = CallId {
            operation: OperationId::new(record.import_id.as_uuid()).map_err(|_| refused())?,
            step: StepName::new("admin_import_published").map_err(|_| refused())?,
        };
        let call = encode_shared_call::<CopyKind, FleetKind, AdminImportWorkflow>(
            &caller,
            (),
            AdminImportWorkflow,
            AdminImportCommand::Published {
                database: *database,
                digest: record.header.digest.clone(),
                import_id: record.import_id,
                position,
            },
            identity,
        )?;
        let bytes = io.object_call(call.destination, call.bytes).await?;
        let answer: Result<AdminImportRecord, crate::AdminImportRefusal> =
            serde_json::from_slice(&bytes).map_err(|_| CallFailure::AnswerLost)?;
        answer.map(|_| ()).map_err(|_| CallFailure::AnswerLost)
    }
}
fn seed_attempt(import_id: RequestId) -> Result<SeedAttemptId, CallFailure> {
    SeedAttemptId::new(import_id.as_uuid()).map_err(|_| refused())
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
