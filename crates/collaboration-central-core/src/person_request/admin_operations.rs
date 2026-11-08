//! The closed admin operations a person can ask central for, and the signed
//! intent each one hashes (T06: `{v:1, operation, organization, target,
//! operationId, params}`, RFC 8785 JCS, SHA-256, base64url).
use super::compact_jwt::sha256_url;
use super::refusal::PersonRequestRefusal;
use fleet_identity_protocol::OrganizationId;
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
impl AdminOperationId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub const MAXIMUM_NAME_BYTES: usize = 256;
pub const MAXIMUM_DESCRIPTION_BYTES: usize = 2048;
pub const MAXIMUM_RECORD_PAGE: u16 = 100;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkspaceRequest {
    pub organization_id: OrganizationId,
    pub operation_id: AdminOperationId,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceLookupRequest {
    pub organization_id: OrganizationId,
    pub workspace_id: message_board::WorkspaceId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadChangeRecordsRequest {
    pub organization_id: OrganizationId,
    #[serde(default)]
    pub after: Option<durable_steps::Uuid>,
    pub limit: u16,
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
                !request.name.trim().is_empty()
                    && request.name.len() <= MAXIMUM_NAME_BYTES
                    && request.description.len() <= MAXIMUM_DESCRIPTION_BYTES
            }
            Self::WorkspaceLookup(_) => true,
            Self::ReadChangeRecords(request) => (1..=MAXIMUM_RECORD_PAGE).contains(&request.limit),
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
                json!({"name":request.name,"description":request.description}),
            ),
            Self::WorkspaceLookup(request) => (json!(request.workspace_id.as_str()), json!({})),
            Self::ReadChangeRecords(request) => (
                json!(organization),
                json!({"after":request.after.map(|id| id.to_string()),"limit":request.limit}),
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

    /// base64url(SHA-256(JCS(intent))), the proof's `intent` claim.
    pub fn intent_digest(&self) -> Result<String, PersonRequestRefusal> {
        Ok(sha256_url(canonical_json(&self.intent())?.as_bytes()))
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
