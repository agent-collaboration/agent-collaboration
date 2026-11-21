//! The closed admin operations a person can ask central for, and the signed
//! intent each one hashes (T06: `{v:1, operation, organization, target,
//! operationId, params}`, RFC 8785 JCS, SHA-256, base64url).
use super::compact_jwt::sha256_url;
use super::refusal::PersonRequestRefusal;
use fleet_contracts::{
    MAXIMUM_DESCRIPTION_BYTES,
    workspace_description::{description_schema, deserialize_description},
};
use fleet_identity_protocol::{EnrollmentId, OrganizationId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The stable operation identity, chosen once by the caller and kept on every
/// retry. Central's request log retains its committed outcome.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AdminOperationId(String);
impl TryFrom<String> for AdminOperationId {
    type Error = PersonRequestRefusal;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        super::compact_jwt::uuid_v7(&value, PersonRequestRefusal::OperationBody)?;
        Ok(Self(value))
    }
}
impl From<AdminOperationId> for String {
    fn from(value: AdminOperationId) -> Self {
        value.0
    }
}
impl JsonSchema for AdminOperationId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AdminOperationId".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string",
            "pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"})
    }
}
impl AdminOperationId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub const MAXIMUM_RECORD_PAGE: u16 = 100;

/// A workspace name exactly as the Fleet stores it: the Fleet's own
/// `ResourceName` rules, but a name that would be trimmed is refused rather
/// than rewritten, so the signed intent hashes the stored name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WorkspaceName(message_board::ResourceName);
impl TryFrom<String> for WorkspaceName {
    type Error = PersonRequestRefusal;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let name = message_board::ResourceName::try_from(value.clone())
            .map_err(|_| PersonRequestRefusal::OperationBody)?;
        if name.as_str() != value {
            return Err(PersonRequestRefusal::OperationBody);
        }
        Ok(Self(name))
    }
}
impl From<WorkspaceName> for String {
    fn from(value: WorkspaceName) -> Self {
        value.0.into()
    }
}
impl WorkspaceName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    #[must_use]
    pub fn resource_name(&self) -> &message_board::ResourceName {
        &self.0
    }
}
impl JsonSchema for WorkspaceName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "WorkspaceName".into()
    }
    // ResourceName's schema already admits only the trimmed form.
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <message_board::ResourceName as JsonSchema>::json_schema(generator)
    }
}

/// One page of change records: 1..=100.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct RecordPageLimit(u16);
impl TryFrom<u16> for RecordPageLimit {
    type Error = PersonRequestRefusal;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if !(1..=MAXIMUM_RECORD_PAGE).contains(&value) {
            return Err(PersonRequestRefusal::OperationBody);
        }
        Ok(Self(value))
    }
}
impl From<RecordPageLimit> for u16 {
    fn from(value: RecordPageLimit) -> Self {
        value.0
    }
}
impl RecordPageLimit {
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}
impl JsonSchema for RecordPageLimit {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RecordPageLimit".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer","minimum":1,"maximum":MAXIMUM_RECORD_PAGE})
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkspaceRequest {
    pub organization_id: OrganizationId,
    pub operation_id: AdminOperationId,
    pub name: WorkspaceName,
    #[serde(deserialize_with = "deserialize_description")]
    #[schemars(schema_with = "description_schema")]
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceLookupRequest {
    pub organization_id: OrganizationId,
    pub workspace_id: message_board::WorkspaceId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadChangeRecordsRequest {
    pub organization_id: OrganizationId,
    /// The last record ID of the previous page; absent or null for the first.
    #[serde(default)]
    #[schemars(with = "Option<String>", extend("format" = "uuid"))]
    pub after: Option<uuid::Uuid>,
    pub limit: RecordPageLimit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineHealthRequest {
    pub organization_id: OrganizationId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineListRetiredRequest {
    pub organization_id: OrganizationId,
    /// The last attempt ID of the previous page; absent or null for the first.
    #[serde(default)]
    #[schemars(with = "Option<String>", extend("format" = "uuid"))]
    pub after: Option<uuid::Uuid>,
    pub limit: RecordPageLimit,
}

/// Retry and discard name one failure record; the operation ID makes a replay
/// return the stored answer, as for CreateWorkspace.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineAttemptRequest {
    pub organization_id: OrganizationId,
    pub operation_id: AdminOperationId,
    #[schemars(with = "String", extend("format" = "uuid"))]
    pub attempt_id: uuid::Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeMachineEnrollmentRequest {
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
}

/// Revocation has only two body fields. The verified bearer supplies the
/// person and factor time used by the proof's intent projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuthorizedMachineRevocation {
    pub request: RevokeMachineEnrollmentRequest,
    authorization: MachineRevocationParams,
}

/// Which route a request arrived on decides how its body is read; the body
/// never chooses its own operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdminRoute {
    CreateWorkspace,
    WorkspaceLookup,
    ReadChangeRecords,
    EngineHealth,
    EngineListRetired,
    EngineRetry,
    EngineDiscard,
    RevokeMachineEnrollment,
}

impl AdminRoute {
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::CreateWorkspace => "/organization-admin/v1/create-workspace",
            Self::WorkspaceLookup => "/organization-admin/v1/workspace-lookup",
            Self::ReadChangeRecords => "/organization-admin/v1/change-records",
            Self::EngineHealth => "/organization-admin/v1/engine-health",
            Self::EngineListRetired => "/organization-admin/v1/engine-retired",
            Self::EngineRetry => "/organization-admin/v1/engine-retry",
            Self::EngineDiscard => "/organization-admin/v1/engine-discard",
            Self::RevokeMachineEnrollment => "/organization-admin/v1/revoke-machine-enrollment",
        }
    }
    /// The permission the sign-in service's policy grants for this route.
    #[must_use]
    pub const fn permission(self) -> &'static str {
        match self {
            Self::CreateWorkspace => "workspace:create",
            Self::WorkspaceLookup => "organization:view",
            Self::ReadChangeRecords => "organization:readAudit",
            Self::EngineHealth | Self::EngineListRetired => "organization:readAudit",
            // Mapped by the sign-in service's policy (Fleet access Lead); until it
            // is, admission refuses these two routes with NotPermitted.
            Self::EngineRetry | Self::EngineDiscard => "organization:repairWork",
            Self::RevokeMachineEnrollment => {
                crate::identity_contract::RevokePermission::MachineRevoke.as_str()
            }
        }
    }
    /// Changes need a fresh second factor; reads are exempt.
    #[must_use]
    pub const fn changes_state(self) -> bool {
        matches!(
            self,
            Self::CreateWorkspace
                | Self::EngineRetry
                | Self::EngineDiscard
                | Self::RevokeMachineEnrollment
        )
    }
}

/// A parsed body together with its route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "route", content = "request", rename_all = "camelCase")]
pub enum AdminOperation {
    CreateWorkspace(CreateWorkspaceRequest),
    WorkspaceLookup(WorkspaceLookupRequest),
    ReadChangeRecords(ReadChangeRecordsRequest),
    EngineHealth(EngineHealthRequest),
    EngineListRetired(EngineListRetiredRequest),
    EngineRetry(EngineAttemptRequest),
    EngineDiscard(EngineAttemptRequest),
    RevokeMachineEnrollment(AuthorizedMachineRevocation),
}

/// The version-one T06 object whose canonical bytes a person's proof hashes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct OrganizationAdminIntent(OrganizationAdminIntentWire);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "operation",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum OrganizationAdminIntentWire {
    CreateWorkspace {
        v: IntentVersionOne,
        organization: OrganizationId,
        target: OrganizationId,
        operation_id: AdminOperationId,
        params: CreateWorkspaceParams,
    },
    WorkspaceLookup {
        v: IntentVersionOne,
        organization: OrganizationId,
        target: message_board::WorkspaceId,
        operation_id: (),
        params: WorkspaceLookupParams,
    },
    ReadChangeRecords {
        v: IntentVersionOne,
        organization: OrganizationId,
        target: OrganizationId,
        operation_id: (),
        params: ReadChangeRecordsParams,
    },
    EngineHealth {
        v: IntentVersionOne,
        organization: OrganizationId,
        target: OrganizationId,
        operation_id: (),
        params: WorkspaceLookupParams,
    },
    EngineListRetired {
        v: IntentVersionOne,
        organization: OrganizationId,
        target: OrganizationId,
        operation_id: (),
        params: ReadChangeRecordsParams,
    },
    EngineRetry {
        v: IntentVersionOne,
        organization: OrganizationId,
        target: OrganizationId,
        operation_id: AdminOperationId,
        params: EngineAttemptParams,
    },
    EngineDiscard {
        v: IntentVersionOne,
        organization: OrganizationId,
        target: OrganizationId,
        operation_id: AdminOperationId,
        params: EngineAttemptParams,
    },
    RevokeMachineEnrollment {
        v: IntentVersionOne,
        organization: OrganizationId,
        target: EnrollmentId,
        operation_id: (),
        params: MachineRevocationParams,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
struct IntentVersionOne;

impl TryFrom<u8> for IntentVersionOne {
    type Error = PersonRequestRefusal;
    fn try_from(version: u8) -> Result<Self, Self::Error> {
        if version == 1 {
            Ok(Self)
        } else {
            Err(PersonRequestRefusal::OperationBody)
        }
    }
}
impl From<IntentVersionOne> for u8 {
    fn from(_: IntentVersionOne) -> Self {
        1
    }
}
impl JsonSchema for IntentVersionOne {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "IntentVersionOne".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer","const":1})
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateWorkspaceParams {
    name: WorkspaceName,
    #[serde(deserialize_with = "deserialize_description")]
    #[schemars(schema_with = "description_schema")]
    description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkspaceLookupParams {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadChangeRecordsParams {
    #[schemars(with = "Option<String>", extend("format" = "uuid"))]
    after: Option<uuid::Uuid>,
    limit: RecordPageLimit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EngineAttemptParams {
    #[schemars(with = "String", extend("format" = "uuid"))]
    attempt_id: uuid::Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MachineRevocationParams {
    actor: String,
    second_factor_at: Option<i64>,
}

impl AdminOperation {
    pub fn parse(route: AdminRoute, body: &[u8]) -> Result<Self, PersonRequestRefusal> {
        Self::parse_authenticated(route, body, None)
    }

    pub(super) fn parse_for_person(
        route: AdminRoute,
        body: &[u8],
        actor: &str,
        second_factor_at: Option<i64>,
    ) -> Result<Self, PersonRequestRefusal> {
        Self::parse_authenticated(route, body, Some((actor, second_factor_at)))
    }

    fn parse_authenticated(
        route: AdminRoute,
        body: &[u8],
        person: Option<(&str, Option<i64>)>,
    ) -> Result<Self, PersonRequestRefusal> {
        if body.len() > 8192 {
            return Err(PersonRequestRefusal::OperationBody);
        }
        let refusal = |_| PersonRequestRefusal::OperationBody;
        let operation = match route {
            AdminRoute::CreateWorkspace => {
                Self::CreateWorkspace(serde_json::from_slice(body).map_err(refusal)?)
            }
            AdminRoute::WorkspaceLookup => {
                Self::WorkspaceLookup(serde_json::from_slice(body).map_err(refusal)?)
            }
            AdminRoute::ReadChangeRecords => {
                Self::ReadChangeRecords(serde_json::from_slice(body).map_err(refusal)?)
            }
            AdminRoute::EngineHealth => {
                Self::EngineHealth(serde_json::from_slice(body).map_err(refusal)?)
            }
            AdminRoute::EngineListRetired => {
                Self::EngineListRetired(serde_json::from_slice(body).map_err(refusal)?)
            }
            AdminRoute::EngineRetry => {
                Self::EngineRetry(serde_json::from_slice(body).map_err(refusal)?)
            }
            AdminRoute::EngineDiscard => {
                Self::EngineDiscard(serde_json::from_slice(body).map_err(refusal)?)
            }
            AdminRoute::RevokeMachineEnrollment => {
                let (actor, second_factor_at) = person.ok_or(PersonRequestRefusal::PersonClaims)?;
                let request: RevokeMachineEnrollmentRequest =
                    serde_json::from_slice(body).map_err(refusal)?;
                super::compact_jwt::uuid_v7(
                    request.enrollment_id.as_str(),
                    PersonRequestRefusal::OperationBody,
                )?;
                Self::RevokeMachineEnrollment(AuthorizedMachineRevocation {
                    request,
                    authorization: MachineRevocationParams {
                        actor: actor.to_owned(),
                        second_factor_at,
                    },
                })
            }
        };
        operation.validate()?;
        Ok(operation)
    }

    fn validate(&self) -> Result<(), PersonRequestRefusal> {
        let valid = match self {
            Self::CreateWorkspace(request) => {
                request.description.len() <= MAXIMUM_DESCRIPTION_BYTES
            }
            Self::WorkspaceLookup(_) => true,
            Self::ReadChangeRecords(_)
            | Self::EngineHealth(_)
            | Self::EngineListRetired(_)
            | Self::EngineRetry(_)
            | Self::EngineDiscard(_)
            | Self::RevokeMachineEnrollment(_) => true,
        };
        if valid {
            Ok(())
        } else {
            Err(PersonRequestRefusal::OperationBody)
        }
    }

    #[must_use]
    pub const fn route(&self) -> AdminRoute {
        match self {
            Self::CreateWorkspace(_) => AdminRoute::CreateWorkspace,
            Self::WorkspaceLookup(_) => AdminRoute::WorkspaceLookup,
            Self::ReadChangeRecords(_) => AdminRoute::ReadChangeRecords,
            Self::EngineHealth(_) => AdminRoute::EngineHealth,
            Self::EngineListRetired(_) => AdminRoute::EngineListRetired,
            Self::EngineRetry(_) => AdminRoute::EngineRetry,
            Self::EngineDiscard(_) => AdminRoute::EngineDiscard,
            Self::RevokeMachineEnrollment(_) => AdminRoute::RevokeMachineEnrollment,
        }
    }

    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        match self {
            Self::CreateWorkspace(request) => &request.organization_id,
            Self::WorkspaceLookup(request) => &request.organization_id,
            Self::ReadChangeRecords(request) => &request.organization_id,
            Self::EngineHealth(request) => &request.organization_id,
            Self::EngineListRetired(request) => &request.organization_id,
            Self::EngineRetry(request) | Self::EngineDiscard(request) => &request.organization_id,
            Self::RevokeMachineEnrollment(revocation) => &revocation.request.organization_id,
        }
    }

    #[must_use]
    pub fn operation_id(&self) -> Option<&AdminOperationId> {
        match self {
            Self::CreateWorkspace(request) => Some(&request.operation_id),
            Self::EngineRetry(request) | Self::EngineDiscard(request) => {
                Some(&request.operation_id)
            }
            Self::WorkspaceLookup(_)
            | Self::ReadChangeRecords(_)
            | Self::EngineHealth(_)
            | Self::EngineListRetired(_)
            | Self::RevokeMachineEnrollment(_) => None,
        }
    }

    /// The closed intent object; `params` holds every field that changes the effect.
    #[must_use]
    pub fn intent(&self) -> OrganizationAdminIntent {
        let organization = self.organization_id().clone();
        OrganizationAdminIntent(match self {
            Self::CreateWorkspace(request) => OrganizationAdminIntentWire::CreateWorkspace {
                v: IntentVersionOne,
                organization: organization.clone(),
                target: organization,
                operation_id: request.operation_id.clone(),
                params: CreateWorkspaceParams {
                    name: request.name.clone(),
                    description: request.description.clone(),
                },
            },
            Self::WorkspaceLookup(request) => OrganizationAdminIntentWire::WorkspaceLookup {
                v: IntentVersionOne,
                organization,
                target: request.workspace_id.clone(),
                operation_id: (),
                params: WorkspaceLookupParams {},
            },
            Self::ReadChangeRecords(request) => OrganizationAdminIntentWire::ReadChangeRecords {
                v: IntentVersionOne,
                organization: organization.clone(),
                target: organization,
                operation_id: (),
                params: ReadChangeRecordsParams {
                    after: request.after,
                    limit: request.limit,
                },
            },
            Self::EngineHealth(_) => OrganizationAdminIntentWire::EngineHealth {
                v: IntentVersionOne,
                organization: organization.clone(),
                target: organization,
                operation_id: (),
                params: WorkspaceLookupParams {},
            },
            Self::EngineListRetired(request) => OrganizationAdminIntentWire::EngineListRetired {
                v: IntentVersionOne,
                organization: organization.clone(),
                target: organization,
                operation_id: (),
                params: ReadChangeRecordsParams {
                    after: request.after,
                    limit: request.limit,
                },
            },
            Self::EngineRetry(request) => OrganizationAdminIntentWire::EngineRetry {
                v: IntentVersionOne,
                organization: organization.clone(),
                target: organization,
                operation_id: request.operation_id.clone(),
                params: EngineAttemptParams {
                    attempt_id: request.attempt_id,
                },
            },
            Self::EngineDiscard(request) => OrganizationAdminIntentWire::EngineDiscard {
                v: IntentVersionOne,
                organization: organization.clone(),
                target: organization,
                operation_id: request.operation_id.clone(),
                params: EngineAttemptParams {
                    attempt_id: request.attempt_id,
                },
            },
            Self::RevokeMachineEnrollment(revocation) => {
                OrganizationAdminIntentWire::RevokeMachineEnrollment {
                    v: IntentVersionOne,
                    organization,
                    target: revocation.request.enrollment_id.clone(),
                    operation_id: (),
                    params: revocation.authorization.clone(),
                }
            }
        })
    }

    /// JCS(intent): the exact bytes the proof's `intent` claim hashes.
    pub fn canonical_intent(&self) -> Result<String, PersonRequestRefusal> {
        let intent =
            serde_json::to_value(self.intent()).map_err(|_| PersonRequestRefusal::OperationBody)?;
        canonical_json(&intent)
    }

    /// base64url(SHA-256(JCS(intent))), the proof's `intent` claim.
    pub fn intent_digest(&self) -> Result<String, PersonRequestRefusal> {
        Ok(sha256_url(self.canonical_intent()?.as_bytes()))
    }
}

/// RFC 8785 for the closed intent shapes: object members sorted by key
/// (all keys here are ASCII, so code-unit and byte order agree), no
/// insignificant whitespace, integers only, JSON string escaping.
pub(crate) fn canonical_json(value: &Value) -> Result<String, PersonRequestRefusal> {
    let mut out = String::new();
    write_canonical(value, &mut out)?;
    Ok(out)
}

fn write_canonical(value: &Value, out: &mut String) -> Result<(), PersonRequestRefusal> {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => {
            out.push_str(
                &serde_json::to_string(value).map_err(|_| PersonRequestRefusal::OperationBody)?,
            );
        }
        Value::Number(number) => {
            if !(number.is_i64() || number.is_u64()) {
                return Err(PersonRequestRefusal::OperationBody);
            }
            out.push_str(&number.to_string());
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out)?;
            }
            out.push(']');
        }
        Value::Object(members) => {
            let mut keys = members.keys().collect::<Vec<_>>();
            keys.sort_unstable_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(
                    &serde_json::to_string(key).map_err(|_| PersonRequestRefusal::OperationBody)?,
                );
                out.push(':');
                let member = members
                    .get(key)
                    .ok_or(PersonRequestRefusal::OperationBody)?;
                write_canonical(member, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}
