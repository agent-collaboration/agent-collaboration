//! Real org core requests, actual approved migrations and owning SQLite readback.
use super::workerd_host::WorkerdHost;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_core::WorkspaceCaller;
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
pub(super) fn caller() -> WorkspaceCaller {
    WorkspaceCaller {
        organization_id: serde_json::from_str(&format!("\"{}\"", id::<m::WorkspaceId>(1))).unwrap(),
        caller: m::Caller::Session(m::SessionAddress {
            machine_id: id(2),
            endpoint: "codex-local".to_owned().try_into().unwrap(),
            session_id: "test-session".to_owned().try_into().unwrap(),
        }),
        credential_hash: "fixture-credential-digest".to_owned(),
    }
}
pub(super) async fn seed(host: &WorkerdHost, caller: &WorkspaceCaller) {
    let runtime = host.central(&caller.organization_id).await;
    let statements=[
        ("INSERT INTO organization_identity(id,organization_statement,created_at,updated_at) VALUES (?,'fixture organization statement',0,0)",vec![text(caller.organization_id.as_str())]),
        ("INSERT INTO machines (machine_id,installation_id,label,state,allowed_workspaces,created_at,updated_at) VALUES (?,?,?,'enrolled',?,?,?)",vec![text(caller.caller.machine_id()),text(id::<m::WorkspaceId>(3)),text("Machine"),text("[]"),text("2026-11-13T00:00:00Z"),text("2026-11-13T00:00:00Z")]),
        ("INSERT INTO machine_credentials (credential_id,machine_id,token_hash,expires_at,created_at,updated_at) VALUES (?,?,?,?,?,?)",vec![text(id::<m::WorkspaceId>(4)),text(caller.caller.machine_id()),text(&caller.credential_hash),text("2030-01-01T00:00:00Z"),text("2026-11-13T00:00:00Z"),text("2026-11-13T00:00:00Z")]),
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
pub(super) async fn call(
    host: &WorkerdHost,
    caller: &WorkspaceCaller,
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
pub(super) async fn create_workspace(
    host: &WorkerdHost,
    caller: &WorkspaceCaller,
) -> m::WorkspaceId {
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
                sql: "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
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
    pub caller: WorkspaceCaller,
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
        call(
            &self.host,
            &self.caller,
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

pub(super) fn session(caller: &WorkspaceCaller) -> m::SessionAddress {
    let m::Caller::Session(session) = &caller.caller else {
        panic!("fixture requires session caller");
    };
    session.clone()
}
