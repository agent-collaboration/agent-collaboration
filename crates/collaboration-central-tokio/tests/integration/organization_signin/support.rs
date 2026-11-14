use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
pub(super) use collaboration_central_conformance::signin_fixtures as signed;
pub(super) type Result<TValue = ()> = collaboration_central_conformance::ConformanceResult<TValue>;
use collaboration_central_core::{SignInTrustSettings, WorkspaceRuntime};
use collaboration_central_tokio::{
    ObjectStorage, OrganizationSignInLink, TokioWorkspaceHost, TokioWorkspaceStores,
};
use fleet_identity_protocol::OrganizationId;
use router_clock::ManualClock;
use runtime_primitives::SyncTime;
use serde_json::{Value, json};
use std::sync::Arc;
pub(super) const A: &str = signed::ORGANIZATION;
pub(super) const B: &str = "019f0000-0000-7000-8000-00000000b2b1";
pub(super) const WORKSPACE: &str = "019f0000-0000-7000-8000-00000000b301";
pub(super) const MACHINE: &str = "019f0000-0000-7000-8000-00000000b302";
pub(super) const PROVISION: &str = "/internal/organization-provisioning";
pub(super) const DIGEST: &str = "/internal/organization-statement-digest";
pub(super) const REVISIONS: &str = "/internal/organization-authorization-revisions";
pub(super) type Runtime = WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>;

pub(super) fn org(value: &str) -> Result<OrganizationId> {
    Ok(value.to_owned().try_into()?)
}
pub(super) fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
pub(super) fn number(value: i64) -> Result<SqlValue> {
    Ok(SqlValue::Integer(value.try_into()?))
}
pub(super) fn provision_body(organization: &str, statement: &str) -> Value {
    json!({"organizationId":organization,"organizationStatement":statement})
}
pub(super) fn organization_floor(organization: &str, revision: i64, state: &str) -> Value {
    json!({"kind":"organization","organizationId":organization,"revision":revision,"state":state,"permissionPolicyDigest":signed::POLICY})
}
pub(super) fn person_floor(
    organization: &str,
    person: &str,
    subject: &str,
    revision: i64,
    state: &str,
) -> Value {
    json!({"kind":"person","organizationId":organization,"personId":person,"subjectId":subject,"revision":revision,"state":state})
}

pub(super) struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}
impl Reply {
    pub fn value(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&self.body)?)
    }
}

pub(super) struct Fixture {
    pub root: tempfile::TempDir,
    pub clock: Arc<ManualClock>,
    pub host: Arc<TokioWorkspaceHost>,
    pub keys: signed::SignInKeys,
    pub socket: std::path::PathBuf,
    client: reqwest::Client,
    link: Option<OrganizationSignInLink>,
}
impl Fixture {
    pub async fn new() -> Result<Self> {
        Self::new_with_issuer(signed::ISSUER).await
    }
    pub async fn new_with_issuer(issuer: &str) -> Result<Self> {
        let root = tempfile::Builder::new().prefix("signin-").tempdir()?;
        let clock = Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(
            signed::NOW_SECONDS * 1000,
        ))?);
        let keys = signed::SignInKeys::start().await?;
        let socket = root.path().join("sign-in.sock");
        let mut trust = settings(&keys.url)?;
        trust.issuer = issuer.to_owned();
        let host = Arc::new(
            TokioWorkspaceHost::open_with_signin(root.path(), clock.clone(), trust).await?,
        );
        let link = OrganizationSignInLink::start(host.clone(), &socket)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .unix_socket(socket.clone())
            .build()?;
        Ok(Self {
            root,
            clock,
            host,
            keys,
            socket,
            client,
            link: Some(link),
        })
    }
    pub async fn post(&self, path: &str, body: &Value) -> Result<Reply> {
        post(&self.client, path, serde_json::to_string(body)?, vec![]).await
    }
    pub async fn person(
        &self,
        path: &str,
        body: &Value,
        person: &signed::Person,
        intent: &Value,
        jti: &str,
    ) -> Result<Reply> {
        let now = self.clock_time();
        post(
            &self.client,
            path,
            serde_json::to_string(body)?,
            person.headers(path, intent, jti, now)?,
        )
        .await
    }
    pub fn clock_time(&self) -> i64 {
        use runtime_primitives::Clock;
        self.clock.now().unix_millis().div_euclid(1000)
    }
    pub async fn provision(&self, organization: &str, statement: &str) -> Result<Reply> {
        self.post(PROVISION, &provision_body(organization, statement))
            .await
    }
    pub async fn digest(&self, organization: &str) -> Result<Reply> {
        self.post(DIGEST, &json!({"organizationId":organization}))
            .await
    }
    pub async fn runtime(&self, organization: &str) -> Result<Arc<Runtime>> {
        Ok(self.host.central(&org(organization)?).await?)
    }
    pub async fn rows(
        &self,
        organization: &str,
        sql: &'static str,
        args: Vec<SqlValue>,
    ) -> Result<Vec<Vec<SqlValue>>> {
        Ok(self
            .runtime(organization)
            .await?
            .statement_host()
            .read(Statement { sql, args })
            .await?
            .rows)
    }
    pub async fn apply(
        &self,
        organization: &str,
        sql: &'static str,
        args: Vec<SqlValue>,
    ) -> Result {
        let outcome = self
            .runtime(organization)
            .await?
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement { sql, args },
                    expect: RowExpectation::Any,
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await?;
        if !matches!(outcome, BatchOutcome::Applied { .. }) {
            return Err("fixture batch refused".into());
        }
        Ok(())
    }
    pub async fn snapshot(&self) -> Result<(Vec<Vec<SqlValue>>, Vec<Vec<SqlValue>>)> {
        Ok((self.rows(A,"SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id",vec![]).await?,self.rows(A,"SELECT id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at FROM organization_change_records ORDER BY id",vec![]).await?))
    }
    pub async fn ready(&self) -> Result {
        if self
            .provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
            .await?
            .status
            != 200
        {
            return Err("fixture provisioning refused".into());
        }
        for floor in [
            organization_floor(A, 3, "active"),
            person_floor(A, signed::OWNER, signed::OWNER_SUBJECT, 4, "active"),
            person_floor(A, signed::MEMBER, signed::MEMBER_SUBJECT, 1, "active"),
        ] {
            if self.post(REVISIONS, &floor).await?.status != 200 {
                return Err("fixture floor refused".into());
            }
        }
        Ok(())
    }
    pub async fn reopen(&mut self) -> Result {
        if let Some(link) = self.link.take() {
            link.shutdown().await?;
        }
        self.host.close().await;
        self.host = Arc::new(
            TokioWorkspaceHost::open_with_signin(
                self.root.path(),
                self.clock.clone(),
                settings(&self.keys.url)?,
            )
            .await?,
        );
        self.link = Some(OrganizationSignInLink::start(
            self.host.clone(),
            &self.socket,
        )?);
        self.client = reqwest::Client::builder()
            .no_proxy()
            .unix_socket(self.socket.clone())
            .build()?;
        Ok(())
    }
    pub fn client(&self) -> reqwest::Client {
        self.client.clone()
    }
    pub async fn close(mut self) -> Result {
        if let Some(link) = self.link.take() {
            link.shutdown().await?;
        }
        self.host.close().await;
        self.keys.shutdown().await?;
        Ok(())
    }
    pub async fn seed_machine(&self) -> Result {
        self.apply(A,"INSERT INTO machines(machine_id,installation_id,label,state,allowed_workspaces,created_at,updated_at) VALUES (?,?,'existing machine','enrolled','[]','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![text(MACHINE),text("019f0000-0000-7000-8000-00000000b303")]).await?;
        self.apply(A,"INSERT INTO machine_credentials(credential_id,machine_id,token_hash,expires_at,created_at,updated_at) VALUES (?,?,'signin-fixture-hash','2033-05-18T03:33:20Z','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![text("019f0000-0000-7000-8000-00000000b304"),text(MACHINE)]).await
    }
}
pub(super) async fn post(
    client: &reqwest::Client,
    path: &str,
    body: String,
    headers: Vec<(&'static str, String)>,
) -> Result<Reply> {
    let mut request = client
        .post(format!("http://central.local{path}"))
        .header("content-type", "application/json")
        .body(body);
    for (name, value) in headers {
        request = request.header(name, value)
    }
    let response = request.send().await?;
    Ok(Reply {
        status: response.status().as_u16(),
        body: response.bytes().await?.to_vec(),
    })
}
fn settings(url: &str) -> Result<SignInTrustSettings> {
    Ok(SignInTrustSettings {
        issuer: signed::ISSUER.into(),
        audience: signed::AUDIENCE.into(),
        admin_audience: signed::ADMIN_AUDIENCE.into(),
        public_origin: signed::PUBLIC_ORIGIN.into(),
        jwks_url: url.into(),
        service_id: Some(
            "019f0000-0000-7000-8000-00000000d200"
                .to_owned()
                .try_into()?,
        ),
    })
}
