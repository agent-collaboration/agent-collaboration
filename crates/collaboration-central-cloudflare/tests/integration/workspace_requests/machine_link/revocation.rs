use super::*;
use serde_json::json;

#[tokio::test]
async fn private_signed_revocation_commits_and_closes_the_idle_machine_link() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = revocation_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn revocation_journey(fixture: &Fixture) -> Result<()> {
    let mut link = ConnectedLink::open(fixture).await?;
    let origin = fixture.host.signin_origin();
    let revocation = signed::RevocationJourney {
        client: &fixture.client,
        origin: &origin,
        organization: ORGANIZATION,
        now: signed::NOW_SECONDS,
    };
    let mut owner = revocation.prepare().await?;
    let enrollment = fixture.enrollment.as_str();
    revocation.check_refusals(&mut owner, enrollment).await?;
    link.socket.send(Message::Text("ping".into())).await?;
    if !matches!(link.socket.next().await, Some(Ok(Message::Text(value))) if value == "pong") {
        return Err("a refused revocation must leave the original link open".into());
    }

    revocation
        .expect(&owner, enrollment, json!({"result":"revoked"}))
        .await?;
    if !matches!(link.socket.next().await, Some(Ok(Message::Close { .. }))) {
        return Err("revocation must close the idle socket before another request".into());
    }
    revocation
        .expect(&owner, enrollment, json!({"result":"alreadyEnded"}))
        .await?;
    let rejected = fixture
        .client
        .get(fixture.link_url(ORGANIZATION))
        .bearer_auth(&fixture.token)
        .upgrade()
        .send()
        .await?;
    if rejected.status().as_u16() != 401 {
        return Err("old bearer must be refused on reconnect".into());
    }

    let readback = fixture.runtime().read(Statement {
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
async fn signed_revocation_closes_the_worker_link_with_an_unread_reply() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = revoke_with_unread_reply(&fixture).await;
    fixture.close().await?;
    result
}

async fn revoke_with_unread_reply(fixture: &Fixture) -> Result<()> {
    use central_link_protocol::{LinkRequest, RequestPayload};
    use workspace_model::{WorkspaceOperation, WorkspaceRequest};

    let mut link = ConnectedLink::open(fixture).await?;
    let origin = fixture.host.signin_origin();
    let revocation = signed::RevocationJourney {
        client: &fixture.client,
        origin: &origin,
        organization: ORGANIZATION,
        now: signed::NOW_SECONDS,
    };
    let owner = revocation.prepare().await?;
    let request_id: workspace_model::RequestId = "019f0000-0000-7000-8000-00000000e311".parse()?;
    let request = LinkRequest::new(
        "019f0000-0000-7000-8000-00000000e310".parse()?,
        super::fixture::session_caller()?,
        RequestPayload::Workspace(WorkspaceRequest {
            request_id: Some(request_id.clone()),
            workspace_id: None,
            operation: WorkspaceOperation::CreateWorkspace {
                name: "Unread reply".to_owned().try_into()?,
            },
        }),
    )?;
    super::fixture::send(
        &mut link.socket,
        &DataFrame::Request {
            request: Box::new(request),
        },
    )
    .await?;
    let mut applied = false;
    for _ in 0..24 {
        let read = fixture
            .runtime()
            .read(Statement {
                sql: "SELECT request_id FROM runtime_request_log WHERE request_id=?",
                args: vec![SqlValue::Text(request_id.to_string())],
            })
            .await?;
        if !read.rows.is_empty() {
            applied = true;
            break;
        }
    }
    if !applied {
        return Err("request never committed while leaving its reply unread".into());
    }
    revocation
        .expect(
            &owner,
            fixture.enrollment.as_str(),
            json!({"result":"revoked"}),
        )
        .await?;
    let Some(Ok(Message::Text(_))) = link.socket.next().await else {
        return Err("the socket did not retain its pre-revocation reply".into());
    };
    if !matches!(link.socket.next().await, Some(Ok(Message::Close { .. }))) {
        return Err("workerd did not close the socket behind the unread reply".into());
    }
    Ok(())
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

    let runtime = fixture.runtime();
    let result = runtime.run_batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "UPDATE login_machine_approvals SET machine_client_id=? WHERE approval_id=?",
                args: vec![
                    SqlValue::Text("019f0000-0000-7000-8000-00000000e600".to_owned()),
                    SqlValue::Text(fixture.enrollment.as_str().to_owned()),
                ],
            },
            expect: RowExpectation::Exactly(1),
        }],
        alarm: AlarmUpdate::Unchanged,
    }).await?;
    if !matches!(result, BatchOutcome::Applied { .. }) {
        return Err("fixture must change exactly one stored client ID".into());
    }
    let client = &fixture.client;
    let response = client
        .get(fixture.link_url(ORGANIZATION))
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
