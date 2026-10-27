//! Owner HTTP requests compose persisted workflow state with library authority.

use crate::{
    AccessCaller, CallOutcome, CallerContext, CentralApplication, CentralPlatform, CopyKind,
    EdgeKind, FleetCall, FleetKind, FleetResult, MayCall, ObjectKind, RegistryKind,
    admin_import_contract::{
        AdminImportPhase, AdminImportRecord, AdminProjectImportAnswer, AdminProjectImportRequest,
    },
    admin_import_copy::{AdminCopySeed, AdminSeedAnswer, AdminSeedRequest},
    admin_import_fleet::AdminImportWorkflow,
    admin_import_registry::{AdminImportReservation, ReservationAnswer},
    admin_import_store::{AdminImportCommand, AdminImportRefusal},
    object_call_envelope::encode_shared_call,
};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use durable_steps::{CallFailure, CallId, CallTarget, NotSentReason, OperationId, StepName};
use fleet_sync_protocol::{
    DatabaseId, Import, ImportAnswer, ReplicationAnswer, RequestId, SeedAnswer, SeedPageAnswer,
};
use std::sync::Arc;

pub(crate) async fn fleet<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(bundle): Json<fleet_admission::FleetImportBundle>,
) -> Response {
    match application
        .call_from_edge::<FleetKind>(
            caller,
            (),
            FleetCall::Import {
                bundle: Box::new(bundle),
            },
        )
        .await
    {
        CallOutcome::Answered(FleetResult::Imported(result)) => Json(result).into_response(),
        CallOutcome::Failed(failure) => crate::owner_http_routes::failed(failure),
    }
}

pub(crate) async fn project<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Path(project): Path<DatabaseId>,
    Json(request): Json<AdminProjectImportRequest>,
) -> Response {
    match project_request(&application, caller, project, request).await {
        Ok(AdminProjectImportAnswer::Pending) => (
            StatusCode::ACCEPTED,
            Json(AdminProjectImportAnswer::Pending),
        )
            .into_response(),
        Ok(AdminProjectImportAnswer::Retired) => (
            StatusCode::GONE,
            Json(collaboration_central_admin_protocol::OwnerOperationFailure::Retired),
        )
            .into_response(),
        Ok(AdminProjectImportAnswer::CopyUnavailable { reason }) => {
            crate::owner_http_routes::failed(fleet_sync_receiver::retryable_copy(reason))
        }
        Ok(answer) => Json(answer).into_response(),
        Err(failure) => crate::owner_http_routes::failed(failure),
    }
}

async fn project_request<Platform: CentralPlatform>(
    application: &CentralApplication<Platform>,
    caller: AccessCaller,
    database: DatabaseId,
    request: AdminProjectImportRequest,
) -> Result<AdminProjectImportAnswer, CallFailure> {
    let candidate = RequestId::new(application.platform.ids().new_id()).map_err(|_| refused())?;
    let digest = match &request {
        AdminProjectImportRequest::Begin { header } if header.database() == database => {
            header.digest.clone()
        }
        AdminProjectImportRequest::Begin { .. } => {
            return Ok(AdminProjectImportAnswer::Refused(
                AdminImportRefusal::IdentityMismatch,
            ));
        }
        AdminProjectImportRequest::Page { digest, .. }
        | AdminProjectImportRequest::Complete { digest, .. }
        | AdminProjectImportRequest::Status { digest } => digest.clone(),
    };
    let loaded = invoke::<_, FleetKind, _>(
        application,
        caller.clone(),
        (),
        AdminImportWorkflow,
        AdminImportCommand::Read { database, digest },
    )
    .await?;
    let mut record = match loaded {
        Ok(record) => record,
        Err(AdminImportRefusal::NotFound)
            if matches!(request, AdminProjectImportRequest::Begin { .. }) =>
        {
            let AdminProjectImportRequest::Begin { header } = &request else {
                return Err(refused());
            };
            match invoke::<_, FleetKind, _>(
                application,
                caller.clone(),
                (),
                AdminImportWorkflow,
                AdminImportCommand::Reserve {
                    header: Box::new(header.clone()),
                    candidate,
                },
            )
            .await?
            {
                Ok(record) => record,
                Err(reason) => return Ok(AdminProjectImportAnswer::Refused(reason)),
            }
        }
        Err(reason) => return Ok(AdminProjectImportAnswer::Refused(reason)),
    };
    if matches!(request, AdminProjectImportRequest::Status { .. }) {
        return Ok(AdminProjectImportAnswer::Progress(record));
    }
    let import_id = match &request {
        AdminProjectImportRequest::Begin { .. } => record.import_id,
        AdminProjectImportRequest::Page { import_id, .. }
        | AdminProjectImportRequest::Complete { import_id, .. } => *import_id,
        AdminProjectImportRequest::Status { .. } => return Err(refused()),
    };
    if import_id != record.import_id {
        return Ok(AdminProjectImportAnswer::Refused(
            AdminImportRefusal::IdentityMismatch,
        ));
    }
    // A completed workflow still asks the library with a fresh attempt so a later
    // retirement takes precedence. A pending workflow resumes its original task.
    let attempt = if matches!(record.phase, AdminImportPhase::Reserved) {
        import_id
    } else {
        candidate
    };
    if let Some(answer) = reservation(application, caller.clone(), &record, attempt).await? {
        return Ok(answer);
    }
    if matches!(record.phase, AdminImportPhase::Complete { .. }) {
        return Ok(AdminProjectImportAnswer::AlreadyImported);
    }
    // Reconcile the receiver's retained seed-completion evidence before
    // comparing a changed physical header or attempting another publication.
    match invoke::<_, CopyKind, _>(
        application,
        caller.clone(),
        database,
        AdminCopySeed { database },
        AdminSeedRequest::ReconcilePublication(Box::new(record.clone())),
    )
    .await?
    {
        AdminSeedAnswer::Published { position } => {
            record.phase = AdminImportPhase::Published { position }
        }
        AdminSeedAnswer::Unpublished => {}
        AdminSeedAnswer::Retired => return Ok(AdminProjectImportAnswer::Retired),
        AdminSeedAnswer::Replication(_) => return Err(CallFailure::AnswerLost),
    }
    if let AdminProjectImportRequest::Begin { header } = &request {
        record = match invoke::<_, FleetKind, _>(
            application,
            caller.clone(),
            (),
            AdminImportWorkflow,
            AdminImportCommand::Reserve {
                header: Box::new(header.clone()),
                candidate,
            },
        )
        .await?
        {
            Ok(record) => record,
            Err(reason) => return Ok(AdminProjectImportAnswer::Refused(reason)),
        };
    }
    if matches!(record.phase, AdminImportPhase::Published { .. }) {
        return finish_import(application, caller, &record, candidate, true).await;
    }
    match request {
        AdminProjectImportRequest::Begin { .. } => {
            match invoke::<_, CopyKind, _>(
                application,
                caller,
                database,
                AdminCopySeed { database },
                AdminSeedRequest::Begin { import_id },
            )
            .await?
            {
                AdminSeedAnswer::Replication(ReplicationAnswer::SeedStarted { seed }) => {
                    Ok(AdminProjectImportAnswer::Ready { import_id, seed })
                }
                AdminSeedAnswer::Replication(answer) => copy_failure(answer),
                _ => Err(CallFailure::AnswerLost),
            }
        }
        AdminProjectImportRequest::Page {
            page,
            contents,
            page_digest,
            ..
        } => {
            if page >= record.header.pages
                || contents.database != database
                || contents.position != record.header.manifest.position
                || contents.schema != record.header.manifest.schema
            {
                return Ok(AdminProjectImportAnswer::Refused(
                    AdminImportRefusal::InvalidHeader,
                ));
            }
            match invoke::<_, CopyKind, _>(
                application,
                caller,
                database,
                AdminCopySeed { database },
                AdminSeedRequest::Page {
                    import_id,
                    page,
                    contents: Box::new(contents),
                    digest: page_digest,
                },
            )
            .await?
            {
                AdminSeedAnswer::Replication(ReplicationAnswer::SeedPage(
                    SeedPageAnswer::Staged | SeedPageAnswer::AlreadyStaged,
                )) => Ok(AdminProjectImportAnswer::Staged { import_id, page }),
                AdminSeedAnswer::Replication(ReplicationAnswer::SeedPage(answer)) => Ok(
                    AdminProjectImportAnswer::Refused(AdminImportRefusal::PageRefused(answer)),
                ),
                AdminSeedAnswer::Replication(answer) => copy_failure(answer),
                _ => Err(CallFailure::AnswerLost),
            }
        }
        AdminProjectImportRequest::Complete { .. } => {
            match invoke::<_, CopyKind, _>(
                application,
                caller.clone(),
                database,
                AdminCopySeed { database },
                AdminSeedRequest::Complete(Box::new(record.clone())),
            )
            .await?
            {
                AdminSeedAnswer::Published { position } => {
                    record.phase = AdminImportPhase::Published { position };
                    finish_import(application, caller, &record, candidate, false).await
                }
                AdminSeedAnswer::Replication(answer) => copy_failure(answer),
                _ => Err(CallFailure::AnswerLost),
            }
        }
        AdminProjectImportRequest::Status { .. } => Err(refused()),
    }
}

async fn reservation<Platform: CentralPlatform>(
    application: &CentralApplication<Platform>,
    caller: AccessCaller,
    record: &AdminImportRecord,
    attempt: RequestId,
) -> Result<Option<AdminProjectImportAnswer>, CallFailure> {
    Ok(
        match invoke::<_, RegistryKind, _>(
            application,
            caller,
            (),
            AdminImportReservation,
            Import {
                attempt,
                database: record.header.database(),
                digest: record.header.digest.clone(),
            },
        )
        .await?
        {
            ReservationAnswer::Pending => Some(AdminProjectImportAnswer::Pending),
            ReservationAnswer::Resolved(ImportAnswer::Retired) => {
                Some(AdminProjectImportAnswer::Retired)
            }
            ReservationAnswer::Resolved(ImportAnswer::ImportConflict { existing, offered }) => {
                Some(AdminProjectImportAnswer::ImportConflict { existing, offered })
            }
            ReservationAnswer::Resolved(ImportAnswer::Imported | ImportAnswer::AlreadyImported) => {
                None
            }
        },
    )
}

async fn finish_import<Platform: CentralPlatform>(
    application: &CentralApplication<Platform>,
    caller: AccessCaller,
    record: &AdminImportRecord,
    attempt: RequestId,
    replayed: bool,
) -> Result<AdminProjectImportAnswer, CallFailure> {
    // ImportTask owns Unheld and preserves an existing holder. A fresh library
    // attempt after publication checks that authority without a second writer.
    if let Some(answer) = reservation(application, caller.clone(), record, attempt).await? {
        return Ok(answer);
    }
    match invoke::<_, FleetKind, _>(
        application,
        caller,
        (),
        AdminImportWorkflow,
        AdminImportCommand::Complete {
            database: record.header.database(),
            digest: record.header.digest.clone(),
            import_id: record.import_id,
        },
    )
    .await?
    {
        Ok(_) if replayed => Ok(AdminProjectImportAnswer::AlreadyImported),
        Ok(_) => Ok(AdminProjectImportAnswer::Imported {
            head: record.header.manifest.position,
        }),
        Err(reason) => Ok(AdminProjectImportAnswer::Refused(reason)),
    }
}

fn copy_failure(answer: ReplicationAnswer) -> Result<AdminProjectImportAnswer, CallFailure> {
    Ok(match answer {
        ReplicationAnswer::SeedComplete(SeedAnswer::Incomplete { .. }) => {
            AdminProjectImportAnswer::Refused(AdminImportRefusal::NotPublished)
        }
        ReplicationAnswer::SeedComplete(SeedAnswer::Retired)
        | ReplicationAnswer::SeedPage(SeedPageAnswer::Retired) => AdminProjectImportAnswer::Retired,
        ReplicationAnswer::SeedComplete(answer) => {
            AdminProjectImportAnswer::Refused(AdminImportRefusal::SeedRefused(answer))
        }
        ReplicationAnswer::CopyUnavailable { reason } => {
            AdminProjectImportAnswer::CopyUnavailable { reason }
        }
        ReplicationAnswer::OutcomeUnknown { reason } => {
            AdminProjectImportAnswer::OutcomeUnknown { reason }
        }
        ReplicationAnswer::Refused(reason) => {
            AdminProjectImportAnswer::Refused(AdminImportRefusal::CopyRefused(reason))
        }
        _ => return Err(CallFailure::AnswerLost),
    })
}

async fn invoke<Platform: CentralPlatform, To: ObjectKind, Call: CallTarget>(
    application: &CentralApplication<Platform>,
    caller: AccessCaller,
    destination: To::Address,
    target: Call,
    request: Call::Request,
) -> Result<Call::Answer, CallFailure>
where
    EdgeKind: MayCall<To>,
{
    if !matches!(caller, AccessCaller::Owner { .. }) {
        return Err(refused());
    }
    let caller = CallerContext::<EdgeKind>::for_verified_edge(caller).map_err(|_| refused())?;
    let identity = CallId {
        operation: OperationId::new(application.platform.ids().new_id()).map_err(|_| refused())?,
        step: StepName::new("admin_import").map_err(|_| refused())?,
    };
    let call =
        encode_shared_call::<EdgeKind, To, Call>(&caller, destination, target, request, identity)?;
    let bytes = application.platform.transport_object(call).await?;
    serde_json::from_slice(&bytes).map_err(|_| CallFailure::AnswerLost)
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

#[cfg(test)]
mod integrity_refusal_tests {

    use super::*;
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    #[test]
    fn seed_integrity_refusal_retains_both_tables_in_the_admin_answer()
    -> Result<(), Box<dyn std::error::Error>> {
        let refusal = SeedAnswer::IntegrityViolation {
            table: fleet_sync_protocol::TableName::new("posts")?,
            references: fleet_sync_protocol::TableName::new("threads")?,
        };
        let answer = copy_failure(ReplicationAnswer::SeedComplete(refusal.clone()))
            .map_err(|error| format!("unexpected call failure: {error:?}"))?;
        assert_eq!(
            answer,
            AdminProjectImportAnswer::Refused(AdminImportRefusal::SeedRefused(refusal))
        );
        Ok(())
    }
}
