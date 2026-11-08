//! Conformance only: read an organization's whole B2 state back through the
//! Fleet object that owns it, on any host, in one transaction that writes
//! nothing: its stamp, change records, organization and person floors,
//! used-request markers, workspaces and request decisions. Every row is read;
//! nothing is capped.
use crate::generated_organization_conformance as queries;
use crate::organization_authority::{
    OrganizationChangeRecord, StoredStampRead, read_change_record_page, read_stored_stamp,
};
use crate::person_request::{OrganizationFloor, OrganizationFloorState, read_organization_floor};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_identity_protocol::OrganizationId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub const ORGANIZATION_RECORDS_PATH: &str = "/admin/v1/conformance/organization-records";
/// Conformance only: overwrite the stored statement in place, as damage on disk would.
pub const ORGANIZATION_STATEMENT_TAMPER_PATH: &str =
    "/admin/v1/conformance/organization-statement-tamper";
const RECORD_PAGE: u16 = 100;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrganizationStatementTamper {
    pub organization: OrganizationId,
    pub statement: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrganizationRecordsRequest {
    pub organization: OrganizationId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationRecordsObservation {
    /// The stored statement exactly as received, when the store is stamped.
    pub statement: Option<String>,
    pub stamped_organization: Option<OrganizationId>,
    /// Facts read from the stored statement's own bytes; not a verification.
    pub stamp: Option<ObservedStamp>,
    pub records: Vec<OrganizationChangeRecord>,
    /// The organization floor the same transaction retains, if any.
    pub organization_floor: Option<ObservedOrganizationFloor>,
    pub person_floors: Vec<ObservedPersonFloor>,
    pub used_requests: Vec<ObservedUsedRequest>,
    pub workspaces: Vec<ObservedWorkspace>,
    pub decisions: Vec<ObservedDecision>,
    /// Every row of every table above was read, with no cap.
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedStamp {
    /// Lowercase hex SHA-256 of the stored compact statement.
    pub digest: String,
    pub key_id: Option<String>,
    pub issued_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedOrganizationFloor {
    pub revision: i64,
    pub state: String,
    pub policy_digest: String,
}

impl From<OrganizationFloor> for ObservedOrganizationFloor {
    fn from(floor: OrganizationFloor) -> Self {
        let state = match floor.state {
            OrganizationFloorState::Active => "active",
            OrganizationFloorState::Stopped => "stopped",
            OrganizationFloorState::Deleted => "deleted",
        };
        Self {
            revision: floor.revision.get(),
            state: state.into(),
            policy_digest: floor.policy_digest,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedPersonFloor {
    pub subject_id: String,
    pub person_id: String,
    pub revision: i64,
    pub state: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedUsedRequest {
    pub id: String,
    pub operation_id: Option<String>,
    pub person_id: String,
    pub session_id: String,
    pub request_time_ms: i64,
    pub intent_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedWorkspace {
    pub workspace_id: String,
    pub name: String,
    pub state: String,
    pub request_id: Option<String>,
    pub origin_installation: String,
    pub origin_machine: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedDecision {
    pub request_id: String,
    pub workspace_id: String,
    pub decision_kind: String,
    pub change_kind: Option<String>,
    pub origin_installation: String,
    pub origin_machine: String,
}

fn required(value: Option<String>, field: &str) -> Result<String, StoreError> {
    value.ok_or_else(|| StoreError::Corrupt {
        field: format!("organization_records.{field}"),
        message: "missing value".into(),
    })
}

fn integer(value: Option<String>, field: &str) -> Result<i64, StoreError> {
    required(value, field)?
        .parse()
        .map_err(|error: std::num::ParseIntError| StoreError::Corrupt {
            field: format!("organization_records.{field}"),
            message: error.to_string(),
        })
}

/// The header kid and claim times as the stored bytes say, without verifying.
fn stamp_facts(statement: &str) -> ObservedStamp {
    let part = |index: usize| -> Option<serde_json::Value> {
        let encoded = statement.split('.').nth(index)?;
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).ok()?).ok()
    };
    let header = part(0);
    let claims = part(1);
    ObservedStamp {
        digest: format!("{:x}", Sha256::digest(statement.as_bytes())),
        key_id: header
            .as_ref()
            .and_then(|value| value.get("kid")?.as_str().map(str::to_owned)),
        issued_at: claims.as_ref().and_then(|value| value.get("iat")?.as_i64()),
        expires_at: claims.as_ref().and_then(|value| value.get("exp")?.as_i64()),
    }
}

impl OrganizationRecordsObservation {
    /// A Fleet with no store holds nothing; observing it creates no store.
    #[must_use]
    pub fn nothing_stored() -> Self {
        Self {
            statement: None,
            stamped_organization: None,
            stamp: None,
            records: Vec::new(),
            organization_floor: None,
            person_floors: Vec::new(),
            used_requests: Vec::new(),
            workspaces: Vec::new(),
            decisions: Vec::new(),
            complete: true,
        }
    }
}

/// One read-only pass over the owning Fleet store. It inserts no marker and
/// writes no row; the caller runs it inside one transaction.
pub(crate) async fn read_organization_records<S: TransactionScope>(
    scope: &mut S,
    organization: &OrganizationId,
) -> Result<OrganizationRecordsObservation, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let stamp = read_stored_stamp(scope).await?;
    let mut records: Vec<OrganizationChangeRecord> = Vec::new();
    loop {
        let after = records.last().map(|record| record.id);
        let page = read_change_record_page(scope, after, RECORD_PAGE).await?;
        let full = page.len() == usize::from(RECORD_PAGE);
        records.extend(page);
        if !full {
            break;
        }
    }
    let organization_floor = read_organization_floor(scope, organization.as_str())
        .await?
        .map(Into::into);
    let person_floors =
        queries::read_conformance_person_floors(scope, organization.as_str().into())
            .await
            .map_err(StoreError::backend)?
            .into_iter()
            .map(|row| {
                Ok(ObservedPersonFloor {
                    subject_id: required(row.subject_id, "subject_id")?,
                    person_id: required(row.person_id, "person_id")?,
                    revision: integer(row.minimum_revision, "minimum_revision")?,
                    state: required(row.state, "state")?,
                })
            })
            .collect::<Result<_, StoreError>>()?;
    let used_requests =
        queries::read_conformance_used_requests(scope, organization.as_str().into())
            .await
            .map_err(StoreError::backend)?
            .into_iter()
            .map(|row| {
                Ok(ObservedUsedRequest {
                    id: required(row.id, "id")?,
                    operation_id: row.operation_id,
                    person_id: required(row.actor_person_id, "actor_person_id")?,
                    session_id: required(row.actor_session_id, "actor_session_id")?,
                    request_time_ms: integer(row.request_time_ms, "request_time_ms")?,
                    intent_digest: required(row.intent_digest, "intent_digest")?,
                })
            })
            .collect::<Result<_, StoreError>>()?;
    let workspaces = queries::read_conformance_workspaces(scope)
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            Ok(ObservedWorkspace {
                workspace_id: required(row.workspace_id, "workspace_id")?,
                name: required(row.name, "name")?,
                state: required(row.state, "state")?,
                request_id: row.request_id,
                origin_installation: required(row.origin_installation, "origin_installation")?,
                origin_machine: required(row.origin_machine, "origin_machine")?,
            })
        })
        .collect::<Result<_, StoreError>>()?;
    let decisions = queries::read_conformance_decisions(scope)
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            Ok(ObservedDecision {
                request_id: required(row.request_id, "request_id")?,
                workspace_id: required(row.workspace_id, "workspace_id")?,
                decision_kind: required(row.decision_kind, "decision_kind")?,
                change_kind: row.change_kind,
                origin_installation: required(row.origin_installation, "origin_installation")?,
                origin_machine: required(row.origin_machine, "origin_machine")?,
            })
        })
        .collect::<Result<_, StoreError>>()?;
    let (statement, stamped_organization, stamp_observed) = match stamp {
        StoredStampRead::Unprovisioned => (None, None, None),
        StoredStampRead::Stamped(stored) => (
            Some(stored.statement().to_owned()),
            Some(stored.organization_id().clone()),
            Some(stamp_facts(stored.statement())),
        ),
    };
    Ok(OrganizationRecordsObservation {
        statement,
        stamped_organization,
        stamp: stamp_observed,
        records,
        organization_floor,
        person_floors,
        used_requests,
        workspaces,
        decisions,
        complete: true,
    })
}

#[cfg(test)]
#[path = "organization_records_observation_test.rs"]
mod tests;
