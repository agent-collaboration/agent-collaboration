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
        sql: "SELECT m.state AS machine_state,m.allowed_workspaces,m.enrollment_generation,a.state,a.end_reason,(SELECT COUNT(*) FROM signin_change_records WHERE kind='machineRevoked' AND target_id=a.approval_id AND actor_person_id=? AND permission_checked='machine:enroll') AS audit_records FROM login_machine_approvals a JOIN login_machines m USING(machine_id) WHERE a.approval_id=?",
        args: vec![SqlValue::Text(owner.id), SqlValue::Text(enrollment.to_owned())],
    }).await?;
    if readback.rows
        != vec![vec![
            SqlValue::Text("revoked".into()),
            SqlValue::Text("[]".into()),
            SqlValue::Integer(2.try_into()?),
            SqlValue::Text("ended".into()),
            SqlValue::Text("owner".into()),
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
        DecideDeviceLogin, MachinePrincipal, MachineTokenDecision, PresentedMachine,
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
    )
}
