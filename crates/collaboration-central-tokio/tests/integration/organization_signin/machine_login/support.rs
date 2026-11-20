//! Real private decision routes; workspace rows below are labelled preconditions.
impl MachineFixture {
    pub async fn raw_batch(
        &self,
        statement: central_storage::GuardedStatement,
    ) -> Result<central_storage::BatchOutcome> {
        Ok(self
            .inner
            .runtime(ORG)
            .await?
            .statement_host()
            .run_batch(central_storage::StatementBatch {
                statements: vec![statement],
                alarm: central_storage::AlarmUpdate::Unchanged,
            })
            .await?)
    }
}

pub(super) type RevokeResult = std::result::Result<
    Option<collaboration_central_core::identity_contract::EndedMachineClient>,
    central_storage::StoreFailure,
>;
pub(super) fn revoke_control(
    serial: u16,
    machine_id: &str,
    enrollment: &str,
) -> Result<collaboration_central_core::MachineLoginControl> {
    Ok(collaboration_central_core::MachineLoginControl::Revoke {
        person_id: signed::OWNER.to_owned().try_into()?,
        operation_id: id(serial).try_into()?,
        machine_id: machine_id.to_owned().try_into()?,
        enrollment_id: enrollment.to_owned().try_into()?,
    })
}

impl MachineFixture {
    pub async fn revoke(
        &self,
        serial: u16,
        machine_id: &str,
        enrollment: &str,
    ) -> Result<RevokeResult> {
        let runtime = self.inner.runtime(ORG).await?;
        Ok(collaboration_central_core::machine_login_control(
            runtime.as_ref(),
            revoke_control(serial, machine_id, enrollment)?,
        )
        .await)
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
pub(super) fn number(value: i64) -> Result<SqlValue> {
    Ok(SqlValue::Integer(value.try_into()?))
}
pub(super) fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
pub(super) fn approval(
    serial: u16,
    machine_id: &str,
    label: &str,
    workspaces: &[&str],
    now: i64,
) -> Value {
    json!({"organizationId":ORG,"approver":signed::OWNER,"permissionChecked":"machine:enroll",
 "requestId":id(serial),"machineId":machine_id,"label":label,
 "allowedWorkspaces":workspaces,"approverSecondFactorAt":now-60,"codeExpiresAt":now+600})
}
pub(super) fn token(enrollment: &str, client: &str) -> Value {
    json!({"organizationId":ORG,"enrollmentId":enrollment,"clientId":client})
}
pub(super) fn enrollment(answer: &Value) -> Result<String> {
    if answer.get("result") != Some(&json!("approved")) {
        return Err("fixture reply is not an approved enrollment".into());
    }
    let value = answer
        .get("enrollmentId")
        .and_then(Value::as_str)
        .ok_or("approved fixture reply lacks a string enrollmentId")?;
    if !uuid::Uuid::parse_str(value).is_ok_and(|id| id.get_version_num() == 7) {
        return Err("approved fixture reply has no UUIDv7 enrollmentId".into());
    }
    Ok(value.to_owned())
}
pub(super) fn grant(
    enrollment: &str,
    machine_id: &str,
    now: i64,
    superseded: Option<(&str, &str)>,
) -> Value {
    let mut answer = serde_json::Map::from_iter([
        ("result".to_owned(), json!("grant")),
        ("machineId".to_owned(), json!(machine_id)),
        ("enrollmentId".to_owned(), json!(enrollment)),
        ("decidedAt".to_owned(), json!(now)),
        ("grantExpiresAt".to_owned(), json!(now + LIFETIME)),
    ]);
    if let Some((enrollment, client)) = superseded {
        answer.insert(
            "superseded".to_owned(),
            json!({"enrollmentId":enrollment,"clientId":client}),
        );
    }
    Value::Object(answer)
}
pub(super) fn refused(reason: &str) -> Value {
    json!({"result":"refuse","reason":{"kind":reason}})
}
pub(super) async fn snapshot(f: &MachineFixture) -> Result<Vec<Vec<Vec<SqlValue>>>> {
    let mut rows = vec![];
    for sql in [
        "SELECT * FROM login_machines ORDER BY machine_id",
        "SELECT * FROM login_machine_approvals ORDER BY approval_id",
        "SELECT * FROM signin_change_records ORDER BY id",
        "SELECT name FROM sqlite_schema WHERE type='table' AND name='machine_credentials'",
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
        f.execute("INSERT INTO directory_workspaces(workspace_id,name,state,created_at,updated_at) VALUES (?,?,'active','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![text(workspace),text(name)]).await?;
    }
    Ok(())
}

use super::super::support::Fixture;
use std::time::Duration;
pub(super) struct MachineFixture {
    pub inner: Fixture,
}
impl MachineFixture {
    pub async fn racing_grants(
        &self,
        first: &Value,
        second: &Value,
    ) -> Result<(DecisionReply, DecisionReply)> {
        let runtime = self.inner.runtime(ORG).await?;
        let mut held = runtime
            .pause_points()
            .hold_central(collaboration_central_core::PausePoint::AfterSignInSample);
        let client = self.inner.client();
        let body = serde_json::to_string(first)?;
        let first =
            tokio::spawn(
                async move { super::super::support::post(&client, TOKEN, body, vec![]).await },
            );
        held.until_held().await;
        let client = self.inner.client();
        let body = serde_json::to_string(second)?;
        let mut second =
            tokio::spawn(
                async move { super::super::support::post(&client, TOKEN, body, vec![]).await },
            );
        let queued =
            tokio::select! {()=runtime.until_central_waiting(1)=>true,_=&mut second=>false};
        assert!(
            queued,
            "the second token decision crossed the held org turn"
        );
        self.advance(1_000).await?;
        held.release();
        let first = first.await??;
        let second = second.await??;
        Ok((
            (first.status, first.value()?),
            (second.status, second.value()?),
        ))
    }
    pub async fn held_approval(&self, body: &Value, millis: u64) -> Result<(u16, Value)> {
        let runtime = self.inner.runtime(ORG).await?;
        let mut held = runtime
            .pause_points()
            .hold_central(collaboration_central_core::PausePoint::BeforeSignInSample);
        let client = self.inner.client();
        let body = serde_json::to_string(body)?;
        let pending = tokio::spawn(async move {
            super::super::support::post(&client, DEVICE, body, vec![]).await
        });
        held.until_held().await;
        self.advance(millis).await?;
        held.release();
        let reply = pending.await??;
        let value = if reply.body.is_empty() {
            Value::Null
        } else {
            reply.value()?
        };
        Ok((reply.status, value))
    }
    pub async fn new() -> Result<Self> {
        let inner = Fixture::new().await?;
        let f = Self { inner };
        let compact = signed::stamp(ORG, signed::NOW_SECONDS)?;
        assert_eq!(f.inner.provision(ORG, &compact).await?.status, 200);
        seed_workspaces(&f).await?;
        Ok(f)
    }
    pub async fn post(&self, path: &str, body: &Value) -> Result<(u16, Value)> {
        let reply = self.inner.post(path, body).await?;
        let value = if reply.body.is_empty() {
            Value::Null
        } else {
            reply.value()?
        };
        Ok((reply.status, value))
    }
    pub async fn rows(&self, sql: &'static str) -> Result<Vec<Vec<SqlValue>>> {
        self.inner.rows(ORG, sql, vec![]).await
    }
    pub async fn execute(&self, sql: &'static str, args: Vec<SqlValue>) -> Result<()> {
        self.inner.apply(ORG, sql, args).await
    }
    pub fn now(&self) -> i64 {
        self.inner.clock_time()
    }
    pub async fn advance(&self, millis: u64) -> Result<()> {
        self.inner
            .clock
            .advance(Duration::from_millis(millis))
            .await?;
        Ok(())
    }
    pub async fn reopen(&mut self) -> Result<()> {
        self.inner.reopen().await
    }
    pub async fn other_org(&self) -> Result<()> {
        assert_eq!(
            self.inner
                .provision(OTHER_ORG, &signed::stamp(OTHER_ORG, self.now())?)
                .await?
                .status,
            200
        );
        Ok(())
    }
    pub async fn close(self) -> Result<()> {
        self.inner.close().await
    }
}
