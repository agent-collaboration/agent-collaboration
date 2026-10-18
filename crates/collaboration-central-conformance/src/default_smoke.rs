use crate::{ConformanceClient, ConformanceResult};
use collaboration_central_core::{AdminRequest, AdminResult};
use collaboration_replication_protocol::*;
use sha2::{Digest, Sha256};
pub async fn run_default_smoke(client: &ConformanceClient) -> ConformanceResult<()> {
    let installation = InstallationId::new(uuid::Uuid::now_v7().to_string())?;
    let machine = "native-smoke-synthetic-machine";
    let result = client
        .admin(AdminRequest::EnrollMachine {
            label: "test machine".into(),
            installation: installation.clone(),
            access_client_id: crate::access_client_id(machine)?,
            credential_hash: format!("{:x}", Sha256::digest(machine.as_bytes())),
            expires_at: chrono::Utc::now() + chrono::TimeDelta::hours(1),
        })
        .await?;
    if result != AdminResult::MachineEnrolled {
        return Err("enrollment failed".into());
    }
    if client.connect_owner().await.is_ok() {
        return Err("admin admitted to machine route".into());
    }
    let mut socket = client.connect(machine).await?;
    let session_id = SessionId::new(uuid::Uuid::now_v7().to_string())?;
    socket
        .send(&MachineFrame::Hello {
            installation,
            session_id: session_id.clone(),
            schema_support: SchemaSupport { max_version: 10 },
        })
        .await?;
    match socket.receive().await? {
        CentralFrame::Welcome {
            session_id: actual,
            projects,
        } if actual == session_id && projects.is_empty() => {}
        _ => return Err("unexpected Welcome".into()),
    }
    let answer = socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: ProjectId::new(uuid::Uuid::now_v7().to_string())?,
            },
            after: None,
        })
        .await?;
    if answer != SessionAnswer::Refused(SessionRefusal::NotInitialized) {
        return Err("copy read refusal".into());
    }
    socket.close().await?;
    let response = client
        .client
        .post(format!("{}/__conformance/control", client.base_url))
        .header("Cf-Access-Jwt-Assertion", crate::owner_assertion()?)
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::UNAUTHORIZED
        || response
            .json::<collaboration_central_core::AccessRefusal>()
            .await?
            != collaboration_central_core::AccessRefusal::RouteRefused
    {
        return Err("test control route must be absent from a default build".into());
    }
    println!(
        "DEFAULT smoke: enrollment, auth on upgrade, binary Hello/Welcome, correlated read refusal, admin denied on session, test controls absent: passed"
    );
    Ok(())
}
