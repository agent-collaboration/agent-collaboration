//! Change records are written by the transaction that makes the change.
use crate::{fleet_row_codec, generated_organization as queries};
use durable_steps::{StoreError, Uuid, storage::TransactionScope};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrganizationChangeKind {
    OrganizationProvisioned,
    StampRenewed,
    WorkspaceCreated,
}

impl OrganizationChangeKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::OrganizationProvisioned => "organizationProvisioned",
            Self::StampRenewed => "stampRenewed",
            Self::WorkspaceCreated => "workspaceCreated",
        }
    }
    #[cfg_attr(
        not(any(test, feature = "conformance")),
        expect(dead_code, reason = "B2d's change-record lookups decode records")
    )]
    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "organizationProvisioned" => Ok(Self::OrganizationProvisioned),
            "stampRenewed" => Ok(Self::StampRenewed),
            "workspaceCreated" => Ok(Self::WorkspaceCreated),
            _ => Err(corrupt("kind", "unknown change kind")),
        }
    }
}

/// A sign-in-originated change carries no person or permission; none is invented.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OrganizationChangeOrigin {
    SignInProvisioning,
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
    type Error = StoreError;
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
impl ChangeRequestId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationChangeRecord {
    pub id: Uuid,
    pub request_id: ChangeRequestId,
    pub kind: OrganizationChangeKind,
    pub origin: OrganizationChangeOrigin,
    pub target_id: String,
    pub created_at_millis: i64,
}

pub(crate) async fn insert<S: TransactionScope>(
    scope: &mut S,
    record: &OrganizationChangeRecord,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Encode<'v, S::Database>,
{
    fleet_row_codec::check_private_id(&record.id.to_string(), "organization_change_records.id")?;
    let (origin, person, permission) = match &record.origin {
        OrganizationChangeOrigin::SignInProvisioning => ("signInProvisioning", None, None),
        OrganizationChangeOrigin::Person {
            person_id,
            permission_checked,
        } => (
            "person",
            Some(person_id.clone()),
            Some(permission_checked.clone()),
        ),
    };
    let at = record.created_at_millis.to_string();
    queries::insert_organization_change_record(
        scope,
        record.id.to_string(),
        record.request_id.as_str().into(),
        record.kind.as_str().into(),
        origin.into(),
        person,
        permission,
        record.target_id.clone(),
        at.clone(),
        at,
    )
    .await
    .map_err(StoreError::backend)
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "B2d's change-record lookups read records back")
)]
pub(crate) async fn read_by_request<S: TransactionScope>(
    scope: &mut S,
    request: &ChangeRequestId,
) -> Result<Option<OrganizationChangeRecord>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let Some(row) = queries::read_organization_change_record(scope, request.as_str().into())
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    decode(StoredColumns {
        id: (row.id, row.id_class),
        request_id: (row.request_id, row.request_id_class),
        kind: (row.kind, row.kind_class),
        origin: (row.origin, row.origin_class),
        actor_person_id: (row.actor_person_id, row.actor_person_id_class),
        permission_checked: (row.permission_checked, row.permission_checked_class),
        target_id: (row.target_id, row.target_id_class),
        created_at: (row.created_at, row.created_at_class),
    })
    .map(Some)
}

/// A bounded page of records in identity order, after the given record.
#[cfg_attr(
    not(any(test, feature = "conformance")),
    expect(dead_code, reason = "B2d's change-record lookups read records back")
)]
pub(crate) async fn read_page<S: TransactionScope>(
    scope: &mut S,
    after: Option<Uuid>,
    limit: u16,
) -> Result<Vec<OrganizationChangeRecord>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    // The empty string sorts before every canonical UUID text.
    let after = after.map(|id| id.to_string()).unwrap_or_default();
    queries::read_organization_change_records(scope, after, limit.to_string())
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            decode(StoredColumns {
                id: (row.id, row.id_class),
                request_id: (row.request_id, row.request_id_class),
                kind: (row.kind, row.kind_class),
                origin: (row.origin, row.origin_class),
                actor_person_id: (row.actor_person_id, row.actor_person_id_class),
                permission_checked: (row.permission_checked, row.permission_checked_class),
                target_id: (row.target_id, row.target_id_class),
                created_at: (row.created_at, row.created_at_class),
            })
        })
        .collect()
}

#[cfg_attr(
    not(any(test, feature = "conformance")),
    expect(dead_code, reason = "B2d's change-record lookups read records back")
)]
type Column = (Option<String>, Option<String>);
#[cfg_attr(
    not(any(test, feature = "conformance")),
    expect(dead_code, reason = "B2d's change-record lookups read records back")
)]
struct StoredColumns {
    id: Column,
    request_id: Column,
    kind: Column,
    origin: Column,
    actor_person_id: Column,
    permission_checked: Column,
    target_id: Column,
    created_at: Column,
}

#[cfg_attr(
    not(any(test, feature = "conformance")),
    expect(dead_code, reason = "B2d's change-record lookups read records back")
)]
fn decode(columns: StoredColumns) -> Result<OrganizationChangeRecord, StoreError> {
    let id = text(columns.id, "id")?;
    fleet_row_codec::check_private_id(&id, "organization_change_records.id")?;
    let id = Uuid::parse_str(&id).map_err(|error| corrupt("id", error))?;
    let request_id = ChangeRequestId::try_from(text(columns.request_id, "request_id")?)?;
    let kind = OrganizationChangeKind::parse(&text(columns.kind, "kind")?)?;
    let person = optional_text(columns.actor_person_id, "actor_person_id")?;
    let permission = optional_text(columns.permission_checked, "permission_checked")?;
    let origin = match (text(columns.origin, "origin")?.as_str(), person, permission) {
        ("signInProvisioning", None, None) => OrganizationChangeOrigin::SignInProvisioning,
        ("person", Some(person_id), Some(permission_checked))
            if !person_id.is_empty() && !permission_checked.is_empty() =>
        {
            OrganizationChangeOrigin::Person {
                person_id,
                permission_checked,
            }
        }
        _ => return Err(corrupt("origin", "origin does not match its actor columns")),
    };
    let target_id = text(columns.target_id, "target_id")?;
    if target_id.is_empty() {
        return Err(corrupt("target_id", "target is empty"));
    }
    let (created_at, class) = columns.created_at;
    fleet_row_codec::check_millis(
        created_at.clone(),
        class,
        "organization_change_records.created_at",
    )?;
    let created_at_millis = fleet_row_codec::required(created_at, "created_at")?
        .parse()
        .map_err(|error| corrupt("created_at", error))?;
    Ok(OrganizationChangeRecord {
        id,
        request_id,
        kind,
        origin,
        target_id,
        created_at_millis,
    })
}

#[cfg_attr(
    not(any(test, feature = "conformance")),
    expect(dead_code, reason = "B2d's change-record lookups read records back")
)]
fn text((value, class): Column, field: &str) -> Result<String, StoreError> {
    if fleet_row_codec::required(class, field)? != "text" {
        return Err(corrupt(field, "column is not TEXT"));
    }
    fleet_row_codec::required(value, field)
}

#[cfg_attr(
    not(any(test, feature = "conformance")),
    expect(dead_code, reason = "B2d's change-record lookups read records back")
)]
fn optional_text((value, class): Column, field: &str) -> Result<Option<String>, StoreError> {
    match fleet_row_codec::required(class, field)?.as_str() {
        "null" => Ok(None),
        "text" => fleet_row_codec::required(value, field).map(Some),
        _ => Err(corrupt(field, "column is neither TEXT nor NULL")),
    }
}

fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("organization_change_records.{field}"),
        message: message.to_string(),
    }
}
