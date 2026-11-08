//! The Fleet object's adapter for a person's admin request. It checks the
//! call's envelope, hands the admin decision the narrow view it needs
//! ([`OrganizationAdminFleet`]) and runs the one applying transaction:
//! admission against the floors, the Fleet decision and the change record,
//! on the clock sampled inside it. The decision lives in
//! `organization_admin_calls`.
use crate::{
    CallerKind, FleetKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    fleet_metadata_step::DecideFleet,
    fleet_organization_authority::{FleetOrganizationAuthority, StampStanding},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    organization_admin_calls::{
        self as admin_calls, AdminRefusal, FleetOrganizationAdmin, OrganizationAdminFleet,
        OrganizationAdminResult, WorkspaceView, admin_record_origin, refused,
    },
    organization_authority::{
        ChangeRequestId, OrganizationChangeKind, OrganizationChangeOrigin,
        OrganizationChangeRecord, change_records,
    },
    person_request::{
        AdminOperation, CreateWorkspaceRequest, PersonRequestInput, VerifiedPersonRequest,
        admit_person_request,
    },
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallFailure, StoreError, storage::TransactionScope};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_identity_protocol::OrganizationId;
use message_board::{
    Description, HumanId, ProtocolVersion, Record, RecordKind, RecordedActor, RequestDecision,
    RequestId, WorkspaceId,
};
use runtime_primitives::Clock;
use serde::Serialize;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{convert::Infallible, sync::Arc};

/// The parts of a Fleet object the admin decision acts through.
struct FleetAdmin<'fleet, Host: ObjectHost, Io: PlatformIo> {
    runtime: &'fleet ObjectRuntime<FleetKind, Host, Io>,
    organization: &'fleet OrganizationId,
    authority: &'fleet FleetOrganizationAuthority,
    host: &'fleet Arc<Host>,
    io: &'fleet Arc<Io>,
}

impl<Host: ObjectHost, Io: PlatformIo> OrganizationAdminFleet for FleetAdmin<'_, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Decode<'value, Host::Database> + Encode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    fn organization(&self) -> &OrganizationId {
        self.organization
    }
    fn stamp_authority(&self) -> &FleetOrganizationAuthority {
        self.authority
    }
    async fn stamp_standing(&self) -> Result<StampStanding, CallFailure> {
        self.runtime.stamp_standing().await
    }
    async fn apply(
        &self,
        request: VerifiedPersonRequest,
        sign_in_service: message_board::ServiceId,
    ) -> Result<OrganizationAdminResult, CallFailure> {
        let (answer, _) = StepDriver::new(self.host.clone())
            .run(ApplyAdmin {
                request,
                sign_in_service,
                clock: self.io.clone(),
                row_ids: crate::fleet_row_ids::FleetRowIds::new(self.io.clone()),
                record_id: self.io.new_id(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        Ok(answer)
    }
    #[cfg(feature = "conformance")]
    async fn before_apply(&self, kind: &'static str) -> Result<(), CallFailure> {
        self.io.before_organization_apply(kind).await
    }
    #[cfg(feature = "conformance")]
    fn take_answer_drop(&self) -> bool {
        self.io.take_organization_answer_drop()
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Decode<'value, Host::Database> + Encode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn organization_admin_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source_face != CallerKind::Edge
            || !matches!(call.authority, CallAuthority::SignInLink)
            || !call.has_fleet_source()
        {
            return Err(refused());
        }
        let _: FleetOrganizationAdmin =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let input: PersonRequestInput =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet {
            organization,
            organization_stamp,
            host,
            io,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        let fleet = FleetAdmin {
            runtime: self,
            organization,
            authority: organization_stamp,
            host,
            io,
        };
        encode(admin_calls::admin(&fleet, input).await?)
    }
}

struct ApplyAdmin<C> {
    request: VerifiedPersonRequest,
    sign_in_service: message_board::ServiceId,
    clock: Arc<C>,
    row_ids: crate::fleet_row_ids::FleetRowIds,
    record_id: durable_steps::Uuid,
}

impl<Store, Driver, C> StepCallback<Store, Driver> for ApplyAdmin<C>
where
    Store: AtomicStore,
    Driver: Database,
    C: Clock + Send + Sync + 'static,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = OrganizationAdminResult;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            // The authorization clock is sampled here, inside the applying
            // transaction, immediately before every bound is rechecked.
            let now_ms = self.clock.now().unix_millis();
            if let Err(refusal) = admit_person_request(scope, &self.request, now_ms).await? {
                return Ok((
                    OrganizationAdminResult::Refused {
                        reason: refusal.into(),
                    },
                    Vec::new(),
                ));
            }
            let answer = match &self.request.operation {
                AdminOperation::CreateWorkspace(create) => {
                    create_workspace::<Store, Driver>(
                        scope,
                        &self.request,
                        create,
                        self.sign_in_service,
                        now_ms,
                        self.row_ids,
                        self.record_id,
                    )
                    .await?
                }
                AdminOperation::WorkspaceLookup(lookup) => OrganizationAdminResult::Found {
                    workspace: read_workspace(scope, &lookup.workspace_id).await?,
                },
                AdminOperation::ReadChangeRecords(page) => OrganizationAdminResult::Records {
                    records: change_records::read_page(scope, page.after, page.limit.get()).await?,
                },
            };
            Ok((answer, Vec::new()))
        })
    }
}

async fn create_workspace<Store, Driver>(
    scope: &mut Store::Scope<'_>,
    request: &VerifiedPersonRequest,
    create: &CreateWorkspaceRequest,
    sign_in_service: message_board::ServiceId,
    now_ms: i64,
    row_ids: crate::fleet_row_ids::FleetRowIds,
    record_id: durable_steps::Uuid,
) -> Result<OrganizationAdminResult, StoreError>
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    fn invalid(error: impl std::fmt::Display) -> StoreError {
        StoreError::backend(format!(
            "admitted admin request has invalid fields: {error}"
        ))
    }
    // The workspace's ID is its operation's ID, so an exact retry hashes to the
    // same ledger request; the fresh request ID never enters the ledger.
    let operation = create.operation_id.as_str().to_owned();
    let workspace_id = WorkspaceId::try_from(operation.clone()).map_err(invalid)?;
    let fleet_request = FleetRequest {
        request_id: RequestId::try_from(operation.clone()).map_err(invalid)?,
        workspace: workspace_id.clone(),
        actor: RecordedActor::Human {
            human_id: HumanId::try_from(request.person_id.clone()).map_err(invalid)?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: create.name.resource_name().clone(),
            description: Description::try_from(create.description.clone()).map_err(invalid)?,
        },
    };
    let origin = admin_record_origin(sign_in_service)?;
    let now = chrono::DateTime::from_timestamp_millis(now_ms)
        .ok_or_else(|| StoreError::backend("invalid admin clock"))?;
    let (decided, _) = StepCallback::<Store, Driver>::run(
        DecideFleet {
            request: fleet_request,
            origin,
            now,
            row_ids,
        },
        scope,
    )
    .await?;
    let existing = match decided {
        FleetRequestResult::ConflictingRequest => {
            return Ok(OrganizationAdminResult::Refused {
                reason: AdminRefusal::Conflict,
            });
        }
        FleetRequestResult::CallerNotEnrolled => {
            return Ok(OrganizationAdminResult::Refused {
                reason: AdminRefusal::Rejected,
            });
        }
        FleetRequestResult::Decided(RequestDecision::Accepted { .. }) => workspace_id.clone(),
        FleetRequestResult::Decided(RequestDecision::Rejected { reasons }) => {
            // Two operations for one canonical name end with one workspace.
            match reasons.iter().find_map(|reason| match reason {
                message_board::Refusal::NameConflict { existing, .. }
                    if existing.kind == RecordKind::Workspace =>
                {
                    Some(existing.id.clone())
                }
                _ => None,
            }) {
                Some(id) => WorkspaceId::try_from(String::from(id)).map_err(invalid)?,
                None => {
                    return Ok(OrganizationAdminResult::Refused {
                        reason: AdminRefusal::Rejected,
                    });
                }
            }
        }
    };
    let workspace = read_workspace(scope, &existing)
        .await?
        .ok_or_else(|| StoreError::backend("decided workspace is missing"))?;
    let request_id = ChangeRequestId::try_from(operation)?;
    let created_here = existing == workspace_id
        && change_records::read_by_request(scope, &request_id)
            .await?
            .is_none();
    if created_here {
        change_records::insert(
            scope,
            &OrganizationChangeRecord {
                id: record_id,
                request_id,
                kind: OrganizationChangeKind::WorkspaceCreated,
                origin: OrganizationChangeOrigin::Person {
                    person_id: request.person_id.clone(),
                    permission_checked: request.operation.route().permission().to_owned(),
                },
                target_id: workspace_id.as_str().to_owned(),
                created_at_millis: now_ms,
            },
        )
        .await?;
    }
    Ok(OrganizationAdminResult::Applied {
        workspace,
        already_existed: !created_here,
    })
}

async fn read_workspace<S: TransactionScope>(
    scope: &mut S,
    workspace: &WorkspaceId,
) -> Result<Option<WorkspaceView>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let Some(decoded) = crate::fleet_workspace_store::read(scope, workspace.as_str()).await? else {
        return Ok(None);
    };
    let Record::Workspace(workspace) = decoded.record else {
        return Err(StoreError::backend(
            "workspace row decoded as another record",
        ));
    };
    Ok(Some(WorkspaceView {
        workspace_id: workspace.workspace_id,
        name: workspace.name,
        description: workspace.description,
        state: workspace.state,
    }))
}

fn encode<T: Serialize>(answer: T) -> Result<Vec<u8>, CallFailure> {
    serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
}
