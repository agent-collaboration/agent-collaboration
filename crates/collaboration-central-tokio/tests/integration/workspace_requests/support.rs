//! Real org core requests, actual approved migrations and owning SQLite readback.
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_core::WorkspaceCaller;
use collaboration_central_tokio::TokioWorkspaceHost;
use std::sync::Arc;
use workspace_model as m;

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
pub(super) use collaboration_central_conformance::signin_fixtures as signed;
pub(super) fn organization() -> fleet_identity_protocol::OrganizationId {
    id::<m::WorkspaceId>(1).to_string().try_into().unwrap()
}
pub(super) fn settings(url: &str) -> collaboration_central_core::SignInTrustSettings {
    collaboration_central_core::SignInTrustSettings {
        issuer: signed::ISSUER.into(),
        audience: signed::AUDIENCE.into(),
        admin_audience: signed::ADMIN_AUDIENCE.into(),
        machine_audience: signed::MACHINE_AUDIENCE.into(),
        public_origin: signed::PUBLIC_ORIGIN.into(),
        jwks_url: url.into(),
        service_id: Some(id::<m::WorkspaceId>(9).to_string().try_into().unwrap()),
    }
}
pub(super) async fn seed_machine(
    host: &TokioWorkspaceHost,
    machine_id: &workspace_ids::MachineId,
    enrollment: u32,
    now: i64,
) {
    seed_machine_for(host, &organization(), machine_id, enrollment, now).await;
}
pub(super) async fn seed_machine_for(
    host: &TokioWorkspaceHost,
    organization: &fleet_identity_protocol::OrganizationId,
    machine_id: &workspace_ids::MachineId,
    enrollment: u32,
    now: i64,
) {
    let runtime = host.central(organization).await.unwrap();
    let at = chrono::DateTime::from_timestamp(now, 0)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let statements = [
        ("INSERT INTO machines(machine_id,label,state,allowed_workspaces,enrollment_generation,created_at,updated_at) VALUES (?,'Machine','enrolled','[]',1,?,?)", vec![text(machine_id), text(&at), text(&at)]),
        ("INSERT INTO machine_approvals(approval_id,request_id,machine_id,label,allowed_workspaces,state,approved_by_person_id,permission_checked,base_generation,code_expires_at,machine_client_id,last_grant_at,created_at,updated_at) VALUES (?,?,?,'Machine','[]','enrolled',?,'machine:enroll',0,?,?,?,?,?)", vec![text(id::<m::WorkspaceId>(enrollment)),text(id::<m::WorkspaceId>(enrollment+1)),text(machine_id),text(signed::OWNER),text(&at),text(id::<m::WorkspaceId>(enrollment)),text(&at),text(&at),text(&at)]),
    ].into_iter().map(|(sql,args)| GuardedStatement { statement: Statement { sql,args }, expect: RowExpectation::Exactly(1) }).collect();
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
pub(super) async fn authenticated(
    host: &TokioWorkspaceHost,
    caller: m::Caller,
    enrollment: &str,
    now: i64,
) -> WorkspaceCaller {
    let runtime = host.central(&organization()).await.unwrap();
    let token = signed::machine_token(
        organization().as_str(),
        &caller.machine_id().to_string(),
        enrollment,
        now,
        now + 600,
    )
    .unwrap();
    let connection = runtime
        .admit_machine_connection(&token)
        .await
        .unwrap()
        .unwrap();
    WorkspaceCaller::new(connection, caller).unwrap()
}
pub(super) async fn call(
    host: &TokioWorkspaceHost,
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
    host: &TokioWorkspaceHost,
    caller: &WorkspaceCaller,
) -> m::WorkspaceId {
    let reply = call(
        host,
        caller,
        None,
        10,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Workspace"),
            description: None,
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) = reply.result
    else {
        panic!("workspace creation refused: {reply:?}");
    };
    let runtime = host.central(caller.organization_id()).await.unwrap();
    let batch = StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
                args: vec![
                    text(serde_json::to_string(&vec![workspace.workspace_id.clone()]).unwrap()),
                    text(caller.caller().machine_id()),
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
    pub host: TokioWorkspaceHost,
    pub caller: WorkspaceCaller,
    pub workspace: m::WorkspaceId,
    pub clock: Arc<router_clock::GatedClock>,
    pub root: tempfile::TempDir,
    pub keys: signed::SignInKeys,
}
impl Fixture {
    pub async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let clock = Arc::new(
            router_clock::GatedClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
                1_794_571_200_000,
            ))
            .unwrap(),
        );
        let keys = signed::SignInKeys::start().await.unwrap();
        let host =
            TokioWorkspaceHost::open_with_signin(root.path(), clock.clone(), settings(&keys.url))
                .await
                .unwrap();
        use runtime_primitives::Clock;
        let now = clock.now().unix_millis().div_euclid(1000);
        let runtime = host.central(&organization()).await.unwrap();
        assert!(runtime.statement_host().read(Statement { sql: "SELECT name FROM sqlite_schema WHERE type='table' AND name='machine_credentials'", args: vec![] }).await.unwrap().rows.is_empty());
        runtime
            .provision_organization(
                &collaboration_central_core::identity_contract::ProvisionOrganization {
                    organization_id: organization(),
                    organization_statement: signed::stamp(organization().as_str(), now - 3600)
                        .unwrap()
                        .try_into()
                        .unwrap(),
                },
            )
            .await
            .unwrap();
        seed_machine(&host, &id(2), 4, now).await;
        let caller = authenticated(
            &host,
            m::Caller::Session(workspace_ids::SessionRef {
                machine_id: id(2),
                session_id: "test-session".to_owned().try_into().unwrap(),
            }),
            &id::<m::WorkspaceId>(4).to_string(),
            now,
        )
        .await;
        let workspace = create_workspace(&host, &caller).await;
        Self {
            host,
            caller,
            workspace,
            clock,
            root,
            keys,
        }
    }
    /// Re-admits the fixture session with a machine token minted at the clock's
    /// current time. A test that moves the clock past the token's life calls
    /// this; central refuses frames after a token's effective expiry.
    pub async fn refresh_caller(&mut self) {
        use runtime_primitives::Clock;
        let now = self.clock.now().unix_millis().div_euclid(1000);
        self.caller = authenticated(
            &self.host,
            self.caller.caller().clone(),
            &id::<m::WorkspaceId>(4).to_string(),
            now,
        )
        .await;
    }
    pub async fn relocate_authority(&mut self, at: m::Timestamp) {
        self.set_utc(at).await;
        let runtime = self
            .host
            .central(self.caller.organization_id())
            .await
            .unwrap();
        let current = runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT organization_statement FROM organization_identity",
                args: vec![],
            })
            .await
            .unwrap();
        let [row] = current.rows.as_slice() else {
            panic!("fixture statement")
        };
        let [SqlValue::Text(previous)] = row.as_slice() else {
            panic!("fixture statement")
        };
        let statement =
            signed::renewal(organization().as_str(), previous, at.timestamp(), 0).unwrap();
        runtime
            .provision_organization(
                &collaboration_central_core::identity_contract::ProvisionOrganization {
                    organization_id: organization(),
                    organization_statement: statement.try_into().unwrap(),
                },
            )
            .await
            .unwrap();
        let at = chrono::DateTime::from_timestamp(at.timestamp(), 0)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        assert!(matches!(runtime.statement_host().run_batch(StatementBatch {
            statements: vec![GuardedStatement { statement: Statement { sql: "UPDATE machine_approvals SET last_grant_at=?,updated_at=? WHERE state='enrolled'", args: vec![text(&at),text(&at)] }, expect: RowExpectation::Any }], alarm: AlarmUpdate::Unchanged,
        }).await.unwrap(), BatchOutcome::Applied { .. }));
        self.caller = self.fresh_caller(&self.caller).await;
    }
    pub async fn fresh_caller(&self, caller: &WorkspaceCaller) -> WorkspaceCaller {
        use runtime_primitives::Clock;
        authenticated(
            &self.host,
            caller.caller().clone(),
            caller.connection().enrollment_id().as_str(),
            self.clock.now().unix_millis().div_euclid(1000),
        )
        .await
    }
    /// Moves the clock forward once every held alarm drive has finished; no
    /// drive starts during the move (the clock's gate).
    pub async fn advance(&self, step: std::time::Duration) {
        self.clock.advance(step).await.unwrap();
    }
    /// Moves the clock to `at` under the same gate.
    pub async fn advance_to(&self, at: runtime_primitives::SyncTime) {
        self.clock.advance_to(at).await.unwrap();
    }
    /// Sets the wall clock under the same gate.
    pub async fn set_utc(&self, utc: chrono::DateTime<chrono::Utc>) {
        self.clock.set_utc(utc).await;
    }
    /// The one deliberate overlap (mail/engine_failure_overlap).
    pub fn set_utc_while_a_batch_is_held(&self, utc: chrono::DateTime<chrono::Utc>) {
        self.clock.set_utc_while_a_batch_is_held(utc);
    }
    /// The same clock, read-only, for a runtime a case builds itself.
    pub fn shared_for_runtime(&self) -> Arc<dyn router_clock::RuntimeClock> {
        self.clock.clone()
    }
    pub async fn call(&self, request: u32, operation: m::WorkspaceOperation) -> m::WorkspaceReply {
        use runtime_primitives::Clock;
        // Normal scenario requests reconnect with a fresh verified token once their original grant expires.
        // Expiry probes call the host/runtime directly with the retained original context.
        let renewed;
        let caller = if self.clock.now().unix_millis().div_euclid(1000)
            >= self.caller.connection().effective_expires_at().0
        {
            renewed = self.fresh_caller(&self.caller).await;
            &renewed
        } else {
            &self.caller
        };
        call(
            &self.host,
            caller,
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
                    description: "Test description".to_owned().try_into().unwrap(),
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
                    description: "Test description".to_owned().try_into().unwrap(),
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
        self.host.close().await;
        self.host = TokioWorkspaceHost::open_with_signin(
            self.root.path(),
            self.clock.clone(),
            settings(&self.keys.url),
        )
        .await
        .unwrap();
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
        let store = collaboration_central_tokio::ObjectStorage::new(
            self.root
                .path()
                .join("workspace-stores")
                .join(format!("{}.sqlite", self.workspace)),
            Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator()),
        );
        assert!(matches!(
            store
                .run_batch(StatementBatch {
                    statements: vec![GuardedStatement {
                        statement: Statement { sql, args },
                        expect
                    }],
                    alarm: AlarmUpdate::Unchanged
                })
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
        store.close().await;
    }
}

pub(super) fn session(caller: &WorkspaceCaller) -> workspace_ids::SessionRef {
    let m::Caller::Session(session) = caller.caller() else {
        panic!("fixture requires session caller");
    };
    session.clone()
}

/// Provisions one organization on the shared host, enrolls the fixture machine
/// and admits a session caller with a signed machine token.
pub async fn admitted_organization<C: runtime_primitives::Clock + ?Sized>(
    host: &TokioWorkspaceHost,
    organization: &fleet_identity_protocol::OrganizationId,
    clock: &C,
) -> collaboration_central_conformance::ConformanceResult<collaboration_central_core::WorkspaceCaller>
{
    let now = clock.now().unix_millis().div_euclid(1000);
    let runtime = host.central(organization).await?;
    runtime
        .provision_organization(
            &collaboration_central_core::identity_contract::ProvisionOrganization {
                organization_id: organization.clone(),
                organization_statement: signed::stamp(organization.as_str(), now - 3600)?
                    .try_into()?,
            },
        )
        .await?;
    seed_machine_for(host, organization, &id(2), 4, now).await;
    let token = signed::machine_token(
        organization.as_str(),
        &id::<workspace_ids::MachineId>(2).to_string(),
        &id::<m::WorkspaceId>(4).to_string(),
        now,
        now + 600,
    )?;
    let connection = runtime
        .admit_machine_connection(&token)
        .await?
        .map_err(|_| "fixture machine admission refused")?;
    let caller = collaboration_central_core::WorkspaceCaller::new(
        connection,
        m::Caller::Session(workspace_ids::SessionRef {
            machine_id: id(2),
            session_id: "test-session".to_owned().try_into()?,
        }),
    )
    .map_err(|_| "fixture session caller refused")?;
    create_workspace(host, &caller).await;
    Ok(caller)
}
