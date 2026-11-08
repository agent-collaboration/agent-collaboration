//! The closed admin operations a person can ask central for, and the signed
//! intent each one hashes (T06: `{v:1, operation, organization, target,
//! operationId, params}`, RFC 8785 JCS, SHA-256, base64url).
use super::compact_jwt::sha256_url;
use super::refusal::PersonRequestRefusal;
use fleet_identity_protocol::OrganizationId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The stable operation identity, chosen once by the caller and kept on every
/// retry. It maps onto the Fleet request ledger's `RequestId`.
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

pub const MAXIMUM_DESCRIPTION_BYTES: usize = 2048;
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
    pub after: Option<durable_steps::Uuid>,
    pub limit: RecordPageLimit,
}

/// Which route a request arrived on decides how its body is read; the body
/// never chooses its own operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdminRoute {
    CreateWorkspace,
    WorkspaceLookup,
    ReadChangeRecords,
}

impl AdminRoute {
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::CreateWorkspace => "/organization-admin/v1/create-workspace",
            Self::WorkspaceLookup => "/organization-admin/v1/workspace-lookup",
            Self::ReadChangeRecords => "/organization-admin/v1/change-records",
        }
    }
    const fn operation(self) -> &'static str {
        match self {
            Self::CreateWorkspace => "CreateWorkspace",
            Self::WorkspaceLookup => "WorkspaceLookup",
            Self::ReadChangeRecords => "ReadChangeRecords",
        }
    }
    /// The permission the sign-in service's policy grants for this route.
    #[must_use]
    pub const fn permission(self) -> &'static str {
        match self {
            Self::CreateWorkspace => "workspace:create",
            Self::WorkspaceLookup => "organization:view",
            Self::ReadChangeRecords => "organization:readAudit",
        }
    }
    /// Changes need a fresh second factor; reads are exempt.
    #[must_use]
    pub const fn changes_state(self) -> bool {
        matches!(self, Self::CreateWorkspace)
    }
}

/// A parsed body together with its route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "route", content = "request", rename_all = "camelCase")]
pub enum AdminOperation {
    CreateWorkspace(CreateWorkspaceRequest),
    WorkspaceLookup(WorkspaceLookupRequest),
    ReadChangeRecords(ReadChangeRecordsRequest),
}

impl AdminOperation {
    pub fn parse(route: AdminRoute, body: &[u8]) -> Result<Self, PersonRequestRefusal> {
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
            Self::ReadChangeRecords(_) => true,
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
        }
    }

    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        match self {
            Self::CreateWorkspace(request) => &request.organization_id,
            Self::WorkspaceLookup(request) => &request.organization_id,
            Self::ReadChangeRecords(request) => &request.organization_id,
        }
    }

    #[must_use]
    pub fn operation_id(&self) -> Option<&AdminOperationId> {
        match self {
            Self::CreateWorkspace(request) => Some(&request.operation_id),
            Self::WorkspaceLookup(_) | Self::ReadChangeRecords(_) => None,
        }
    }

    /// The closed intent object; `params` holds every field that changes the effect.
    #[must_use]
    pub fn intent(&self) -> Value {
        let organization = self.organization_id().as_str();
        let (target, params) = match self {
            Self::CreateWorkspace(request) => (
                json!(organization),
                json!({"name":request.name.as_str(),"description":request.description}),
            ),
            Self::WorkspaceLookup(request) => (json!(request.workspace_id.as_str()), json!({})),
            Self::ReadChangeRecords(request) => (
                json!(organization),
                json!({"after":request.after.map(|id| id.to_string()),"limit":request.limit.get()}),
            ),
        };
        json!({
            "v": 1,
            "operation": self.route().operation(),
            "organization": organization,
            "target": target,
            "operationId": self.operation_id().map(AdminOperationId::as_str),
            "params": params,
        })
    }

    /// JCS(intent): the exact bytes the proof's `intent` claim hashes.
    pub fn canonical_intent(&self) -> Result<String, PersonRequestRefusal> {
        canonical_json(&self.intent())
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
