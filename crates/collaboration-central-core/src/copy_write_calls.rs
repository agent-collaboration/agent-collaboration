//! Authenticated Session writes and owner seed publication enter the same copy rules.
use crate::{
    CallerKind, CopyDirectorySummary, CopyKind, CoreHost, ObjectAddress, ObjectHost, ObjectRuntime,
    PlatformIo,
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use board_sync_protocol::BoardAnnotation;
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, Engine, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::{
    DatabaseId, PeerId, Piece, ReceiverId, ReplicationAnswer, ReplicationRequest, SeedAttemptId,
    SeedComplete, SeedPage,
};
use fleet_sync_receiver::{
    ApplyDecision, ApplyFollowUp, CopyTransactionFailure, InstalledSnapshot, SeedAuthority,
    SeedDecision,
};
use runtime_primitives::{SyncTime, WakeScheduler};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopySessionWrite {
    pub copy: ReceiverId,
    pub database: DatabaseId,
}
impl CallTarget for CopySessionWrite {
    const KIND: &'static str = "central.copy-session-write";
    type Request = WriterCopyRequest;
    type Answer = ReplicationAnswer;
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WriterCopyRequest {
    pub sender: PeerId,
    pub request: ReplicationRequest<BoardAnnotation>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopyAdminSeed {
    pub copy: ReceiverId,
    pub database: DatabaseId,
}
impl CallTarget for CopyAdminSeed {
    const KIND: &'static str = "central.copy-admin-seed";
    type Request = AdminCopySeedRequest;
    type Answer = ReplicationAnswer;
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum AdminCopySeedRequest {
    Begin {
        database: DatabaseId,
        attempt: SeedAttemptId,
    },
    Page(Box<SeedPage>),
    Complete {
        done: SeedComplete,
        summary: Option<BoardAnnotation>,
    },
}
enum CopyWrite {
    Piece {
        piece: Box<Piece<BoardAnnotation>>,
        sender: PeerId,
    },
    Begin {
        database: DatabaseId,
        attempt: SeedAttemptId,
        authority: SeedAuthority,
    },
    Page {
        page: Box<SeedPage>,
        authority: SeedAuthority,
    },
    Complete {
        done: SeedComplete,
        summary: Option<BoardAnnotation>,
        authority: SeedAuthority,
    },
}
impl CopyWrite {
    fn database(&self) -> DatabaseId {
        match self {
            Self::Piece { piece, .. } => piece.database,
            Self::Begin { database, .. } => *database,
            Self::Page { page, .. } => page.contents.database,
            Self::Complete { done, .. } => done.database,
        }
    }
    fn writer(request: WriterCopyRequest) -> Result<Self, CallFailure> {
        let authority = SeedAuthority::Writer(request.sender);
        Ok(match request.request {
            ReplicationRequest::Piece(piece) => Self::Piece {
                piece: Box::new(piece),
                sender: request.sender,
            },
            ReplicationRequest::SeedBegin { database, attempt } => Self::Begin {
                database,
                attempt,
                authority,
            },
            ReplicationRequest::SeedPage(page) => Self::Page { page, authority },
            ReplicationRequest::SeedComplete(done) => Self::Complete {
                done,
                summary: None,
                authority,
            },
            _ => return Err(refused()),
        })
    }
    fn admin(request: AdminCopySeedRequest) -> Self {
        match request {
            AdminCopySeedRequest::Begin { database, attempt } => Self::Begin {
                database,
                attempt,
                authority: SeedAuthority::Admin,
            },
            AdminCopySeedRequest::Page(page) => Self::Page {
                page,
                authority: SeedAuthority::Admin,
            },
            AdminCopySeedRequest::Complete { done, summary } => Self::Complete {
                done,
                summary,
                authority: SeedAuthority::Admin,
            },
        }
    }
}
pub(crate) fn write_database(request: &ReplicationRequest<BoardAnnotation>) -> Option<DatabaseId> {
    match request {
        ReplicationRequest::Piece(piece) => Some(piece.database),
        ReplicationRequest::SeedBegin { database, .. } => Some(*database),
        ReplicationRequest::SeedPage(page) => Some(page.contents.database),
        ReplicationRequest::SeedComplete(done) => Some(done.database),
        _ => None,
    }
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
    pub(crate) async fn writer_copy_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if !matches!(call.authority, CallAuthority::Object)
            || call.source_face != CallerKind::Session
            || !matches!(call.source, ObjectAddress::Connection(_))
        {
            return Err(refused());
        }
        let target: CopySessionWrite = decode(call.target)?;
        let request = CopyWrite::writer(decode(call.request)?)?;
        self.checked_write(target.copy, target.database, request)
            .await
    }
    pub(crate) async fn admin_seed_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if !matches!(call.authority, CallAuthority::Owner(_))
            || call.source_face != CallerKind::Edge
        {
            return Err(refused());
        }
        let target: CopyAdminSeed = decode(call.target)?;
        self.checked_write(
            target.copy,
            target.database,
            CopyWrite::admin(decode(call.request)?),
        )
        .await
    }
    async fn checked_write(
        &self,
        copy: ReceiverId,
        database: DatabaseId,
        request: CopyWrite,
    ) -> Result<Vec<u8>, CallFailure> {
        let ObjectWork::Copy {
            receiver,
            database: expected,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        if copy != *receiver || database != *expected || request.database() != *expected {
            return Err(refused());
        }
        serde_json::to_vec(&self.write_copy(request).await).map_err(|_| CallFailure::AnswerLost)
    }
    async fn write_copy(&self, request: CopyWrite) -> ReplicationAnswer {
        let ObjectWork::Copy {
            host, io, changes, ..
        } = &self.work
        else {
            return failure(StoreError::Aborted {
                reason: "copy write reached another face".into(),
            });
        };
        match StepDriver::new(host.clone())
            .run(WriteCopy {
                request,
                now: io.now(),
            })
            .await
        {
            Ok((answer, committed)) => {
                for effect in committed.into_effects() {
                    match effect {
                        CopyWriteEffect::Changed => changes.signal_committed(),
                        CopyWriteEffect::Activate => {
                            if host.wake().wake_at(io.now()).await.is_err() {
                                eprintln!("copy follow-up activation failed; journal retains work");
                            }
                        }
                        CopyWriteEffect::StagedExpiry(at) => {
                            if let Err(error) = host.wake().wake_at(at).await {
                                return failure(StoreError::backend(error));
                            }
                        }
                    }
                }
                answer
            }
            Err(error) => match fleet_sync_receiver::classify_copy_store_error(error) {
                CopyTransactionFailure::RefusedDiscardingParts {
                    refusal,
                    transaction,
                } => {
                    match StepDriver::new(host.clone())
                        .run(DiscardParts(transaction))
                        .await
                    {
                        Ok(_) => ReplicationAnswer::Refused(refusal),
                        Err(error) => failure(error),
                    }
                }
                failed => failed.answer(),
            },
        }
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<CopyKind, Host, Io> {
    pub(crate) async fn drive_copy_followups(&self) -> Result<(), StoreError> {
        let ObjectWork::Copy {
            host,
            io,
            receiver,
            database,
            ..
        } = &self.work
        else {
            return Err(StoreError::Aborted {
                reason: "copy alarm reached another face".into(),
            });
        };
        let shared = CoreHost::for_copy(io.clone(), *receiver, *database)
            .map_err(|error| StoreError::backend(format!("{error:?}")))?;
        let mut engine = Engine::new(host.store().clone(), Arc::new(shared));
        engine.register(ApplyFollowUp::<Host::Store, CopyDirectorySummary>::new())?;
        engine.drive_due().await?;
        Ok(())
    }
}

enum CopyWriteEffect {
    Changed,
    Activate,
    StagedExpiry(SyncTime),
}
struct WriteCopy {
    request: CopyWrite,
    now: SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for WriteCopy
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> f64: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Vec<u8>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ReplicationAnswer;
    type Effect = CopyWriteEffect;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let mut effects = Vec::new();
            let answer = match self.request {
                CopyWrite::Piece { piece, sender } => {
                    let rules = crate::CopyReceiverRules::new(self.now, None)?;
                    match fleet_sync_receiver::apply_piece(scope, &piece, &sender, &rules, self.now)
                        .await
                        .map_err(fleet_sync_receiver::apply_rollback)?
                    {
                        ApplyDecision::Applied {
                            through,
                            follow_ups,
                        } => {
                            if !follow_ups.is_empty() {
                                fleet_sync_receiver::admit_follow_ups::<Store,CopyDirectorySummary>(scope,piece.database,piece.epoch,through,follow_ups).await.map_err(StoreError::backend)?;
                                effects.push(CopyWriteEffect::Activate);
                            }
                            effects.push(CopyWriteEffect::Changed);
                            ReplicationAnswer::Applied { through }
                        }
                        ApplyDecision::AlreadyApplied { through } => {
                            ReplicationAnswer::AlreadyApplied { through }
                        }
                        ApplyDecision::PartStaged {
                            transaction,
                            index,
                            acked,
                        } => {
                            let at = crate::copy_staged_parts::next_expiry(scope)
                                .await?
                                .ok_or_else(|| {
                                    StoreError::backend("PartStaged has no staged-part expiry")
                                })?;
                            effects.push(CopyWriteEffect::StagedExpiry(at));
                            ReplicationAnswer::PartStaged {
                                transaction,
                                index,
                                acked,
                            }
                        }
                    }
                }
                CopyWrite::Begin {
                    database,
                    attempt,
                    authority,
                } => ReplicationAnswer::SeedStarted {
                    seed: fleet_sync_receiver::seed_begin(scope, database, attempt, &authority)
                        .await
                        .map_err(fleet_sync_receiver::receiver_rollback)?,
                },
                CopyWrite::Page { page, authority } => ReplicationAnswer::SeedPage(
                    fleet_sync_receiver::seed_page(scope, &page, &authority)
                        .await
                        .map_err(fleet_sync_receiver::receiver_rollback)?,
                ),
                CopyWrite::Complete {
                    done,
                    summary,
                    authority,
                } => {
                    let rules = crate::CopyReceiverRules::new(self.now, summary)?;
                    let answer =
                        match fleet_sync_receiver::seed_complete(scope, &done, &rules, &authority)
                            .await
                            .map_err(fleet_sync_receiver::receiver_rollback)?
                        {
                            SeedDecision::Published {
                                position,
                                follow_ups,
                            } => {
                                if !follow_ups.is_empty() {
                                    let installed = InstalledSnapshot {
                                        database: done.database,
                                        position,
                                        schema: done.schema,
                                    };
                                    fleet_sync_receiver::admit_snapshot_follow_ups::<
                                        Store,
                                        CopyDirectorySummary,
                                    >(
                                        scope, &installed, follow_ups
                                    )
                                    .await
                                    .map_err(StoreError::backend)?;
                                    effects.push(CopyWriteEffect::Activate);
                                }
                                effects.push(CopyWriteEffect::Changed);
                                fleet_sync_protocol::SeedAnswer::Published { position }
                            }
                            SeedDecision::Answer(answer) => answer,
                        };
                    ReplicationAnswer::SeedComplete(answer)
                }
            };
            Ok((answer, effects))
        })
    }
}
struct DiscardParts(fleet_sync_protocol::TransactionKey);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for DiscardParts
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ();
    type Effect = std::convert::Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<(), Self::Effect>> {
        Box::pin(async move {
            fleet_sync_receiver::discard_staged_parts(scope, &self.0)
                .await
                .map_err(fleet_sync_receiver::receiver_rollback)?;
            Ok(((), Vec::new()))
        })
    }
}
fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, CallFailure> {
    serde_json::from_value(value).map_err(crate::object_call_envelope::encode_refusal)
}
fn failure(error: StoreError) -> ReplicationAnswer {
    fleet_sync_receiver::classify_copy_store_error(error).answer()
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: durable_steps::NotSentReason::Unreachable { last_seen: None },
    }
}
