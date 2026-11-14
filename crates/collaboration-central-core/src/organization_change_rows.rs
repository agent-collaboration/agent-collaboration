//! Sign-in change records share the stamp/effect atomic batch.
use crate::{
    ObjectHost,
    workspace_decode_error::corrupt as bad_value,
    workspace_row_codec::{column, string},
};
use central_storage::{
    GuardedStatement, ReadOutcome, RowExpectation, SqlValue, Statement, StoreFailure,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OrganizationChangeKind {
    OrganizationProvisioned,
    StampRenewed,
    WorkspaceCreated,
}

impl OrganizationChangeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::OrganizationProvisioned => "organizationProvisioned",
            Self::StampRenewed => "stampRenewed",
            Self::WorkspaceCreated => "workspaceCreated",
        }
    }
    pub(crate) fn parse(value: &str) -> Result<Self, StoreFailure> {
        match value {
            "organizationProvisioned" => Ok(Self::OrganizationProvisioned),
            "stampRenewed" => Ok(Self::StampRenewed),
            "workspaceCreated" => Ok(Self::WorkspaceCreated),
            _ => Err(corrupt("kind", "unknown change kind")),
        }
    }
}

/// A sign-in-originated change carries no person or permission; none is invented.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationChangeOrigin {
    /// An empty struct variant so an unknown field is refused like any other.
    SignInProvisioning {},
    Person {
        person_id: String,
        permission_checked: String,
    },
}

/// The operation's own identity; a repeat of that operation finds this row.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ChangeRequestId(String);

impl TryFrom<String> for ChangeRequestId {
    type Error = StoreFailure;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > 128 || value.contains('\0') {
            return Err(corrupt(
                "request_id",
                "request identity is not bounded text",
            ));
        }
        Ok(Self(value))
    }
}
impl From<ChangeRequestId> for String {
    fn from(value: ChangeRequestId) -> Self {
        value.0
    }
}
impl JsonSchema for ChangeRequestId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ChangeRequestId".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string","minLength":1,"maxLength":128})
    }
}

impl ChangeRequestId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrganizationChangeRecord {
    #[schemars(with = "String", extend("format" = "uuid"))]
    pub id: Uuid,
    pub request_id: ChangeRequestId,
    pub kind: OrganizationChangeKind,
    pub origin: OrganizationChangeOrigin,
    pub target_id: String,
    pub created_at_millis: i64,
}

fn corrupt(field: &str, _message: impl std::fmt::Display) -> StoreFailure {
    bad_value(&format!("organization_change_records.{field}"))
}
fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
fn integer(value: i64) -> Result<SqlValue, StoreFailure> {
    value
        .try_into()
        .map(SqlValue::Integer)
        .map_err(|_| bad_value("organization_change_records.created_at"))
}
fn private_id(value: &str) -> Result<Uuid, StoreFailure> {
    let id = Uuid::parse_str(value).map_err(|_| bad_value("organization_change_records.id"))?;
    if !runtime_primitives::uuid_v7(id) || id.to_string() != value {
        return Err(bad_value("organization_change_records.id"));
    }
    Ok(id)
}
pub(crate) fn insert(record: &OrganizationChangeRecord) -> Result<GuardedStatement, StoreFailure> {
    private_id(&record.id.to_string())?;
    let (origin, person, permission) = match &record.origin {
        OrganizationChangeOrigin::SignInProvisioning {} => {
            ("signInProvisioning", SqlValue::Null, SqlValue::Null)
        }
        OrganizationChangeOrigin::Person {
            person_id,
            permission_checked,
        } => {
            if person_id.is_empty() || permission_checked.is_empty() {
                return Err(bad_value("organization_change_records.origin"));
            }
            ("person", text(person_id), text(permission_checked))
        }
    };
    if record.target_id.is_empty() {
        return Err(bad_value("organization_change_records.target_id"));
    }
    Ok(GuardedStatement {
        statement: Statement {
            sql: "INSERT INTO organization_change_records(id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?)",
            args: vec![
                text(record.id),
                text(record.request_id.as_str()),
                text(record.kind.as_str()),
                text(origin),
                person,
                permission,
                text(&record.target_id),
                integer(record.created_at_millis)?,
                integer(record.created_at_millis)?,
            ],
        },
        expect: RowExpectation::Exactly(1),
    })
}
fn optional_text(
    rows: &ReadOutcome,
    row: &[SqlValue],
    name: &str,
) -> Result<Option<String>, StoreFailure> {
    match column(rows, row, name)? {
        SqlValue::Null => Ok(None),
        SqlValue::Text(value) => Ok(Some(value.clone())),
        _ => Err(bad_value(name)),
    }
}
fn decode(rows: &ReadOutcome, row: &[SqlValue]) -> Result<OrganizationChangeRecord, StoreFailure> {
    let id = private_id(string(rows, row, "id")?)?;
    let request_id = string(rows, row, "request_id")?.to_owned().try_into()?;
    let kind = OrganizationChangeKind::parse(string(rows, row, "kind")?)?;
    let person = optional_text(rows, row, "actor_person_id")?;
    let permission = optional_text(rows, row, "permission_checked")?;
    let origin = match (string(rows, row, "origin")?, person, permission) {
        ("signInProvisioning", None, None) => OrganizationChangeOrigin::SignInProvisioning {},
        ("person", Some(person_id), Some(permission_checked))
            if !person_id.is_empty() && !permission_checked.is_empty() =>
        {
            OrganizationChangeOrigin::Person {
                person_id,
                permission_checked,
            }
        }
        _ => return Err(bad_value("organization_change_records.origin")),
    };
    let target_id = string(rows, row, "target_id")?.to_owned();
    if target_id.is_empty() {
        return Err(bad_value("organization_change_records.target_id"));
    }
    let SqlValue::Integer(created) = column(rows, row, "created_at")? else {
        return Err(bad_value("organization_change_records.created_at"));
    };
    Ok(OrganizationChangeRecord {
        id,
        request_id,
        kind,
        origin,
        target_id,
        created_at_millis: created.get(),
    })
}
pub(crate) async fn read_page(
    host: &impl ObjectHost,
    after: Option<Uuid>,
    limit: u16,
) -> Result<Vec<OrganizationChangeRecord>, StoreFailure> {
    let rows=host.read(Statement{sql:"SELECT id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at FROM organization_change_records WHERE id>? ORDER BY id LIMIT ?",args:vec![text(after.map(|id|id.to_string()).unwrap_or_default()),integer(i64::from(limit))?]}).await?;
    rows.rows.iter().map(|row| decode(&rows, row)).collect()
}
