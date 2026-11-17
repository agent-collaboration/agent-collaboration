//! An organization with a real workspace, today's timers and the four P8 engine
//! admin routes called through the real Worker sign-in binding with real person
//! proofs. The sign-in fixture captures the Worker's logs, so the engine's
//! `central engine:` observations are readable here.
use super::super::workerd_host::RemoteRuntime;
use super::support::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::signin_fixtures::{self as signed, Person};
use collaboration_central_core::WorkspaceCaller;
use router_clock::RuntimeClock;
use serde_json::{Value, json};
use std::sync::Arc;
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
const MACHINE: &str = "019f0000-0000-7000-8000-00000000b302";
pub(super) type Time = chrono::DateTime<chrono::Utc>;

pub(super) fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
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
        let f = Fixture::new().await;
        assert_eq!(f.provision().await.0, 200);
        f.floors().await;
        f.execute("INSERT INTO machines(machine_id,installation_id,label,state,allowed_workspaces,created_at,updated_at) VALUES (?,?,'Engine admin fixture','enrolled','[]','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![text(MACHINE),text("019f0000-0000-7000-8000-00000000b303")]).await;
        f.execute("INSERT INTO machine_credentials(credential_id,machine_id,token_hash,expires_at,created_at,updated_at) VALUES (?,?,'signin-fixture-hash','2033-05-18T03:33:20Z','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![text("019f0000-0000-7000-8000-00000000b304"),text(MACHINE)]).await;
        let caller = WorkspaceCaller {
            organization_id: f.org.clone(),
            caller: m::Caller::Session(m::SessionAddress {
                machine_id: MACHINE.to_owned().try_into().unwrap(),
                endpoint: "codex-local".to_owned().try_into().unwrap(),
                session_id: "engine-admin".to_owned().try_into().unwrap(),
            }),
            credential_hash: "signin-fixture-hash".to_owned(),
        };
        let mut engine = Self {
            f,
            caller,
            workspace: "019f0000-0000-7000-8000-00000000b301"
                .to_owned()
                .try_into()
                .unwrap(),
            serial: std::cell::Cell::new(1),
        };
        let created = engine
            .request(
                None,
                m::WorkspaceOperation::CreateWorkspace {
                    name: "Engine admin".to_owned().try_into().unwrap(),
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
            .execute(
                "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
                vec![
                    text(serde_json::to_string(&vec![engine.workspace.clone()]).unwrap()),
                    text(MACHINE),
                ],
            )
            .await;
        engine
    }
    pub fn now(&self) -> Time {
        self.f.clock.utc_now()
    }
    pub fn set(&self, at: Time) {
        self.f.clock.set_utc(at);
    }
    fn now_seconds(&self) -> i64 {
        self.now().timestamp()
    }
    pub fn fresh(&self) -> String {
        signed::fresh_request_id(self.now_seconds()).unwrap()
    }
    /// The organization's runtime, with the held time pushed to the Worker.
    pub async fn runtime(&self) -> Arc<RemoteRuntime> {
        self.f.runtime().await
    }
    pub async fn rows(&self, sql: &'static str, args: Vec<SqlValue>) -> Vec<Vec<SqlValue>> {
        self.runtime()
            .await
            .read(Statement { sql, args })
            .await
            .unwrap()
            .rows
    }
    pub async fn count(&self, sql: &'static str, args: Vec<SqlValue>) -> i64 {
        match &self.rows(sql, args).await[0][0] {
            SqlValue::Integer(count) => count.get(),
            other => panic!("count: {other:?}"),
        }
    }
    pub async fn raw(&self, sql: &'static str, args: Vec<SqlValue>) {
        assert!(matches!(
            self.runtime()
                .await
                .run_batch(StatementBatch {
                    statements: vec![GuardedStatement {
                        statement: Statement { sql, args },
                        expect: RowExpectation::Any,
                    }],
                    alarm: AlarmUpdate::Unchanged,
                })
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
    }
    pub async fn after_event(&self) {
        self.runtime().await.after_event().await.unwrap();
    }
    pub async fn alarm_ms(&self) -> Option<i64> {
        self.runtime()
            .await
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap()
    }
    pub async fn invocations(&self) -> collaboration_central_core::LegacyUnitInvocations {
        self.runtime().await.legacy_unit_invocations().await
    }
    pub async fn request(
        &self,
        workspace: Option<m::WorkspaceId>,
        operation: m::WorkspaceOperation,
    ) -> m::WorkspaceReply {
        self.f
            .host
            .request(
                &self.caller,
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
        let serial = self.serial.get();
        self.serial.set(serial + 1);
        let m::ItemRef::Area(area_id) = self
            .write(m::WorkspaceOperation::CreateArea {
                name: format!("Area {serial}").try_into().unwrap(),
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
    pub async fn observer(&self, thread: &m::ThreadId) -> m::ActorIdentityId {
        let mut observer = self.caller.clone();
        let m::Caller::Session(address) = &mut observer.caller else {
            panic!("session caller")
        };
        address.session_id = "engine-observer".to_owned().try_into().unwrap();
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
    async fn central(&self, operation: CentralMailOperation) -> CentralMailResult {
        self.f
            .host
            .request_central(
                &self.caller,
                CentralMailRequest {
                    request_id: self.fresh().try_into().unwrap(),
                    operation,
                },
            )
            .await
            .unwrap()
    }
    pub fn target(&self) -> m::SessionAddress {
        let m::Caller::Session(address) = &self.caller.caller else {
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
    /// The Worker's logged failure counts for one subject, in order.
    pub fn observed(&self, key: &str) -> Vec<(i64, bool)> {
        self.f
            .host
            .captured_logs()
            .iter()
            .filter_map(|line| {
                let rest = line.split("central engine: kind=").nth(1)?;
                let (_, rest) = rest.split_once(" key=")?;
                let (logged, rest) = rest.split_once(" class=")?;
                (logged == key).then_some(())?;
                let (_, rest) = rest.split_once(" count=")?;
                let (count, rest) = rest.split_once(" retired=")?;
                let (retired, _) = rest.split_once(" workspaces=")?;
                Some((count.parse().ok()?, retired == "true"))
            })
            .collect()
    }
    pub async fn until_observed(&self, key: &str, class: &str, count: i64) {
        self.f
            .host
            .until_log(&format!("key={key} class={class} count={count} "))
            .await;
    }

    pub fn person(&self, permissions: &[&str], second_factor: bool) -> Person {
        let mut person = Person::owner(ORG, self.now_seconds()).unwrap();
        person.permissions = permissions
            .iter()
            .map(|value| (*value).to_owned())
            .collect();
        if !second_factor {
            person.second_factor_at = None;
        }
        person
    }
    pub fn repairer(&self) -> Person {
        self.person(&[READ_AUDIT, REPAIR_WORK], true)
    }
    async fn admin(&self, path: &str, body: Value, intent: Value, person: &Person) -> Value {
        // Push the held time to the Worker before the proof is judged.
        self.runtime().await;
        let headers = person
            .headers(path, &intent, &self.fresh(), self.now_seconds())
            .unwrap();
        let (status, value) = self.f.post_with(path, &body, &headers).await;
        assert_eq!(status, 200, "{path}: {value}");
        value
    }
    pub async fn health_as(&self, person: &Person) -> Value {
        self.admin(
            HEALTH,
            json!({"organizationId":ORG}),
            json!({"v":1,"operation":"EngineHealth","organization":ORG,"target":ORG,"operationId":null,"params":{}}),
            person,
        )
        .await
    }
    pub async fn health(&self) -> Value {
        self.health_as(&self.person(&[READ_AUDIT], false)).await
    }
    pub async fn retired_as(&self, after: Option<&str>, limit: u32, person: &Person) -> Value {
        self.admin(
            RETIRED,
            json!({"organizationId":ORG,"after":after,"limit":limit}),
            json!({"v":1,"operation":"EngineListRetired","organization":ORG,"target":ORG,"operationId":null,"params":{"after":after,"limit":limit}}),
            person,
        )
        .await
    }
    pub async fn retired(&self, after: Option<&str>, limit: u32) -> Value {
        self.retired_as(after, limit, &self.person(&[READ_AUDIT], false))
            .await
    }
    pub async fn change_as(
        &self,
        path: &str,
        operation_id: &str,
        attempt: &str,
        person: &Person,
    ) -> Value {
        let operation = if path == RETRY {
            "EngineRetry"
        } else {
            "EngineDiscard"
        };
        self.admin(
            path,
            json!({"organizationId":ORG,"operationId":operation_id,"attemptId":attempt}),
            json!({"v":1,"operation":operation,"organization":ORG,"target":ORG,"operationId":operation_id,"params":{"attemptId":attempt}}),
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
        self.f.close().await;
    }
}

pub(super) fn refused(reason: &str) -> Value {
    json!({"result":"refused","reason":reason})
}
pub(super) fn kind_health<'a>(health: &'a Value, kind: &str) -> &'a Value {
    health["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == kind)
        .unwrap_or_else(|| panic!("{kind} missing from {health}"))
}
