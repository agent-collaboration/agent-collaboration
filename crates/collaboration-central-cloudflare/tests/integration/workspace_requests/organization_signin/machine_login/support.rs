//! Real private decision routes; workspace rows below are labelled preconditions.

pub(super) type RevokeResult = std::result::Result<
    Option<collaboration_central_core::identity_contract::EndedMachineClient>,
    central_storage::StoreFailure,
>;
pub(super) fn revoke_control(
    serial: u16,
    installation: &str,
    enrollment: &str,
) -> collaboration_central_core::MachineLoginControl {
    collaboration_central_core::MachineLoginControl::Revoke {
        person_id: signed::OWNER.to_owned().try_into().unwrap(),
        operation_id: id(serial).try_into().unwrap(),
        installation: installation.to_owned().try_into().unwrap(),
        enrollment_id: enrollment.to_owned().try_into().unwrap(),
    }
}

impl MachineFixture {
    pub async fn revoke(
        &self,
        serial: u16,
        installation: &str,
        enrollment: &str,
    ) -> Result<RevokeResult> {
        let runtime = self.inner.runtime().await;
        // The Worker handler serializes the dispatcher's Result. Transport failure
        // and an atomic store refusal stay distinct, especially in rollback cases.
        Ok(runtime
            .control::<RevokeResult>(
                "machine-login-revoke",
                &revoke_control(serial, installation, enrollment),
            )
            .await?)
    }
}
use central_storage::SqlValue;
pub(super) use collaboration_central_conformance::{
    ConformanceResult as Result, signin_fixtures as signed,
};
pub(super) use serde_json::{Value, json};
pub(super) const ORG: &str = signed::ORGANIZATION;
pub(super) const OTHER_ORG: &str = "019f0000-0000-7000-8000-00000000bc02";
pub(super) const WORKSPACE: &str = "019f0000-0000-7000-8000-00000000bc03";
pub(super) const WORKSPACE_TWO: &str = "019f0000-0000-7000-8000-00000000bc04";
pub(super) const INSTALLATION: &str = "019f0000-0000-7000-8000-00000000bc05";
pub(super) const INSTALLATION_TWO: &str = "019f0000-0000-7000-8000-00000000bc06";
pub(super) const DEVICE: &str = "/internal/device-login-decisions";
pub(super) const TOKEN: &str = "/internal/machine-token-decisions";
pub(super) const LIFETIME: i64 = 600;
pub(super) type DecisionReply = (u16, Value);
pub(super) fn id(serial: u16) -> String {
    format!("019f0000-0000-7000-8000-00000000{serial:04x}")
}
pub(super) fn number(value: i64) -> SqlValue {
    SqlValue::Integer(value.try_into().unwrap())
}
pub(super) fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
pub(super) fn approval(
    serial: u16,
    installation: &str,
    label: &str,
    workspaces: &[&str],
    now: i64,
) -> Value {
    json!({"organizationId":ORG,"approver":signed::OWNER,"permissionChecked":"machine:enroll",
 "requestId":id(serial),"installation":installation,"peer":installation,"label":label,
 "allowedWorkspaces":workspaces,"approverSecondFactorAt":now-60,"codeExpiresAt":now+600})
}
pub(super) fn token(enrollment: &str, client: &str) -> Value {
    json!({"organizationId":ORG,"enrollmentId":enrollment,"clientId":client})
}
pub(super) fn enrollment(answer: &Value) -> String {
    assert_eq!(answer.get("result"), Some(&json!("approved")));
    let value = answer.get("enrollmentId").and_then(Value::as_str).unwrap();
    assert!(uuid::Uuid::parse_str(value).is_ok_and(|id| id.get_version_num() == 7));
    value.to_owned()
}
pub(super) fn grant(
    enrollment: &str,
    installation: &str,
    now: i64,
    superseded: Option<(&str, &str)>,
) -> Value {
    let mut answer = json!({"result":"grant","installation":installation,"enrollmentId":enrollment,"decidedAt":now,"grantExpiresAt":now+LIFETIME});
    if let Some((enrollment, client)) = superseded {
        answer.as_object_mut().unwrap().insert(
            "superseded".into(),
            json!({"enrollmentId":enrollment,"clientId":client}),
        );
    }
    answer
}
pub(super) fn refused(reason: &str) -> Value {
    json!({"result":"refuse","reason":{"kind":reason}})
}
pub(super) async fn snapshot(f: &MachineFixture) -> Result<Vec<Vec<Vec<SqlValue>>>> {
    let mut rows = vec![];
    for sql in [
        "SELECT * FROM machines ORDER BY machine_id",
        "SELECT * FROM machine_approvals ORDER BY approval_id",
        "SELECT * FROM organization_change_records ORDER BY id",
        "SELECT * FROM machine_credentials ORDER BY credential_id",
    ] {
        rows.push(f.rows(sql).await?);
    }
    Ok(rows)
}
pub(super) async fn seed_workspaces(f: &MachineFixture) -> Result<()> {
    // Directory presence is only a machine-login precondition. This does not prove
    // setup/CreateWorkspace, and no approval or token result is seeded.
    for (workspace, name) in [
        (WORKSPACE, "Login workspace"),
        (WORKSPACE_TWO, "Second login workspace"),
    ] {
        f.execute("INSERT INTO workspaces(workspace_id,name,state,created_at,updated_at) VALUES (?,?,'active','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![text(workspace),text(name)]).await?;
    }
    Ok(())
}

use super::super::support::Fixture;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, Statement, StatementBatch,
};
use std::time::Duration;
pub(super) struct MachineFixture {
    pub inner: Fixture,
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Real fixture setup and transport controls assert their preconditions while propagating IO failures."
)]
impl MachineFixture {
    pub async fn racing_grants(
        &self,
        first: &Value,
        second: &Value,
    ) -> Result<(DecisionReply, DecisionReply)> {
        let runtime = self.inner.runtime().await;
        assert!(
            runtime
                .control::<bool>("hold-central", &"afterSignInSample")
                .await?
        );
        let origin = self.inner.host.signin_origin();
        let body = first.clone();
        let first = tokio::spawn(async move {
            let response = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .post(format!("{origin}{TOKEN}"))
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(
                response
                    .headers()
                    .get("x-collaboration-central-entry")
                    .unwrap(),
                "collaboration-central-link-entry"
            );
            (
                response.status().as_u16(),
                response.json::<Value>().await.unwrap(),
            )
        });
        assert!(
            runtime
                .control::<bool>("until-held-central", &"afterSignInSample")
                .await?
        );
        let origin = self.inner.host.signin_origin();
        let body = second.clone();
        let mut second = tokio::spawn(async move {
            let response = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .post(format!("{origin}{TOKEN}"))
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(
                response
                    .headers()
                    .get("x-collaboration-central-entry")
                    .unwrap(),
                "collaboration-central-link-entry"
            );
            (
                response.status().as_u16(),
                response.json::<Value>().await.unwrap(),
            )
        });
        let queued = async {
            runtime
                .control::<bool>("until-central-waiting", &1usize)
                .await
        };
        let waiting = tokio::select! {result=queued=>result?,_=&mut second=>false};
        assert!(
            waiting,
            "the second token decision crossed the held org turn"
        );
        self.advance(1_000).await?;
        assert!(
            runtime
                .control::<bool>("release-central", &"afterSignInSample")
                .await?
        );
        Ok((first.await?, second.await?))
    }
    pub async fn held_approval(&self, body: &Value, millis: u64) -> Result<(u16, Value)> {
        let runtime = self.inner.runtime().await;
        assert!(
            runtime
                .control::<bool>("hold-central", &"beforeSignInSample")
                .await?
        );
        let release = async {
            assert!(
                runtime
                    .control::<bool>("until-held-central", &"beforeSignInSample")
                    .await?
            );
            self.advance(millis).await?;
            assert!(
                runtime
                    .control::<bool>("release-central", &"beforeSignInSample")
                    .await?
            );
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (reply, released) = tokio::join!(self.post(DEVICE, body), release);
        released?;
        reply
    }
    pub async fn new() -> Result<Self> {
        let inner = Fixture::new().await;
        assert_eq!(inner.provision().await.0, 200);
        let f = Self { inner };
        seed_workspaces(&f).await?;
        Ok(f)
    }
    pub async fn post(&self, path: &str, body: &Value) -> Result<(u16, Value)> {
        Ok(self.inner.post(path, body).await)
    }
    pub async fn rows(&self, sql: &'static str) -> Result<Vec<Vec<SqlValue>>> {
        Ok(self.inner.read(sql).await.rows)
    }
    pub async fn execute(&self, sql: &'static str, args: Vec<SqlValue>) -> Result<()> {
        let answer = self
            .inner
            .runtime()
            .await
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement { sql, args },
                    expect: RowExpectation::Any,
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await?;
        assert!(matches!(answer, BatchOutcome::Applied { .. }));
        Ok(())
    }
    pub fn now(&self) -> i64 {
        use runtime_primitives::Clock;
        self.inner.clock.now().unix_millis().div_euclid(1000)
    }
    pub async fn advance(&self, millis: u64) -> Result<()> {
        self.inner.clock.advance(Duration::from_millis(millis))?;
        self.inner.runtime().await;
        Ok(())
    }
    pub async fn reopen(&mut self) -> Result<()> {
        let answer: std::result::Result<bool, central_storage::StoreFailure> =
            self.inner.runtime().await.control("crash", &()).await;
        assert!(answer.is_err());
        self.inner.runtime().await;
        Ok(())
    }
    pub async fn other_org(&self) -> Result<()> {
        let org = OTHER_ORG.to_owned().try_into()?;
        self.inner.host.central(&org).await;
        assert_eq!(self.inner.post("/internal/organization-provisioning",&json!({"organizationId":OTHER_ORG,"organizationStatement":signed::stamp(OTHER_ORG,self.now())?})).await.0,200);
        Ok(())
    }
    pub async fn close(self) -> Result<()> {
        self.inner.close().await;
        Ok(())
    }
}
