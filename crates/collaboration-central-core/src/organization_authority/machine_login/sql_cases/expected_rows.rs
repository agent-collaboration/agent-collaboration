//! The rows a step may write, spelled out in full, and the check that a step
//! changed exactly those rows and nothing else. Expected values come from the
//! decision rules: who approved, the injected clock and the injected ID block.
use super::case_values::{APPROVER, LABEL, REVOKER, uuid7};
use super::sql_case_host::{CaseHost, Snapshot, snapshot};
use super::{CaseResult, check_eq};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// One row as the readback keys it, with its full column values.
pub(super) type Entry = (String, Value);

/// The store's rows now, for a later `expect_changes`.
pub(super) async fn before<Host: CaseHost>(host: &Host) -> CaseResult<Snapshot> {
    snapshot(host).await
}

/// Since `before`, the step changed exactly these rows to exactly these values.
pub(super) async fn expect_changes<Host: CaseHost>(
    host: &Host,
    before: &Snapshot,
    expected: Vec<Entry>,
    step: &str,
) -> CaseResult<Snapshot> {
    let after = snapshot(host).await?;
    let mut changed = BTreeMap::new();
    for (key, value) in &after {
        if before.get(key) != Some(value) {
            changed.insert(key.clone(), Some(value.clone()));
        }
    }
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        changed.insert(key.clone(), None);
    }
    let expected = expected
        .into_iter()
        .map(|(key, value)| (key, Some(value)))
        .collect::<BTreeMap<_, _>>();
    check_eq!(changed, expected, "{step}: the rows it changed");
    Ok(after)
}

/// Since `before`, the step wrote nothing.
pub(super) async fn expect_unchanged<Host: CaseHost>(
    host: &Host,
    before: &Snapshot,
    step: &str,
) -> CaseResult {
    expect_changes(host, before, Vec::new(), step)
        .await
        .map(drop)
}

/// The whole store holds exactly these rows.
pub(super) async fn expect_rows<Host: CaseHost>(
    host: &Host,
    expected: Vec<Entry>,
    step: &str,
) -> CaseResult {
    let rows = snapshot(host).await?;
    check_eq!(
        rows,
        expected.into_iter().collect::<Snapshot>(),
        "{step}: the store's rows"
    );
    Ok(())
}

#[derive(Clone, Debug)]
pub(super) struct Installation {
    pub machine: u64,
    pub former: Option<String>,
    pub label: String,
    pub generation: i64,
    pub last_seen_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}
impl Installation {
    /// First written by an approval at `at_ms`.
    pub fn approved(machine: u64, at_ms: i64) -> Self {
        Self {
            machine,
            former: None,
            label: LABEL.to_owned(),
            generation: 0,
            last_seen_at: None,
            created_at: at_ms,
            updated_at: at_ms,
        }
    }
    /// Rewritten at `at_ms` with this generation.
    pub fn at(&self, generation: i64, at_ms: i64) -> Self {
        Self {
            generation,
            updated_at: at_ms,
            ..self.clone()
        }
    }
    pub fn entry(&self) -> Entry {
        let id = uuid7(self.machine);
        (
            format!("machine_installations/{id}"),
            json!({
                "id": id,
                "former_service_id": self.former,
                "label": self.label,
                "enrollment_generation": self.generation,
                "last_seen_at": self.last_seen_at,
                "created_at": self.created_at,
                "updated_at": self.updated_at,
            }),
        )
    }
}

/// An enrollment row. `created_at` is the approval time and never changes.
#[derive(Clone, Debug)]
pub(super) struct Enrollment {
    pub id: u64,
    pub machine: u64,
    pub request: u64,
    pub state: &'static str,
    pub end_reason: Option<&'static str>,
    pub base_generation: i64,
    pub code_expires_at_s: i64,
    pub client: Option<u64>,
    pub last_grant_at_s: Option<i64>,
    pub ended_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}
impl Enrollment {
    pub fn approved(
        id: u64,
        machine: u64,
        request: u64,
        base_generation: i64,
        at_ms: i64,
        code_expires_at_s: i64,
    ) -> Self {
        Self {
            id,
            machine,
            request,
            state: "approved",
            end_reason: None,
            base_generation,
            code_expires_at_s,
            client: None,
            last_grant_at_s: None,
            ended_at: None,
            created_at: at_ms,
            updated_at: at_ms,
        }
    }
    /// Granted at `at_ms`: its own client, the Grant's second.
    pub fn granted(&self, at_ms: i64) -> Self {
        Self {
            state: "enrolled",
            client: Some(self.id),
            last_grant_at_s: Some(at_ms.div_euclid(1000)),
            updated_at: at_ms,
            ..self.clone()
        }
    }
    /// Ended at `at_ms`: revoked or expired keep their client; lapsed has none.
    pub fn ended(&self, state: &'static str, end_reason: Option<&'static str>, at_ms: i64) -> Self {
        Self {
            state,
            end_reason,
            last_grant_at_s: None,
            ended_at: Some(at_ms),
            updated_at: at_ms,
            ..self.clone()
        }
    }
    pub fn entry(&self) -> Entry {
        let id = uuid7(self.id);
        (
            format!("machine_enrollments/{id}"),
            json!({
                "id": id,
                "installation_id": uuid7(self.machine),
                "request_id": uuid7(self.request),
                "state": self.state,
                "end_reason": self.end_reason,
                "approved_by_person_id": uuid7(APPROVER),
                "permission_checked": "machine:enroll",
                "base_generation": self.base_generation,
                "code_expires_at": self.code_expires_at_s * 1000,
                "machine_client_id": self.client.map(uuid7),
                "last_grant_at": self.last_grant_at_s.map(|seconds| seconds * 1000),
                "ended_at": self.ended_at,
                "created_at": self.created_at,
                "updated_at": self.updated_at,
            }),
        )
    }
}

pub(super) fn allowance(
    id: u64,
    machine: u64,
    workspace: u64,
    state: &str,
    created_at: i64,
    updated_at: i64,
) -> Entry {
    let id = uuid7(id);
    (
        format!("machine_allowances/{id}"),
        json!({
            "id": id,
            "installation_id": uuid7(machine),
            "workspace_id": uuid7(workspace),
            "state": state,
            "created_at": created_at,
            "updated_at": updated_at,
        }),
    )
}

/// Who a record names as its origin.
#[derive(Clone, Copy, Debug)]
pub(super) enum Origin {
    Approver,
    Revoker,
    Automatic,
}

/// A change record: its key is `<originating id>:<kind>:<target id>`.
pub(super) fn record(
    id: u64,
    originating: u64,
    kind: &str,
    origin: Origin,
    target: u64,
    at_ms: i64,
) -> Entry {
    let (origin, actor, permission) = match origin {
        Origin::Approver => ("person", Some(uuid7(APPROVER)), Some("machine:enroll")),
        Origin::Revoker => ("person", Some(uuid7(REVOKER)), Some("machine:revoke")),
        Origin::Automatic => ("automatic", None, None),
    };
    let id = uuid7(id);
    let target = uuid7(target);
    (
        format!("organization_change_records/{id}"),
        json!({
            "id": id,
            "request_id": format!("{}:{kind}:{target}", uuid7(originating)),
            "kind": kind,
            "origin": origin,
            "actor_person_id": actor,
            "permission_checked": permission,
            "target_id": target,
            "created_at": at_ms,
            "updated_at": at_ms,
        }),
    )
}
