use super::super::workerd_host::{RemoteRuntime, WorkerdHost};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, ReadOutcome, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::signin_fixtures::{
    self as fixture, NOW_SECONDS, Person, SignInKeys,
};
use fleet_identity_protocol::OrganizationId;
use router_clock::ManualClock;
use serde_json::{Value, json};
use std::sync::Arc;
pub const ORG: &str = "019f0000-0000-7000-8000-00000000d201";
pub const PROVISION: &str = "/internal/organization-provisioning";
pub const DIGEST: &str = "/internal/organization-statement-digest";
pub const REVISIONS: &str = "/internal/organization-authorization-revisions";
pub const RECORDS: &str = "/organization-admin/v1/change-records";
pub const LOOKUP: &str = "/organization-admin/v1/workspace-lookup";
pub struct Fixture {
    pub host: WorkerdHost,
    pub keys: SignInKeys,
    pub clock: Arc<ManualClock>,
    pub org: OrganizationId,
    pub _plan_root: tempfile::TempDir,
    client: reqwest::Client,
}
impl Fixture {
    pub async fn new() -> Self {
        Self::new_with_issuer(fixture::ISSUER).await
    }
    pub async fn new_with_issuer(issuer: &str) -> Self {
        let keys = SignInKeys::start().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let plan = root.path().join("signin-plan.json");
        std::fs::write(&plan,serde_json::to_vec(&json!({"textBindings":{
            "CENTRAL_SIGN_IN_ISSUER":issuer,"CENTRAL_SIGN_IN_AUDIENCE":fixture::AUDIENCE,
            "CENTRAL_SIGN_IN_JWKS_URL":"https://signin.test/.well-known/jwks.json",
            "CENTRAL_ADMIN_AUDIENCE":fixture::ADMIN_AUDIENCE,"CENTRAL_PUBLIC_ORIGIN":fixture::PUBLIC_ORIGIN,
            "CENTRAL_SIGN_IN_SERVICE_ID":"019f0000-0000-7000-8000-00000000d290",
            "ACCESS_TEAM_DOMAIN":"fixture.cloudflareaccess.com","ACCESS_SESSION_AUD":"session-fixture","ACCESS_ADMIN_AUD":"admin-fixture","ACCESS_OWNER_IDENTITY":"owner@example.invalid"
        },"jwksFixtureUrl":keys.url})).unwrap()).unwrap();
        let clock = Arc::new(
            ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
                NOW_SECONDS * 1000,
            ))
            .unwrap(),
        );
        let host = WorkerdHost::open_signin(clock.clone(), &plan)
            .await
            .unwrap();
        let org = ORG.to_owned().try_into().unwrap();
        host.central(&org).await;
        Self {
            host,
            keys,
            clock,
            org,
            _plan_root: root,
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
        }
    }
    pub async fn runtime(&self) -> Arc<RemoteRuntime> {
        self.host.central(&self.org).await
    }
    pub async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        self.post_with(path, body, &[]).await
    }
    pub async fn post_with(
        &self,
        path: &str,
        body: &Value,
        headers: &[(&str, String)],
    ) -> (u16, Value) {
        let (status, bytes) = self.post_bytes(path, body, headers).await;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }
    pub async fn post_bytes(
        &self,
        path: &str,
        body: &Value,
        headers: &[(&str, String)],
    ) -> (u16, Vec<u8>) {
        let mut request = self
            .client
            .post(format!("{}{path}", self.host.signin_origin()))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(body).unwrap());
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let response = request.send().await.unwrap();
        assert_eq!(
            response
                .headers()
                .get("x-collaboration-central-entry")
                .unwrap(),
            "collaboration-central-link-entry"
        );
        let status = response.status().as_u16();
        let bytes = response.bytes().await.unwrap().to_vec();
        (status, bytes)
    }
    pub async fn provision(&self) -> (u16, Value) {
        let stamp = fixture::stamp(ORG, NOW_SECONDS).unwrap();
        self.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":stamp}),
        )
        .await
    }
    pub async fn read(&self, sql: &'static str) -> ReadOutcome {
        self.runtime()
            .await
            .read(Statement { sql, args: vec![] })
            .await
            .unwrap()
    }
    pub async fn execute(&self, sql: &'static str, args: Vec<SqlValue>) {
        assert!(matches!(
            self.runtime()
                .await
                .run_batch(StatementBatch {
                    statements: vec![GuardedStatement {
                        statement: Statement { sql, args },
                        expect: RowExpectation::Exactly(1)
                    }],
                    alarm: AlarmUpdate::Unchanged
                })
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
    }
    pub async fn counts(&self) -> ReadOutcome {
        self.read("SELECT (SELECT COUNT(*) FROM organization_identity),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM organization_used_requests),(SELECT COUNT(*) FROM organization_authorization_floors),(SELECT COUNT(*) FROM person_organization_authorization_floors)").await
    }
    pub async fn floors(&self) {
        assert_eq!(
            self.post(REVISIONS, &org_revision(3, "active")).await.0,
            200
        );
        assert_eq!(
            self.post(REVISIONS, &person_revision(4, "active")).await.0,
            200
        );
    }
    pub fn owner(&self) -> Person {
        Person::owner(ORG, NOW_SECONDS).unwrap()
    }
    pub async fn records(&self, person: &Person) -> (u16, Value) {
        let body = json!({"organizationId":ORG,"after":null,"limit":100});
        let intent = json!({"v":1,"operation":"ReadChangeRecords","organization":ORG,"target":ORG,"operationId":null,"params":{"after":null,"limit":100}});
        let headers = person
            .headers(
                RECORDS,
                &intent,
                &fixture::fresh_request_id(NOW_SECONDS).unwrap(),
                NOW_SECONDS,
            )
            .unwrap();
        self.post_with(RECORDS, &body, &headers).await
    }
    pub async fn close(self) {
        self.host.close().await;
        self.keys.shutdown().await.unwrap();
    }
}
pub fn org_revision(revision: i64, state: &str) -> Value {
    json!({"kind":"organization","organizationId":ORG,"revision":revision,"state":state,"permissionPolicyDigest":fixture::POLICY})
}
pub fn person_revision(revision: i64, state: &str) -> Value {
    json!({"kind":"person","organizationId":ORG,"personId":fixture::OWNER,"subjectId":fixture::OWNER_SUBJECT,"revision":revision,"state":state})
}
