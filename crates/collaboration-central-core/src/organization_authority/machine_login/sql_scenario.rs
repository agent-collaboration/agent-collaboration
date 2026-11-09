//! One scripted machine-login scenario that both hosts run on a fresh store:
//! Tokio on sqlx-turso, workerd on Durable Object SQLite. It builds a Fleet as
//! today's code does, runs B3's staged switch, then drives the decisions
//! through their public entries. It reports what each step observed, and
//! `expected_scenario` is the one report both hosts must produce.
//!
//! Fixture-only: this runs the staged, unregistered corpus. Production
//! bootstrap is unchanged.
use super::decision_support::MachineLoginSettings;
use super::device_login_decision::decide_device_login;
use super::enrollment_port::{Decided, EnrollmentQuery, MachineEnrollmentPort};
use super::enrollment_sql_store::SqlEnrollmentPort;
use super::machine_connection::{
    RevocationBy, admit_machine_connection, revoke_machine_enrollment,
};
use super::machine_token_decision::decide_machine_token;
use crate::identity_contract::{
    AdmitMachineConnection, AllowedWorkspaces, ClientId, DecideDeviceLogin, DecideMachineToken,
    DeviceLoginDecision, DeviceLoginRequestId, EnrollPermission, FormerServiceId, PersonId,
    PresentedMachine, RevokeMachineEnrollment, TokenLifetimeSeconds,
};
use crate::person_request::AdminOperationId;
use durable_steps::{AtomicStore, StoreError, Uuid, storage::TransactionScope};
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId, UnixSeconds};
use fleet_sync_protocol::PeerId;
use message_board::{MachineLabel, WorkspaceId};
use runtime_primitives::{Clock, IdSource, SyncTime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// What one step of the scenario observed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScenarioStep {
    pub name: String,
    pub observed: Value,
}

const NOW_MS: i64 = 1_800_000_000_000;
const NOW_S: i64 = NOW_MS / 1000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const FORMER: &str = "0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7d";

fn id(suffix: u64) -> String {
    format!("019f0000-0000-7000-8000-{suffix:012x}")
}

struct At(i64);
impl Clock for At {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0)
    }
}
fn at(now_ms: i64) -> Arc<dyn Clock> {
    Arc::new(At(now_ms))
}

/// UUIDv7s counting up from a block, so both hosts mint the same IDs.
struct Counting(AtomicU64);
impl IdSource for Counting {
    fn new_id(&self) -> Uuid {
        let next = self.0.fetch_add(1, Ordering::SeqCst);
        Uuid::parse_str(&id(next)).unwrap_or_default()
    }
}
fn ids(block: u64) -> Arc<dyn IdSource> {
    Arc::new(Counting(AtomicU64::new(block)))
}

fn typed<T: TryFrom<String>>(value: String) -> Result<T, String> {
    T::try_from(value).map_err(|_| "a fixture value is invalid".to_owned())
}

fn approval(request: u64, former: bool, code_expires_at: i64) -> Result<DecideDeviceLogin, String> {
    Ok(DecideDeviceLogin {
        organization_id: typed(id(0x01))?,
        approver: typed::<PersonId>(id(0x02))?,
        permission_checked: EnrollPermission::MachineEnroll,
        request_id: typed::<DeviceLoginRequestId>(id(request))?,
        installation: typed::<InstallationId>(id(0x10))?,
        peer: PeerId::try_from(id(0x10)).map_err(|error| error.to_string())?,
        label: typed::<MachineLabel>("scenario machine".to_owned())?,
        allowed_workspaces: AllowedWorkspaces::try_from(vec![typed::<WorkspaceId>(id(0x20))?])
            .map_err(|error| error.to_string())?,
        approver_second_factor_at: Some(UnixSeconds(NOW_S)),
        former_service_id: former
            .then(|| typed::<FormerServiceId>(FORMER.to_owned()))
            .transpose()?,
        code_expires_at: UnixSeconds(code_expires_at),
    })
}

fn token(enrollment: &EnrollmentId) -> Result<DecideMachineToken, String> {
    Ok(DecideMachineToken {
        organization_id: typed(id(0x01))?,
        enrollment_id: enrollment.clone(),
        client_id: typed::<ClientId>(enrollment.as_str().to_owned())?,
    })
}

fn json_of<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn approved(answer: &DeviceLoginDecision) -> Result<EnrollmentId, String> {
    match answer {
        DeviceLoginDecision::Approved { enrollment_id } => Ok(enrollment_id.clone()),
        DeviceLoginDecision::Refused { reason } => Err(format!("refused: {reason:?}")),
    }
}

/// Runs the scenario on a fresh store, in the live initialization order, for
/// one fixed organization so every host runs exactly the same inputs.
#[expect(
    clippy::too_many_lines,
    reason = "one linear script, so both hosts run exactly the same steps in order"
)]
pub async fn run_scenario<Store, Driver>(store: Arc<Store>) -> Result<Vec<ScenarioStep>, String>
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'blob> Vec<u8>: Type<Driver> + Encode<'blob, Driver> + Decode<'blob, Driver>,
    for<'scalar> i64: Type<Driver> + Encode<'scalar, Driver> + Decode<'scalar, Driver>,
    for<'blob> Option<Vec<u8>>: Type<Driver> + Decode<'blob, Driver>,
    for<'scalar> Option<i64>: Type<Driver> + Decode<'scalar, Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    let organization: OrganizationId = typed(id(0x01))?;
    let mut steps = Vec::new();
    let mut step = |name: &str, observed: Value| {
        steps.push(ScenarioStep {
            name: name.to_owned(),
            observed,
        });
    };
    let error = |error: StoreError| error.to_string();

    // Today's Fleet, with one machine written by 0001/0002 code.
    store
        .transaction(|scope| {
            Box::pin(async move {
                crate::object_bootstrap::initialize_pre_b3_fleet(
                    scope,
                    SyncTime::from_unix_millis(NOW_MS),
                    &At(NOW_MS),
                )
                .await?;
                crate::generated_enrollment::insert_enrollment(
                    scope,
                    id(0x10),
                    FORMER.to_owned(),
                    "scenario machine".to_owned(),
                    "legacy-client".to_owned(),
                    "legacy-hash".to_owned(),
                    "2033-05-18T03:33:20+00:00".to_owned(),
                    "2026-11-08T12:00:00.123+01:00".to_owned(),
                    "2026-11-08T12:00:00.123+01:00".to_owned(),
                )
                .await
                .map_err(StoreError::backend)?;
                sqlx_core::query::query::<Driver>(sqlx_core::sql_str::AssertSqlSafe(format!(
                    "INSERT INTO fleet_workspaces(workspace_id, name, description, state, \
                     repository_count, actor_kind, fleet_position, time_basis, \
                     origin_installation, origin_machine, created_at, updated_at) VALUES('{}', \
                     'scenario', '', 'active', 0, 'human', 1, 'router', '{}', 'scenario', 0, 0)",
                    id(0x20),
                    id(0x0f)
                )))
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .map_err(error)?;
    for name in ["switch", "switch again"] {
        let applied = store
            .transaction(|scope| {
                Box::pin(async move {
                    crate::object_bootstrap::initialize_staged_b3_registry(
                        scope,
                        SyncTime::from_unix_millis(NOW_MS),
                        &At(NOW_MS),
                    )
                    .await
                })
            })
            .await
            .map_err(error)?;
        step(name, json!(applied));
    }

    let port = SqlEnrollmentPort::<Store, Driver>::new(store.clone(), organization.clone());
    let machine = || EnrollmentQuery {
        installation: InstallationId::try_from(id(0x10)).ok(),
        ..EnrollmentQuery::default()
    };
    let converted = port
        .transact(machine(), |facts| {
            Decided::unchanged(facts.installation.as_ref().map(|row| {
                json!({"generation": row.generation.0, "label": row.label.as_str(), "active": row.active.is_some()})
            }))
        })
        .await
        .map_err(|failure| failure.to_string())?;
    step("converted machine", json!(converted));

    // A login carrying the converted machine's own former id.
    let first = decide_device_login(
        &port,
        approval(0x30, true, NOW_S + 600)?,
        NOW_MS,
        at(NOW_MS),
        ids(0xa000),
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("approve A", json_of(&first));
    let a = approved(&first)?;
    let granted = decide_machine_token(
        &port,
        token(&a)?,
        at(NOW_MS + 1000),
        ids(0xb000),
        settings()?,
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("first grant A", json_of(&granted));

    // Two approvals at one generation: the first Grant wins (BA1).
    let b = approved(
        &decide_device_login(
            &port,
            approval(0x31, false, NOW_S + 600)?,
            NOW_MS + 2000,
            at(NOW_MS + 2000),
            ids(0xa100),
        )
        .await
        .map_err(|failure| failure.to_string())?,
    )?;
    let c = approved(
        &decide_device_login(
            &port,
            approval(0x32, false, NOW_S + 600)?,
            NOW_MS + 2000,
            at(NOW_MS + 2000),
            ids(0xa200),
        )
        .await
        .map_err(|failure| failure.to_string())?,
    )?;
    let grant_b = decide_machine_token(
        &port,
        token(&b)?,
        at(NOW_MS + 3000),
        ids(0xb100),
        settings()?,
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("first grant B supersedes A", json_of(&grant_b));
    let grant_c = decide_machine_token(
        &port,
        token(&c)?,
        at(NOW_MS + 4000),
        ids(0xb200),
        settings()?,
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("first grant C refused", json_of(&grant_c));

    // The code's own deadline (N10): approved at minute 9, expiring at minute 10.
    let d = approved(
        &decide_device_login(
            &port,
            approval(0x33, false, NOW_S + 60)?,
            NOW_MS + 5000,
            at(NOW_MS + 5000),
            ids(0xa300),
        )
        .await
        .map_err(|failure| failure.to_string())?,
    )?;
    let lapsed = decide_machine_token(
        &port,
        token(&d)?,
        at((NOW_S + 60) * 1000),
        ids(0xb300),
        settings()?,
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("grant D at its code deadline", json_of(&lapsed));

    // A repeated request: pending the owner's fingerprint decision.
    let repeat = decide_device_login(
        &port,
        approval(0x30, true, NOW_S + 600)?,
        NOW_MS + 6000,
        at(NOW_MS + 6000),
        ids(0xa400),
    )
    .await;
    step(
        "repeat A's request",
        json!(
            repeat
                .map(|answer| json_of(&answer))
                .map_err(|failure| failure.to_string())
        ),
    );

    let connection = AdmitMachineConnection {
        organization_id: organization.clone(),
        enrollment_id: b.clone(),
        installation: typed(id(0x10))?,
        presented: PresentedMachine {
            installation: typed(id(0x10))?,
            peer: PeerId::try_from(id(0x10)).map_err(|error| error.to_string())?,
        },
        token_expires_at: UnixSeconds(NOW_S + 900),
        client_id: typed(b.as_str().to_owned())?,
    };
    let admitted = admit_machine_connection(
        &port,
        connection.clone(),
        at(NOW_MS + 7000),
        ids(0xd000),
        settings()?,
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("admit B", json_of(&admitted));

    // Thirty days idle: an automatic expiry, recorded.
    let idle = decide_machine_token(
        &port,
        token(&b)?,
        at(NOW_MS + 3000 + 30 * DAY_MS),
        ids(0xb400),
        settings()?,
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("grant B after thirty idle days", json_of(&idle));
    let after_expiry = admit_machine_connection(
        &port,
        connection,
        at(NOW_MS + 31 * DAY_MS),
        ids(0xd100),
        settings()?,
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("admit B after expiry", json_of(&after_expiry));

    let revoked = revoke_machine_enrollment(
        &port,
        &RevokeMachineEnrollment {
            organization_id: organization,
            installation: typed(id(0x10))?,
        },
        RevocationBy {
            person: typed(id(0x03))?,
            operation: AdminOperationId::try_from(id(0x50)).map_err(|error| error.to_string())?,
        },
        at(NOW_MS + 32 * DAY_MS),
        ids(0xc000),
    )
    .await
    .map_err(|failure| failure.to_string())?;
    step("revoke with nothing active", json_of(&revoked.answer));

    let records = store
        .transaction(|scope| {
            Box::pin(async move {
                crate::organization_authority::change_records::read_page(scope, None, 100).await
            })
        })
        .await
        .map_err(error)?;
    let mut kinds = records
        .iter()
        .map(|record| format!("{:?} {:?}", record.kind, record.origin))
        .collect::<Vec<_>>();
    kinds.sort();
    step("change records", json!(kinds));
    let mut keys = records
        .iter()
        .map(|record| record.request_id.as_str().to_owned())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    step("distinct record keys", json!(keys.len() == records.len()));
    Ok(steps)
}

fn settings() -> Result<MachineLoginSettings, String> {
    TokenLifetimeSeconds::try_from(900)
        .map(MachineLoginSettings::new)
        .map_err(|error| error.to_string())
}

/// The report every host must produce.
#[must_use]
pub fn expected_scenario() -> Vec<ScenarioStep> {
    let grant = |enrollment: u64, decided: i64, superseded: Option<u64>| {
        let mut answer = json!({
            "result": "grant",
            "installation": id(0x10),
            "enrollmentId": id(enrollment),
            "decidedAt": decided,
            "grantExpiresAt": decided + 900,
        });
        if let (Some(ended), Some(fields)) = (superseded, answer.as_object_mut()) {
            fields.insert(
                "superseded".to_owned(),
                json!({"enrollmentId": id(ended), "clientId": id(ended)}),
            );
        }
        answer
    };
    let person = |id_suffix: u64, permission: &str| {
        format!(
            r#"Person {{ person_id: "{}", permission_checked: "{permission}" }}"#,
            id(id_suffix)
        )
    };
    let enroll = person(0x02, "machine:enroll");
    let mut records = vec![
        format!("AllowanceChanged {enroll}"),
        format!("MachineEnrolled {enroll}"),
        format!("MachineRevoked {enroll}"),
        format!("MachineEnrolled {enroll}"),
        "MachineExpired Automatic".to_owned(),
        "AllowanceChanged Automatic".to_owned(),
    ];
    records.sort();
    let steps = [
        ("switch", json!(2)),
        ("switch again", json!(0)),
        (
            "converted machine",
            json!({"generation": 0, "label": "scenario machine", "active": false}),
        ),
        (
            "approve A",
            json!({"result": "approved", "enrollmentId": id(0xa000)}),
        ),
        ("first grant A", grant(0xa000, NOW_S + 1, None)),
        (
            "first grant B supersedes A",
            grant(0xa100, NOW_S + 3, Some(0xa000)),
        ),
        (
            "first grant C refused",
            json!({"result": "refuse", "reason": {"kind": "superseded"}}),
        ),
        (
            "grant D at its code deadline",
            json!({"result": "refuse", "reason": {"kind": "enrollmentNotActive"}}),
        ),
        (
            "repeat A's request",
            json!({"Err": "a repeated device-login request cannot be compared on this store yet"}),
        ),
        (
            "admit B",
            json!({"result": "admitted", "machine": {
                "installation": id(0x10), "peer": id(0x10), "organizationId": id(0x01),
                "enrollmentId": id(0xa100), "tokenExpiresAt": NOW_S + 900}}),
        ),
        (
            "grant B after thirty idle days",
            json!({"result": "refuse", "reason": {"kind": "idleExpired"}}),
        ),
        (
            "admit B after expiry",
            json!({"result": "refused", "reason": {"kind": "enrollmentNotActive"}}),
        ),
        (
            "revoke with nothing active",
            json!({"result": "refused", "reason": {"kind": "notFound"}}),
        ),
        ("change records", json!(records)),
        ("distinct record keys", json!(true)),
    ];
    steps
        .into_iter()
        .map(|(name, observed)| ScenarioStep {
            name: name.to_owned(),
            observed,
        })
        .collect()
}

#[cfg(test)]
#[path = "sql_scenario_test.rs"]
mod tests;
