use super::*;
use central_storage::{SqlValue, Statement};

#[tokio::test]
async fn private_signed_revocation_commits_and_closes_the_idle_machine_link() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = revocation_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn revocation_journey(fixture: &Fixture) -> Result<()> {
    let mut socket = fixture.connect().await?;
    if !matches!(
        receive(&mut socket).await?,
        LinkFrame::Data(DataFrame::Welcome { .. })
    ) {
        return Err("machine did not receive Welcome".into());
    }
    let private = reqwest::Client::builder()
        .no_proxy()
        .unix_socket(fixture.signin_socket.clone())
        .build()?;
    let revocation = signed::RevocationJourney {
        client: &private,
        origin: "http://localhost",
        organization: signed::ORGANIZATION,
        now: signed::NOW_SECONDS,
    };
    let mut owner = revocation.prepare().await?;
    let enrollment = fixture.enrollment_id.as_str();
    revocation.check_refusals(&mut owner, enrollment).await?;
    send(&mut socket, &LinkFrame::Ping).await?;
    if receive(&mut socket).await? != LinkFrame::Pong {
        return Err("a refused revocation must leave the original link open".into());
    }

    revocation
        .expect(&owner, enrollment, json!({"result":"revoked"}))
        .await?;
    if !matches!(socket.next().await, Some(Ok(Message::Close { .. }))) {
        return Err("revocation must close the idle socket before another request".into());
    }
    revocation
        .expect(&owner, enrollment, json!({"result":"alreadyEnded"}))
        .await?;
    let rejected = fixture
        .client()?
        .get(fixture.url(signed::ORGANIZATION))
        .bearer_auth(&fixture.token)
        .upgrade()
        .send()
        .await?;
    if rejected.status().as_u16() != 401 {
        return Err("old bearer must be refused on reconnect".into());
    }

    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let readback = runtime.statement_host().read(Statement {
        sql: "SELECT m.state AS machine_state,m.allowed_workspaces,m.enrollment_generation,a.state,a.end_reason,a.permission_checked,(SELECT COUNT(*) FROM signin_change_records WHERE kind='machineRevoked' AND target_id=a.approval_id AND actor_person_id=? AND permission_checked='machine:revoke') AS audit_records FROM login_machine_approvals a JOIN login_machines m USING(machine_id) WHERE a.approval_id=?",
        args: vec![SqlValue::Text(owner.id), SqlValue::Text(enrollment.to_owned())],
    }).await?;
    if readback.rows
        != vec![vec![
            SqlValue::Text("revoked".into()),
            SqlValue::Text("[]".into()),
            SqlValue::Integer(2.try_into()?),
            SqlValue::Text("ended".into()),
            SqlValue::Text("owner".into()),
            SqlValue::Text("machine:enroll".into()),
            SqlValue::Integer(1.try_into()?),
        ]]
    {
        return Err("revocation must commit once with the verified actor and permission".into());
    }
    Ok(())
}

#[test]
fn issuer_one_machine_id_vectors_are_closed_on_this_host() -> Result<()> {
    use collaboration_central_core::identity_contract::{
        AdmitMachineConnection, AdmittedMachine, DecideDeviceLogin, MachineConnectionAdmission,
        MachinePrincipal, MachineTokenDecision, PresentedMachine,
    };

    fn round_trip<Wire: serde::de::DeserializeOwned + serde::Serialize>(
        value: serde_json::Value,
    ) -> Option<serde_json::Value> {
        serde_json::from_value::<Wire>(value)
            .ok()
            .and_then(|decoded| serde_json::to_value(decoded).ok())
    }

    signed::check_machine_identity_vectors(
        round_trip::<DecideDeviceLogin>,
        round_trip::<PresentedMachine>,
        round_trip::<MachineTokenDecision>,
        round_trip::<MachinePrincipal>,
    )?;
    signed::check_admission_identity_vectors(
        round_trip::<AdmitMachineConnection>,
        round_trip::<AdmittedMachine>,
        round_trip::<MachineConnectionAdmission>,
    )
}

#[tokio::test]
async fn valid_uuid_for_the_wrong_stored_client_is_corrupt() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = mismatched_stored_client(&fixture).await;
    fixture.close().await?;
    result
}

async fn mismatched_stored_client(fixture: &Fixture) -> Result<()> {
    use central_storage::{
        AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
    };
    use reqwest_websocket::RequestBuilderExt;

    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let result = runtime.statement_host().run_batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "UPDATE login_machine_approvals SET machine_client_id=? WHERE approval_id=?",
                args: vec![
                    SqlValue::Text("019f0000-0000-7000-8000-00000000e600".to_owned()),
                    SqlValue::Text(fixture.enrollment_id.as_str().to_owned()),
                ],
            },
            expect: RowExpectation::Exactly(1),
        }],
        alarm: AlarmUpdate::Unchanged,
    }).await?;
    if !matches!(result, BatchOutcome::Applied { .. }) {
        return Err("fixture must change exactly one stored client ID".into());
    }
    let client = &fixture.client()?;
    let response = client
        .get(fixture.url(signed::ORGANIZATION))
        .bearer_auth(&fixture.token)
        .upgrade()
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::SERVICE_UNAVAILABLE {
        return Err(format!(
            "expected corrupt storage to return 503, got {}",
            response.status()
        )
        .into());
    }
    Ok(())
}
