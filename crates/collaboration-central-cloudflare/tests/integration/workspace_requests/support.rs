//! Real org core requests, actual approved migrations and owning SQLite readback.
use super::workerd_host::WorkerdHost;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::signin_fixtures as signed;
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::{Clock, SyncTime};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
use workspace_model as m;

pub(super) struct HeldClock(pub(super) AtomicI64);
impl Clock for HeldClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
pub(super) fn id<T: std::str::FromStr>(value: u32) -> T
where
    T::Err: std::fmt::Debug,
{
    format!("01890f2e-7b4c-7cc0-98c4-{value:012x}")
        .parse()
        .unwrap()
}
pub(super) fn name(value: &str) -> m::ResourceName {
    value.to_owned().try_into().unwrap()
}
pub(super) fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
#[derive(Clone)]
pub(super) struct FixtureCaller {
    pub organization_id: OrganizationId,
    pub caller: m::Caller,
    pub token: String,
    pub enrollment_id: String,
}
pub(super) const FIXTURE_NOW: i64 = 1_861_920_000;
pub(super) fn caller() -> FixtureCaller {
    FixtureCaller {
        organization_id: serde_json::from_str(&format!("\"{}\"", id::<m::WorkspaceId>(1))).unwrap(),
        caller: m::Caller::Session(m::SessionAddress {
            machine_id: id(2),
            endpoint: "codex-local".to_owned().try_into().unwrap(),
            session_id: "test-session".to_owned().try_into().unwrap(),
        }),
        token: signed::machine_token(
            &id::<m::WorkspaceId>(1).to_string(),
            &id::<m::MachineId>(2).to_string(),
            &id::<m::WorkspaceId>(4).to_string(),
            FIXTURE_NOW,
            FIXTURE_NOW + 600,
        )
        .unwrap(),
        enrollment_id: id::<m::WorkspaceId>(4).to_string(),
    }
}
pub(super) fn machine_caller(
    organization_id: OrganizationId,
    caller: m::Caller,
    enrollment_id: String,
    now: i64,
) -> FixtureCaller {
    let token = signed::machine_token(
        organization_id.as_str(),
        &caller.machine_id().to_string(),
        &enrollment_id,
        now,
        now + 600,
    )
    .unwrap();
    FixtureCaller {
        organization_id,
        caller,
        token,
        enrollment_id,
    }
}
pub(super) async fn seed_enrollment(
    host: &WorkerdHost,
    caller: &FixtureCaller,
    private_machine: impl ToString,
    allowed_workspaces: &str,
    now: i64,
) {
    let runtime = host.central(&caller.organization_id).await;
    let at = chrono::DateTime::from_timestamp(now, 0)
        .unwrap()
        .to_rfc3339();
    let code_expires = chrono::DateTime::from_timestamp(now + 600, 0)
        .unwrap()
        .to_rfc3339();
    let statements=[
        ("INSERT INTO machines (machine_id,installation_id,label,state,allowed_workspaces,enrollment_generation,created_at,updated_at) VALUES (?,?,?,'enrolled',?,1,?,?)",vec![text(private_machine.to_string()),text(caller.caller.machine_id()),text("Machine"),text(allowed_workspaces),text(&at),text(&at)]),
        ("INSERT INTO machine_approvals(approval_id,request_id,machine_id,label,allowed_workspaces,state,end_reason,approved_by_person_id,permission_checked,base_generation,code_expires_at,machine_client_id,last_grant_at,ended_at,created_at,updated_at) VALUES (?,?,?,'Machine',?,'enrolled',NULL,?,'machine:enroll',0,?,?,?,NULL,?,?)",vec![text(&caller.enrollment_id),text(&caller.enrollment_id),text(private_machine.to_string()),text(allowed_workspaces),text(signed::OWNER),text(code_expires),text(&caller.enrollment_id),text(&at),text(&at),text(&at)]),
    ].into_iter().map(|(sql,args)|GuardedStatement {statement:Statement {sql,args},expect:RowExpectation::Exactly(1)}).collect();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements,
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
}
pub(super) async fn seed_organization(host: &WorkerdHost, caller: &FixtureCaller) {
    let runtime = host.central(&caller.organization_id).await;
    // Existing clock-rollback cases stay within this independently signed standing.
    let statement = signed::stamp(caller.organization_id.as_str(), FIXTURE_NOW - 3600).unwrap();
    assert!(matches!(runtime.run_batch(StatementBatch { statements: vec![GuardedStatement { statement: Statement { sql: "INSERT INTO organization_identity(id,organization_statement,created_at,updated_at) VALUES (?,?,0,0)", args: vec![text(caller.organization_id.as_str()), text(statement)] }, expect: RowExpectation::Exactly(1) }], alarm: AlarmUpdate::Unchanged }).await.unwrap(), BatchOutcome::Applied { .. }));
}
pub(super) async fn seed(host: &WorkerdHost, caller: &FixtureCaller) {
    seed_organization(host, caller).await;
    seed_enrollment(host, caller, id::<m::WorkspaceId>(3), "[]", FIXTURE_NOW).await;
}
// A fresh bearer represents a renewed connection; existing connection deadlines never move.
pub(super) fn fresh_auth(host: &WorkerdHost, caller: &FixtureCaller) -> FixtureCaller {
    machine_caller(
        caller.organization_id.clone(),
        caller.caller.clone(),
        caller.enrollment_id.clone(),
        host.now_seconds(),
    )
}
// Epoch relocation is an explicit test precondition, separate from renewing a connection.
pub(super) async fn relocate_authority(
    host: &WorkerdHost,
    caller: &FixtureCaller,
) -> FixtureCaller {
    let now = host.now_seconds();
    let at = chrono::DateTime::from_timestamp(now, 0)
        .unwrap()
        .to_rfc3339();
    let runtime = host.central(&caller.organization_id).await;
    let rows = runtime
        .read(Statement {
            sql: "SELECT organization_statement FROM organization_identity WHERE id=?",
            args: vec![text(caller.organization_id.as_str())],
        })
        .await
        .unwrap();
    let SqlValue::Text(predecessor) = &rows.rows[0][0] else {
        panic!("organization statement")
    };
    let renewed = signed::renewal(caller.organization_id.as_str(), predecessor, now, 0).unwrap();
    let response = reqwest::Client::new()
        .post(format!("{}/internal/organization-provisioning", host.signin_origin()))
        .json(&serde_json::json!({"organizationId":caller.organization_id,"organizationStatement":renewed}))
        .send().await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert!(matches!(runtime.run_batch(StatementBatch { statements: vec![
        GuardedStatement { statement: Statement { sql: "UPDATE machine_approvals SET last_grant_at=?,updated_at=? WHERE approval_id=? AND state='enrolled'", args: vec![text(&at), text(&at), text(&caller.enrollment_id)] }, expect: RowExpectation::Exactly(1) },
    ], alarm: AlarmUpdate::Unchanged }).await.unwrap(), BatchOutcome::Applied { .. }));
    fresh_auth(host, caller)
}
pub(super) async fn call(
    host: &WorkerdHost,
    caller: &FixtureCaller,
    workspace: Option<m::WorkspaceId>,
    request: u32,
    operation: m::WorkspaceOperation,
) -> m::WorkspaceReply {
    host.request(
        caller,
        m::WorkspaceRequest {
            request_id: operation.is_write().then(|| id(request)),
            workspace_id: workspace,
            operation,
        },
    )
    .await
    .unwrap()
}
pub(super) async fn create_workspace(host: &WorkerdHost, caller: &FixtureCaller) -> m::WorkspaceId {
    let reply = call(
        host,
        caller,
        None,
        10,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Workspace"),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) = reply.result
    else {
        panic!("workspace creation refused: {reply:?}");
    };
    let runtime = host.central(&caller.organization_id).await;
    let batch = StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "UPDATE machines SET allowed_workspaces=? WHERE installation_id=?",
                args: vec![
                    text(serde_json::to_string(&vec![workspace.workspace_id.clone()]).unwrap()),
                    text(caller.caller.machine_id()),
                ],
            },
            expect: RowExpectation::Exactly(1),
        }],
        alarm: AlarmUpdate::Unchanged,
    };
    assert!(matches!(
        runtime.statement_host().run_batch(batch).await.unwrap(),
        BatchOutcome::Applied { .. }
    ));
    workspace.workspace_id
}
pub(super) fn created(reply: m::WorkspaceReply) -> m::ItemRef {
    match reply.result {
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { item, .. }) => item,
        other => panic!("not created: {other:?}"),
    }
}

pub(super) struct Fixture {
    pub host: WorkerdHost,
    pub caller: FixtureCaller,
    pub workspace: m::WorkspaceId,
    pub clock: Arc<HeldClock>,
}
impl Fixture {
    pub async fn new() -> Self {
        let clock = Arc::new(HeldClock(AtomicI64::new(1_861_920_000_000)));
        let host = WorkerdHost::open(clock.clone()).await.unwrap();
        let caller = caller();
        seed(&host, &caller).await;
        let workspace = create_workspace(&host, &caller).await;
        Self {
            host,
            caller,
            workspace,
            clock,
        }
    }
    pub async fn call(&self, request: u32, operation: m::WorkspaceOperation) -> m::WorkspaceReply {
        let caller = fresh_auth(&self.host, &self.caller);
        call(
            &self.host,
            &caller,
            Some(self.workspace.clone()),
            request,
            operation,
        )
        .await
    }
    pub async fn area(&self, request: u32) -> m::AreaId {
        let m::ItemRef::Area(id) = created(
            self.call(
                request,
                m::WorkspaceOperation::CreateArea {
                    name: name(&format!("Area {request}")),
                },
            )
            .await,
        ) else {
            panic!("area");
        };
        id
    }
    pub async fn project(&self, request: u32, area_id: m::AreaId) -> m::ProjectId {
        let m::ItemRef::Project(id) = created(
            self.call(
                request,
                m::WorkspaceOperation::CreateProject {
                    area_id,
                    title: name(&format!("Project {request}")),
                    lead: None,
                },
            )
            .await,
        ) else {
            panic!("project");
        };
        id
    }
    pub async fn deliverable(&self, request: u32, project_id: m::ProjectId) -> m::DeliverableId {
        let m::ItemRef::Deliverable(id) = created(
            self.call(
                request,
                m::WorkspaceOperation::CreateDeliverable {
                    project_id,
                    title: name(&format!("Deliverable {request}")),
                },
            )
            .await,
        ) else {
            panic!("deliverable");
        };
        id
    }
    pub async fn thread(&self, request: u32, area_id: m::AreaId) -> m::ThreadId {
        let m::ItemRef::Thread(id) = created(
            self.call(
                request,
                m::WorkspaceOperation::CreateThread {
                    area_id,
                    title: name(&format!("Thread {request}")),
                    body: "First post".to_owned().try_into().unwrap(),
                    addressees: vec![],
                },
            )
            .await,
        ) else {
            panic!("thread");
        };
        id
    }
    pub async fn reopen(&mut self) {
        self.host
            .reset_owners(&self.caller.organization_id, &self.workspace)
            .await;
    }
    pub async fn workspace_batch(&self, sql: &'static str, args: Vec<SqlValue>) {
        self.workspace_execute(sql, args, RowExpectation::Exactly(1))
            .await;
    }
    pub async fn workspace_execute(
        &self,
        sql: &'static str,
        args: Vec<SqlValue>,
        expect: RowExpectation,
    ) {
        let runtime = self.host.central(&self.caller.organization_id).await;
        assert!(matches!(
            runtime
                .workspace_batch(
                    &self.workspace,
                    StatementBatch {
                        statements: vec![GuardedStatement {
                            statement: Statement { sql, args },
                            expect
                        }],
                        alarm: AlarmUpdate::Unchanged
                    }
                )
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
    }
}

pub(super) fn session(caller: &FixtureCaller) -> m::SessionAddress {
    let m::Caller::Session(session) = &caller.caller else {
        panic!("fixture requires session caller");
    };
    session.clone()
}
