//! The actual private digest HTTP route repairs a missing warm-owner alarm.
use super::support::Fixture;
use central_storage::{SqlValue, Statement, StoreFailure};
use collaboration_central_conformance::ConformanceResult as TestResult;
use collaboration_central_conformance::signin_fixtures as signed;
use router_clock::RuntimeClock;
use serde_json::{Value, json};
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

const A: &str = super::support::ORG;
fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}

async fn prepare(f: &Fixture) -> TestResult<(m::Timestamp, Value)> {
    let now = f.clock.utc_now();
    let statement = signed::stamp(A, now.timestamp())?;
    let reply = f
        .post(
            super::support::PROVISION,
            &json!({"organizationId":A,"organizationStatement":statement}),
        )
        .await;
    if reply.0 != 200 {
        return Err("signed provisioning failed".into());
    }
    // Existing owning fixture writes establish local machine/credential facts.
    // No timer is planted: the real admitted APIs create the session and wake.
    let caller = f
        .enrolled_session_caller("Alarm HTTP fixture", "private-digest-alarm-recovery")
        .await;
    let m::Caller::Session(target) = caller.caller.clone() else {
        return Err("session caller fixture".into());
    };
    let created = f
        .host
        .request(
            &caller,
            m::WorkspaceRequest {
                request_id: Some(signed::fresh_request_id(now.timestamp())?.try_into()?),
                workspace_id: None,
                operation: m::WorkspaceOperation::CreateWorkspace {
                    name: "Digest recovery".to_owned().try_into()?,
                },
            },
        )
        .await?;
    if !matches!(
        created.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { .. })
    ) {
        return Err("session-registering generic CreateWorkspace failed".into());
    }
    let due = m::Timestamp::from_timestamp((now.timestamp().div_euclid(300) + 2) * 300, 0)
        .ok_or("future wake grid")?;
    let wake = f
        .host
        .request_central(
            &caller,
            CentralMailRequest {
                request_id: signed::fresh_request_id(now.timestamp())?.try_into()?,
                operation: CentralMailOperation::SetWake {
                    target,
                    due_at: due,
                    text: "HTTP recovery future wake".to_owned().try_into()?,
                },
            },
        )
        .await?;
    if !matches!(wake,CentralMailResult::Done(CentralMailAnswer::WakeSet { due_at,.. }) if due_at==due)
    {
        return Err("healthy future wake was not actually admitted".into());
    }
    Ok((
        due,
        json!({"digest":signed::digest(&statement),"expiresAt":now.timestamp()-60+signed::LIFETIME_SECONDS,"signingKeyId":"stamp-a"}),
    ))
}
async fn digest(f: &Fixture) -> TestResult<(u16, Value)> {
    Ok(f.post(super::support::DIGEST, &json!({"organizationId":A}))
        .await)
}
async fn timer_rows(f: &Fixture) -> TestResult<Vec<Vec<SqlValue>>> {
    let runtime = f.runtime().await;
    Ok(runtime.statement_host().read(Statement {
        sql:"SELECT wake_id,target,due_at,text,state,created_at,updated_at FROM wakes ORDER BY wake_id",
        args:vec![],
    }).await?.rows)
}
async fn drop_alarm(f: &Fixture) -> TestResult<()> {
    let runtime = f.runtime().await;
    if !runtime.control::<bool>("raw-drop-alarm", &()).await? {
        return Err("physical alarm drop not acknowledged".into());
    }
    Ok(())
}
async fn alarm(f: &Fixture) -> TestResult<Option<i64>> {
    let runtime = f.runtime().await;
    Ok(runtime
        .control::<Option<i64>>("raw-alarm-time", &())
        .await?)
}
async fn fail_rearm(f: &Fixture) -> TestResult<()> {
    let runtime = f.runtime().await;
    if !runtime
        .control::<bool>("fail-alarm-batch", &StoreFailure::Unavailable)
        .await?
    {
        return Err("rearm fault not acknowledged".into());
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The real private digest HTTP response must preserve exact stamp facts while rearming the same warm owner's healthy wake."
)]
#[tokio::test]
async fn private_http_digest_rearms_missing_alarm_on_the_same_warm_owner() -> TestResult<()> {
    let f = Fixture::new().await;
    let (due, expected) = prepare(&f).await?;

    let before = timer_rows(&f).await?;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].get(4), Some(&text("active")));
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    drop_alarm(&f).await?;
    assert_eq!(
        alarm(&f).await?,
        None,
        "physical deadline must be absent immediately before private HTTP"
    );
    assert_eq!(digest(&f).await?, (200, expected));
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    assert!(
        timer_rows(&f).await? == before,
        "digest recovery must not change the admitted timer row"
    );

    f.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A completion-only alarm fault cannot replace private HTTP digest facts; the next digest on the same owner repairs it."
)]
#[tokio::test]
async fn private_http_digest_preserves_success_when_rearm_fails_then_next_digest_repairs()
-> TestResult<()> {
    let f = Fixture::new().await;
    let (due, expected) = prepare(&f).await?;

    let before = timer_rows(&f).await?;
    drop_alarm(&f).await?;
    assert_eq!(alarm(&f).await?, None);
    fail_rearm(&f).await?;
    assert_eq!(digest(&f).await?, (200, expected.clone()));
    assert_eq!(
        alarm(&f).await?,
        None,
        "one-shot completion rearm fault must leave the physical alarm missing"
    );
    assert!(timer_rows(&f).await? == before);
    assert_eq!(digest(&f).await?, (200, expected));
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    assert!(timer_rows(&f).await? == before);

    f.close().await;
    Ok(())
}
