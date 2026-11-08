//! A person's admin request at the organization's Fleet object: verify the
//! token and proof, then one transaction admits it against the floors, writes
//! the effect through the Fleet's own decision path and records the change.
use crate::{
    CallerKind, FleetKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    fleet_metadata_step::DecideFleet,
    fleet_organization_authority::StampStanding,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    organization_authority::{
        ChangeRequestId, OrganizationChangeKind, OrganizationChangeOrigin,
        OrganizationChangeRecord, change_records,
    },
    person_request::{
        AdminOperation, AdmissionRefusal, CreateWorkspaceRequest, PersonRequestInput,
        PersonRequestRefusal, VerifiedPersonRequest, admit_person_request,
    },
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, NotSentReason, StoreError,
    storage::TransactionScope,
};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use message_board::{
    ActorOrigin, Description, HumanId, ProtocolVersion, Record, RecordKind, RecordedActor,
    RequestDecision, RequestId, ResourceName, WorkspaceId, WorkspaceState,
};
use runtime_primitives::Clock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{convert::Infallible, sync::Arc};

#[derive(Serialize, Deserialize)]
pub struct FleetOrganizationAdmin;
impl CallTarget for FleetOrganizationAdmin {
    const KIND: &'static str = "central.organization-admin";
    type Request = PersonRequestInput;
    type Answer = OrganizationAdminAnswer;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceView {
    pub workspace_id: WorkspaceId,
    pub name: ResourceName,
    pub description: Description,
    pub state: WorkspaceState,
}

/// The Fleet object's answer to the link: a published result, or a
/// verification failure the link turns into a bare 401 that reveals nothing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrganizationAdminAnswer {
    Answered(OrganizationAdminResult),
    Unauthenticated,
}

/// The closed body of a 200 on an admin route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationAdminResult {
    /// `alreadyExisted` when this operation, or another for the same canonical
    /// name, had already created it; the caller sees one workspace either way.
    Applied {
        workspace: WorkspaceView,
        already_existed: bool,
    },
    Found {
        workspace: Option<WorkspaceView>,
    },
    Records {
        records: Vec<OrganizationChangeRecord>,
    },
    Refused {
        reason: AdminRefusal,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AdminRefusal {
    NotPermitted,
    SecondFactorRequired,
    RefreshRequired,
    AuthorizationSyncRequired,
    Expired,
    RequestTime,
    RequestReused,
    /// The same operation ID arrived with a different request.
    Conflict,
    /// The Fleet's own rules refused the change.
    Rejected,
}

impl From<AdmissionRefusal> for AdminRefusal {
    fn from(refusal: AdmissionRefusal) -> Self {
        match refusal {
            AdmissionRefusal::NotPermitted => Self::NotPermitted,
            AdmissionRefusal::SecondFactorRequired => Self::SecondFactorRequired,
            AdmissionRefusal::RefreshRequired => Self::RefreshRequired,
            AdmissionRefusal::AuthorizationSyncRequired => Self::AuthorizationSyncRequired,
            AdmissionRefusal::Expired => Self::Expired,
            AdmissionRefusal::RequestTime => Self::RequestTime,
            AdmissionRefusal::RequestReused => Self::RequestReused,
        }
    }
}

/// The one place a person's admin record origin is built: the installation
/// that wrote it is the sign-in service (its configured ServiceId, never an
/// organization ID); the record's actor is the person.
pub(crate) fn admin_record_origin(
    sign_in_service: message_board::ServiceId,
) -> Result<ActorOrigin, StoreError> {
    Ok(ActorOrigin {
        installation: sign_in_service,
        machine: "sign-in service"
            .to_owned()
            .try_into()
            .map_err(|error| StoreError::backend(format!("admin origin label: {error}")))?,
    })
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
        // An unstamped or untrusted Fleet answers nothing about its subjects.
        if self.stamp_standing().await? != StampStanding::Valid {
            return Err(refused());
        }
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
        let check = organization_stamp.check().ok_or_else(refused)?;
        // Without the sign-in service's configured identity there is no
        // honest origin to record; refuse rather than invent one.
        let sign_in_service = check.sign_in_service().ok_or(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })?;
        let verified = match check.verify_person_request(&input).await {
            Ok(verified) => verified,
            Err(PersonRequestRefusal::VerificationUnavailable | PersonRequestRefusal::Policy) => {
                return Err(CallFailure::NotSent {
                    reason: NotSentReason::Overloaded,
                });
            }
            Err(_) => return encode(OrganizationAdminAnswer::Unauthenticated),
        };
        // A token for another organization is refused before any read.
        if &verified.organization_id != organization {
            return encode(OrganizationAdminAnswer::Unauthenticated);
        }
        let (answer, _) = StepDriver::new(host.clone())
            .run(ApplyAdmin {
                request: verified,
                sign_in_service,
                clock: io.clone(),
                row_ids: crate::fleet_row_ids::FleetRowIds::new(io.clone()),
                record_id: io.new_id(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        // Conformance only: the change is committed; its answer is lost.
        #[cfg(feature = "conformance")]
        if io.take_organization_answer_drop() {
            return Err(CallFailure::AnswerLost);
        }
        encode(OrganizationAdminAnswer::Answered(answer))
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

fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
