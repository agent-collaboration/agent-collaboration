//! An organization with a real workspace, today's timers and the four P8 engine
//! admin routes called through the real sign-in link with real person proofs.
// Test-only module: clippy's test allowances (clippy.toml) apply to its helpers.
#![cfg(test)]
use super::support::*;
use central_storage::SqlValue;
use collaboration_central_core::WorkspaceCaller;
use router_clock::RuntimeClock;
use serde_json::{Value, json};
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

pub(super) const HEALTH: &str = "/organization-admin/v1/engine-health";
pub(super) const RETIRED: &str = "/organization-admin/v1/engine-retired";
pub(super) const RETRY: &str = "/organization-admin/v1/engine-retry";
pub(super) const DISCARD: &str = "/organization-admin/v1/engine-discard";
pub(super) const READ_AUDIT: &str = "organization:readAudit";
pub(super) const REPAIR_WORK: &str = "organization:repairWork";
pub(super) type Time = chrono::DateTime<chrono::Utc>;

pub(super) fn minutes(count: i64) -> chrono::Duration {
    chrono::Duration::minutes(count)
}
pub(super) fn stamp(at: Time) -> SqlValue {
    text(at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
pub(super) fn integer(value: i64) -> SqlValue {
    SqlValue::Integer(value.try_into().unwrap())
}
/// The first 5-minute mark strictly after `at`.
pub(super) fn next_mark(at: Time) -> Time {
    let millis = at.timestamp_millis();
    chrono::DateTime::from_timestamp_millis((millis.div_euclid(300_000) + 1) * 300_000).unwrap()
}

pub(super) struct EngineOrg {
    pub f: Fixture,
    pub caller: WorkspaceCaller,
    pub workspace: m::WorkspaceId,
    serial: std::cell::Cell<u32>,
}
impl EngineOrg {
    pub async fn new() -> Self {
        let f = Fixture::new().await.unwrap();
        f.ready().await.unwrap();
        f.seed_machine().await.unwrap();
        let caller = f.machine_caller("engine-admin").await.unwrap();
        let mut engine = Self {
            f,
            caller,
            workspace: WORKSPACE.to_owned().try_into().unwrap(),
            serial: std::cell::Cell::new(1),
        };
        let created = engine
            .request(
                None,
                m::WorkspaceOperation::CreateWorkspace {
                    name: "Engine admin".to_owned().try_into().unwrap(),
                    description: None,
                },
            )
            .await;
        let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
            created.result
        else {
            panic!("workspace creation refused: {created:?}")
        };
        engine.workspace = workspace.workspace_id;
        engine
            .f
            .apply(
                A,
                "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
                vec![
                    text(serde_json::to_string(&vec![engine.workspace.clone()]).unwrap()),
                    text(INSTALLATION),
                ],
            )
            .await
            .unwrap();
        engine
    }
    pub fn now(&self) -> Time {
        self.f.clock.utc_now()
    }
    pub async fn set(&self, at: Time) {
        self.f.clock.set_utc(at).await;
    }
    /// A fresh UUIDv7 for requests and operation IDs at the held time.
    pub fn fresh(&self) -> String {
        signed::fresh_request_id(self.f.clock_time()).unwrap()
    }
    pub async fn runtime(&self) -> std::sync::Arc<Runtime> {
        self.f.runtime(A).await.unwrap()
    }
    pub async fn rows(&self, sql: &'static str, args: Vec<SqlValue>) -> Vec<Vec<SqlValue>> {
        self.f.rows(A, sql, args).await.unwrap()
    }
    pub async fn count(&self, sql: &'static str, args: Vec<SqlValue>) -> i64 {
        match &self.rows(sql, args).await[0][0] {
            SqlValue::Integer(count) => count.get(),
            other => panic!("count: {other:?}"),
        }
    }
    pub async fn raw(&self, sql: &'static str, args: Vec<SqlValue>) {
        self.f.apply(A, sql, args).await.unwrap();
    }
    pub async fn after_event(&self) {
        self.runtime().await.after_event().await.unwrap();
    }
    pub async fn alarm_ms(&self) -> Option<i64> {
        self.runtime()
            .await
            .statement_host()
            .alarm()
            .map(|time| time.unix_millis())
    }
    pub async fn invocations(&self) -> collaboration_central_core::LegacyUnitInvocations {
        self.runtime().await.legacy_unit_invocations()
    }
    pub async fn request(
        &self,
        workspace: Option<m::WorkspaceId>,
        operation: m::WorkspaceOperation,
    ) -> m::WorkspaceReply {
        let caller = self.f.machine_caller("engine-admin").await.unwrap();
        self.f
            .host
            .request(
                &caller,
                m::WorkspaceRequest {
                    request_id: operation
                        .is_write()
                        .then(|| self.fresh().try_into().unwrap()),
                    workspace_id: workspace,
                    operation,
                },
            )
            .await
            .unwrap()
    }
    pub async fn write(&self, operation: m::WorkspaceOperation) -> m::ItemRef {
        match self
            .request(Some(self.workspace.clone()), operation)
            .await
            .result
        {
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { item, .. }) => item,
            other => panic!("not created: {other:?}"),
        }
    }
    pub async fn thread(&self) -> m::ThreadId {
        let m::ItemRef::Area(area_id) = self
            .write(m::WorkspaceOperation::CreateArea {
                name: format!("Area {}", self.bump()).try_into().unwrap(),
                description: "Test description".to_owned().try_into().unwrap(),
            })
            .await
        else {
            panic!("area")
        };
        let m::ItemRef::Thread(thread) = self
            .write(m::WorkspaceOperation::CreateThread {
                area_id,
                title: "Engine thread".to_owned().try_into().unwrap(),
                body: "First post".to_owned().try_into().unwrap(),
                addressees: vec![],
            })
            .await
        else {
            panic!("thread")
        };
        thread
    }
    fn bump(&self) -> u32 {
        let next = self.serial.get();
        self.serial.set(next + 1);
        next
    }
    /// Joins a second session as observer and returns its actor identity.
    pub async fn observer(&self, thread: &m::ThreadId) -> m::ActorIdentityId {
        let observer = self.f.machine_caller("engine-observer").await.unwrap();
        let joined = self
            .f
            .host
            .request(
                &observer,
                m::WorkspaceRequest {
                    request_id: Some(self.fresh().try_into().unwrap()),
                    workspace_id: Some(self.workspace.clone()),
                    operation: m::WorkspaceOperation::JoinThread {
                        thread_id: thread.clone(),
                        role: m::MemberRole::Observer,
                    },
                },
            )
            .await
            .unwrap();
        assert!(
            matches!(joined.result, m::WorkspaceResult::Done(_)),
            "{joined:?}"
        );
        let actors = self
            .rows(
                "SELECT actor_identity_id,identity FROM actor_identities",
                vec![],
            )
            .await;
        actors
            .iter()
            .find_map(|row| match (&row[0], &row[1]) {
                (SqlValue::Text(id), SqlValue::Text(identity))
                    if identity.contains("engine-observer") =>
                {
                    Some(id.parse().unwrap())
                }
                _ => None,
            })
            .unwrap()
    }
    pub async fn post(&self, thread: &m::ThreadId) {
        self.write(m::WorkspaceOperation::ThreadPost {
            thread_id: thread.clone(),
            body: "Notice activity".to_owned().try_into().unwrap(),
            addressees: vec![],
        })
        .await;
    }
    pub async fn central(&self, operation: CentralMailOperation) -> CentralMailResult {
        let caller = self.f.machine_caller("engine-admin").await.unwrap();
        self.f
            .host
            .request_central(
                &caller,
                CentralMailRequest {
                    request_id: self.fresh().try_into().unwrap(),
                    operation,
                },
            )
            .await
            .unwrap()
    }
    pub fn target(&self) -> workspace_ids::SessionRef {
        let m::Caller::Session(address) = self.caller.caller() else {
            panic!("session caller")
        };
        address.clone()
    }
    pub async fn wake(&self, due: Time, label: &str) -> String {
        let result = self
            .central(CentralMailOperation::SetWake {
                target: self.target(),
                due_at: due,
                text: label.to_owned().try_into().unwrap(),
            })
            .await;
        let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = result else {
            panic!("wake refused: {result:?}")
        };
        wake_id.to_string()
    }
    pub async fn automation(&self, at: Time, label: &str) -> String {
        let result = self
            .central(CentralMailOperation::SetAutomation {
                target: self.target(),
                timing: serde_json::from_value(json!({"kind":"at","at":at})).unwrap(),
                text: label.to_owned().try_into().unwrap(),
            })
            .await;
        let CentralMailResult::Done(CentralMailAnswer::AutomationSet { automation_id, .. }) =
            result
        else {
            panic!("automation refused: {result:?}")
        };
        automation_id.to_string()
    }
    pub async fn wake_messages(&self, wake: &str) -> i64 {
        self.count(
            "SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?",
            vec![text(wake)],
        )
        .await
    }
    /// (state, failure_count, next_try_at, last_failure)
    pub async fn record(&self, kind: &str, key: &str) -> Vec<Vec<SqlValue>> {
        self.rows(
            "SELECT state,failure_count,next_try_at,last_failure FROM central_work_attempts WHERE machine_kind=? AND subject_key=?",
            vec![text(kind), text(key)],
        )
        .await
    }
    pub async fn attempt_id(&self, kind: &str, key: &str) -> String {
        match &self
            .rows(
                "SELECT attempt_id FROM central_work_attempts WHERE machine_kind=? AND subject_key=?",
                vec![text(kind), text(key)],
            )
            .await[..]
        {
            [row] => match &row[0] {
                SqlValue::Text(id) => id.clone(),
                other => panic!("attempt id: {other:?}"),
            },
            other => panic!("one record expected for {kind}/{key}: {other:?}"),
        }
    }

    /// A person with the given permissions and an optional fresh second factor.
    pub fn person(&self, permissions: &[&str], second_factor: bool) -> signed::Person {
        let now = self.f.clock_time();
        let mut person = signed::Person::owner(A, now).unwrap();
        person.permissions = permissions
            .iter()
            .map(|value| (*value).to_owned())
            .collect();
        if !second_factor {
            person.second_factor_at = None;
        }
        person
    }
    pub fn repairer(&self) -> signed::Person {
        self.person(&[READ_AUDIT, REPAIR_WORK], true)
    }
    async fn admin(
        &self,
        path: &str,
        body: Value,
        intent: Value,
        person: &signed::Person,
    ) -> Value {
        let reply = self
            .f
            .person(path, &body, person, &intent, &self.fresh())
            .await
            .unwrap();
        assert_eq!(
            reply.status,
            200,
            "{path}: {:?}",
            String::from_utf8_lossy(&reply.body)
        );
        reply.value().unwrap()
    }
    pub async fn health_as(&self, person: &signed::Person) -> Value {
        self.admin(
            HEALTH,
            json!({"organizationId":A}),
            json!({"v":1,"operation":"EngineHealth","organization":A,"target":A,"operationId":null,"params":{}}),
            person,
        )
        .await
    }
    pub async fn health(&self) -> Value {
        self.health_as(&self.person(&[READ_AUDIT], false)).await
    }
    pub async fn retired_as(
        &self,
        after: Option<&str>,
        limit: u32,
        person: &signed::Person,
    ) -> Value {
        self.admin(
            RETIRED,
            json!({"organizationId":A,"after":after,"limit":limit}),
            json!({"v":1,"operation":"EngineListRetired","organization":A,"target":A,"operationId":null,"params":{"after":after,"limit":limit}}),
            person,
        )
        .await
    }
    pub async fn retired(&self, after: Option<&str>, limit: u32) -> Value {
        self.retired_as(after, limit, &self.person(&[READ_AUDIT], false))
            .await
    }
    /// EngineRetry or EngineDiscard with an explicit operation ID.
    pub async fn change_as(
        &self,
        path: &str,
        operation_id: &str,
        attempt: &str,
        person: &signed::Person,
    ) -> Value {
        let operation = if path == RETRY {
            "EngineRetry"
        } else {
            "EngineDiscard"
        };
        self.admin(
            path,
            json!({"organizationId":A,"operationId":operation_id,"attemptId":attempt}),
            json!({"v":1,"operation":operation,"organization":A,"target":A,"operationId":operation_id,"params":{"attemptId":attempt}}),
            person,
        )
        .await
    }
    pub async fn retry(&self, attempt: &str) -> Value {
        self.change_as(RETRY, &self.fresh(), attempt, &self.repairer())
            .await
    }
    pub async fn discard(&self, attempt: &str) -> Value {
        self.change_as(DISCARD, &self.fresh(), attempt, &self.repairer())
            .await
    }
    pub async fn close(self) {
        self.f.close().await.unwrap();
    }
}

pub(super) fn refused(reason: &str) -> Value {
    json!({"result":"refused","reason":reason})
}
/// The health entry for one kind.
pub(super) fn kind_health<'a>(health: &'a Value, kind: &str) -> &'a Value {
    health["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == kind)
        .unwrap_or_else(|| panic!("{kind} missing from {health}"))
}
