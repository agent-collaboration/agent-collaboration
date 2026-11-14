//! Private fixture IPC: one read-only snapshot of the modern org-owned sign-in rows.
//! The raw compact statement is returned only to the fixture caller; never log it.
use crate::{
    ObjectHost,
    identity_contract::{FenceRevision, OrganizationFenceState, PersonFenceState},
    organization_change_rows::{ChangeRequestId, OrganizationChangeKind},
    person_request::AdminOperationId,
    workspace_decode_error::corrupt,
    workspace_row_codec::{json, string},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use central_storage::{ReadOutcome, SafeInteger, Statement, StoreFailure};
use fleet_identity_protocol::OrganizationId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use workspace_model::{PersonId, RequestId, ResourceName, Timestamp, WorkspaceId, WorkspaceState};

/// Canonical empty fixture shared with launcher apparatus under test-support.
pub const EMPTY_SIGNIN_OBSERVATION_JSON: &str = include_str!("signin_observation_empty.json");

/// The agreed minimal sign-in facts from the owning modern store.
/// Deliberately has no Debug: `statement` belongs only in the private IPC answer.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignInObservation {
    pub statement: Option<String>,
    pub stamp: Option<ObservedStamp>,
    pub records: Vec<ObservedChangeRecord>,
    pub organization_floor: Option<ObservedOrganizationFloor>,
    pub person_floors: Vec<ObservedPersonFloor>,
    pub used_requests: Vec<ObservedUsedRequest>,
    pub workspaces: Vec<ObservedWorkspace>,
    pub complete: bool,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedStamp {
    pub digest: String,
    pub key_id: Option<String>,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedOrganizationFloor {
    pub revision: FenceRevision,
    pub state: OrganizationFenceState,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedPersonFloor {
    pub subject_id: PersonId,
    pub person_id: PersonId,
    pub revision: FenceRevision,
    pub state: PersonFenceState,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedUsedRequest {
    pub id: RequestId,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedWorkspace {
    pub workspace_id: WorkspaceId,
    pub name: ResourceName,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedChangeRecord {
    pub kind: OrganizationChangeKind,
    pub request_id: ChangeRequestId,
    pub target_id: String,
    pub created_at_millis: i64,
}

impl SignInObservation {
    /// Used without touching storage when the host has never opened this org.
    #[must_use]
    pub fn nothing_stored() -> Self {
        Self {
            statement: None,
            stamp: None,
            records: vec![],
            organization_floor: None,
            person_floors: vec![],
            used_requests: vec![],
            workspaces: vec![],
            complete: true,
        }
    }
}

// One SQLite statement supplies one snapshot. Do not page, cap, or open another
// store here: every row in the already-addressed owning store must be observed.
// workerd intentionally caps SQLITE_LIMIT_COMPOUND_SELECT at five terms
// (cloudflare/workerd src/workerd/util/sqlite.c++, setupSecurity). Each CTE stays below it;
// the outer two-term SELECT preserves per-row results without a large JSON aggregate.
// CAST preserves generated JSON's TEXT type through Turso CTE metadata (UW-15).
const SNAPSHOT: &str = r#"
WITH identity_records_and_organization_floor AS (
SELECT 0 AS row_group,id AS row_id,json_object('kind','identity','row',json_object(
 'id',id,'statement',organization_statement,'created_at',created_at,'updated_at',updated_at)) AS payload
 FROM organization_identity
UNION ALL SELECT 1,id,json_object('kind','record','row',json_object(
 'id',id,'request_id',request_id,'kind',kind,'origin',origin,'actor_person_id',actor_person_id,
 'permission_checked',permission_checked,'target_id',target_id,'created_at',created_at,'updated_at',updated_at))
 FROM organization_change_records
UNION ALL SELECT 2,id,json_object('kind','organizationFloor','row',json_object(
 'id',id,'minimum_revision',minimum_revision,'state',state,'policy_digest',permission_policy_digest,
 'reject_through_ms',request_reject_through_ms,'created_at',created_at,'updated_at',updated_at))
 FROM organization_authorization_floors
), person_proofs_and_workspaces AS (
SELECT 3 AS row_group,person_id AS row_id,json_object('kind','personFloor','row',json_object(
 'id',id,'organization_id',organization_id,'person_id',person_id,'minimum_revision',minimum_revision,
 'state',state,'created_at',created_at,'updated_at',updated_at)) AS payload
 FROM person_organization_authorization_floors
UNION ALL SELECT 4,id,json_object('kind','usedRequest','row',json_object(
 'id',id,'organization_id',organization_id,'request_time_ms',request_time_ms,'operation_id',operation_id,
 'person_id',actor_person_id,'session_id',actor_session_id,'intent_digest',intent_digest,
 'proof_key_thumbprint',proof_key_thumbprint,'created_at',created_at,'updated_at',updated_at))
 FROM organization_used_requests
UNION ALL SELECT 5,workspace_id,json_object('kind','workspace','row',json_object(
 'workspaceId',workspace_id,'name',name,'state',state,'createdAt',created_at,'updatedAt',updated_at))
 FROM workspaces
)
SELECT row_group,row_id,CAST(payload AS TEXT) AS payload FROM identity_records_and_organization_floor
UNION ALL SELECT row_group,row_id,CAST(payload AS TEXT) AS payload FROM person_proofs_and_workspaces
ORDER BY row_group,row_id
"#;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "row",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum SnapshotRow {
    Identity(IdentityRow),
    Record(ChangeRow),
    OrganizationFloor(OrganizationFloorRow),
    PersonFloor(PersonFloorRow),
    UsedRequest(UsedRequestRow),
    Workspace(WorkspaceRow),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRow {
    id: OrganizationId,
    statement: String,
    created_at: SafeInteger,
    updated_at: SafeInteger,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum StoredChangeOrigin {
    SignInProvisioning,
    Person,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeRow {
    id: String,
    request_id: ChangeRequestId,
    kind: OrganizationChangeKind,
    origin: StoredChangeOrigin,
    actor_person_id: Option<String>,
    permission_checked: Option<String>,
    target_id: String,
    created_at: SafeInteger,
    updated_at: SafeInteger,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrganizationFloorRow {
    id: OrganizationId,
    minimum_revision: FenceRevision,
    state: OrganizationFenceState,
    policy_digest: String,
    reject_through_ms: SafeInteger,
    created_at: SafeInteger,
    updated_at: SafeInteger,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonFloorRow {
    id: PersonId,
    organization_id: OrganizationId,
    person_id: PersonId,
    minimum_revision: FenceRevision,
    state: PersonFenceState,
    created_at: SafeInteger,
    updated_at: SafeInteger,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UsedRequestRow {
    id: RequestId,
    organization_id: OrganizationId,
    request_time_ms: SafeInteger,
    operation_id: Option<AdminOperationId>,
    person_id: PersonId,
    session_id: String,
    intent_digest: String,
    proof_key_thumbprint: String,
    created_at: SafeInteger,
    updated_at: SafeInteger,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceRow {
    workspace_id: WorkspaceId,
    name: String,
    state: WorkspaceState,
    created_at: Timestamp,
    updated_at: Timestamp,
}

fn non_empty(value: &str, field: &str) -> Result<(), StoreFailure> {
    if value.is_empty() {
        return Err(corrupt(field));
    }
    Ok(())
}
fn canonical_v7(value: &str, field: &str) -> Result<Uuid, StoreFailure> {
    let id = Uuid::parse_str(value).map_err(|_| corrupt(field))?;
    if !runtime_primitives::uuid_v7(id) || id.to_string() != value {
        return Err(corrupt(field));
    }
    Ok(id)
}
fn check_times(created: SafeInteger, updated: SafeInteger) -> Result<(), StoreFailure> {
    for value in [created, updated] {
        Timestamp::from_timestamp_millis(value.get())
            .ok_or_else(|| corrupt("signin_observation.timestamp"))?;
    }
    Ok(())
}

/// Partial facts from stored bytes, deliberately not a signature/profile check.
fn stamp_facts(statement: &str) -> ObservedStamp {
    let part = |index| -> Option<serde_json::Value> {
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
        expires_at: claims.as_ref().and_then(|value| value.get("exp")?.as_i64()),
    }
}

fn decode_snapshot(
    rows: &ReadOutcome,
    organization: &OrganizationId,
) -> Result<SignInObservation, StoreFailure> {
    let mut observation = SignInObservation::nothing_stored();
    for row in &rows.rows {
        let row: SnapshotRow = json(string(rows, row, "payload")?, "signin_observation.snapshot")?;
        match row {
            SnapshotRow::Identity(row) => {
                if observation.statement.is_some() || row.id != *organization {
                    return Err(corrupt("organization_identity.id"));
                }
                non_empty(
                    &row.statement,
                    "organization_identity.organization_statement",
                )?;
                check_times(row.created_at, row.updated_at)?;
                observation.stamp = Some(stamp_facts(&row.statement));
                observation.statement = Some(row.statement);
            }
            SnapshotRow::Record(row) => {
                check_times(row.created_at, row.updated_at)?;
                match (row.origin, row.actor_person_id, row.permission_checked) {
                    (StoredChangeOrigin::SignInProvisioning, None, None) => {}
                    (StoredChangeOrigin::Person, Some(person_id), Some(permission_checked)) => {
                        non_empty(&person_id, "organization_change_records.actor_person_id")?;
                        non_empty(
                            &permission_checked,
                            "organization_change_records.permission_checked",
                        )?;
                    }
                    _ => return Err(corrupt("organization_change_records.origin")),
                };
                non_empty(&row.target_id, "organization_change_records.target_id")?;
                canonical_v7(&row.id, "organization_change_records.id")?;
                observation.records.push(ObservedChangeRecord {
                    request_id: row.request_id,
                    kind: row.kind,
                    target_id: row.target_id,
                    created_at_millis: row.created_at.get(),
                });
            }
            SnapshotRow::OrganizationFloor(row) => {
                if observation.organization_floor.is_some() || row.id != *organization {
                    return Err(corrupt("organization_authorization_floors.id"));
                }
                non_empty(
                    &row.policy_digest,
                    "organization_authorization_floors.permission_policy_digest",
                )?;
                if row.reject_through_ms.get() < 0 {
                    return Err(corrupt(
                        "organization_authorization_floors.request_reject_through_ms",
                    ));
                }
                check_times(row.created_at, row.updated_at)?;
                observation.organization_floor = Some(ObservedOrganizationFloor {
                    revision: row.minimum_revision,
                    state: row.state,
                });
            }
            SnapshotRow::PersonFloor(row) => {
                if row.organization_id != *organization {
                    return Err(corrupt(
                        "person_organization_authorization_floors.organization_id",
                    ));
                }
                check_times(row.created_at, row.updated_at)?;
                observation.person_floors.push(ObservedPersonFloor {
                    subject_id: row.id,
                    person_id: row.person_id,
                    revision: row.minimum_revision,
                    state: row.state,
                });
            }
            SnapshotRow::UsedRequest(row) => {
                if row.organization_id != *organization {
                    return Err(corrupt("organization_used_requests.organization_id"));
                }
                if row.request_time_ms.get() < 0 {
                    return Err(corrupt("organization_used_requests.request_time_ms"));
                }
                canonical_v7(
                    &row.session_id,
                    "organization_used_requests.actor_session_id",
                )?;
                non_empty(
                    &row.intent_digest,
                    "organization_used_requests.intent_digest",
                )?;
                non_empty(
                    &row.proof_key_thumbprint,
                    "organization_used_requests.proof_key_thumbprint",
                )?;
                check_times(row.created_at, row.updated_at)?;
                let _validated_marker = (row.operation_id, row.person_id);
                observation
                    .used_requests
                    .push(ObservedUsedRequest { id: row.id });
            }
            SnapshotRow::Workspace(row) => {
                let name = ResourceName::try_from(row.name.clone())
                    .map_err(|_| corrupt("workspaces.name"))?;
                // ResourceName trims incoming input. A stored row must already
                // be canonical; observation must never rewrite its name.
                if name.as_str() != row.name {
                    return Err(corrupt("workspaces.name"));
                }
                observation.workspaces.push(ObservedWorkspace {
                    workspace_id: row.workspace_id,
                    name,
                });
                let _validated_directory = (row.state, row.created_at, row.updated_at);
            }
        }
    }
    Ok(observation)
}

/// Observes an already-opened, already-migrated owning store. Unknown-org handling
/// belongs to the host, which returns `nothing_stored()` without opening storage.
pub async fn observe_signin_organization(
    host: &impl ObjectHost,
    organization: &OrganizationId,
) -> Result<SignInObservation, StoreFailure> {
    let rows = host
        .read(Statement {
            sql: SNAPSHOT,
            args: vec![],
        })
        .await?;
    decode_snapshot(&rows, organization)
}

#[cfg(test)]
#[path = "signin_test_observation_test.rs"]
mod tests;
