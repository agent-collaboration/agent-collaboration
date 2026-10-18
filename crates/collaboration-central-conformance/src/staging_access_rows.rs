//! Access rows for a staging normal central: each pairing and refusal at the
//! session and admin routes, observed as status, ray ID and the Worker's own
//! typed refusal when the Worker answered.
use crate::{ConformanceClient, ConformanceResult, holder_fixtures::*};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use reqwest_websocket::RequestBuilderExt;

/// The normal central must be the inventory's host and must refuse the
/// conformance control route: its artifact has none.
pub async fn guard_normal_target(
    client: &ConformanceClient,
    inventory_host: &str,
) -> ConformanceResult<()> {
    let url = reqwest::Url::parse(&client.base_url)?;
    check(
        url.scheme() == "https" && url.host_str() == Some(inventory_host),
        "normal target is not the inventory's normal central",
    )?;
    let response = client
        .with_owner_access(
            client
                .client
                .post(format!("{}/__conformance/control", client.base_url)),
        )?
        .json(&crate::ControlRequest::PendingAlarms)
        .send()
        .await?;
    equal(response.status(), reqwest::StatusCode::UNAUTHORIZED)?;
    equal(
        response.json::<AccessRefusal>().await?,
        AccessRefusal::RouteRefused,
    )
}

/// One probe of the session route: its status, Cloudflare's ray ID for the
/// Worker log, and the Worker's typed refusal when the Worker answered.
/// Access's own pages are never printed.
pub struct ProbeObservation {
    pub status: reqwest::StatusCode,
    pub ray: String,
    pub worker_refusal: Option<serde_json::Value>,
}
impl std::fmt::Display for ProbeObservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "status {} ray {} worker {}",
            self.status.as_u16(),
            self.ray,
            self.worker_refusal
                .as_ref()
                .map_or_else(|| "none".to_owned(), ToString::to_string)
        )
    }
}

struct SessionProbe<'a> {
    access_of: Option<&'a str>,
    bearer: Option<&'a str>,
}
async fn probe_session(
    client: &ConformanceClient,
    probe: SessionProbe<'_>,
) -> ConformanceResult<ProbeObservation> {
    let mut request = client.client.get(format!("{}/v1/session", client.base_url));
    if let Some(credential) = probe.access_of {
        request = client.with_machine_access(request, credential)?;
    }
    if let Some(bearer) = probe.bearer {
        request = request.bearer_auth(bearer);
    }
    let response = request.upgrade().send().await?.into_inner();
    let status = response.status();
    let ray = response
        .headers()
        .get("cf-ray")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    let worker_json = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    let worker_refusal = if worker_json {
        Some(response.json::<serde_json::Value>().await?)
    } else {
        None
    };
    Ok(ProbeObservation {
        status,
        ray,
        worker_refusal,
    })
}

/// A7, A2, A13, A14, A8, A9: each Access pairing and refusal at the session
/// and admin routes. Matching pairs are shown by ordinary connects elsewhere.
pub async fn run_staging_access_rows(client: &ConformanceClient) -> ConformanceResult<()> {
    let a = HolderFixture::enroll(client, "old").await?;
    let b = HolderFixture::enroll(client, "new").await?;
    for (label, probe, expected) in [
        (
            "A7 A's service token with B's credential",
            SessionProbe {
                access_of: Some(&a.token),
                bearer: Some(&b.token),
            },
            Some(CredentialRejection::NotMachine),
        ),
        (
            "A7 B's service token with A's credential",
            SessionProbe {
                access_of: Some(&b.token),
                bearer: Some(&a.token),
            },
            Some(CredentialRejection::NotMachine),
        ),
        (
            "A2 service token with an unknown credential",
            SessionProbe {
                access_of: Some(&a.token),
                bearer: Some("m3-10-unknown-machine-credential"),
            },
            Some(CredentialRejection::Unknown),
        ),
        (
            "A2 credential with no service token",
            SessionProbe {
                access_of: None,
                bearer: Some(&a.token),
            },
            None,
        ),
        (
            "A14 no credentials at all",
            SessionProbe {
                access_of: None,
                bearer: None,
            },
            None,
        ),
    ] {
        let observed = probe_session(client, probe).await?;
        match expected {
            Some(rejection) => {
                equal(observed.status, reqwest::StatusCode::UNAUTHORIZED)?;
                equal(
                    observed.worker_refusal.clone(),
                    Some(serde_json::to_value(rejection)?),
                )?;
            }
            None => check(
                observed.worker_refusal.is_none() && !observed.status.is_success(),
                "Access must refuse before the Worker answers",
            )?,
        }
        println!("STAGING {label}: {observed}");
    }
    let marker = format!("m310-forged-{}", uuid::Uuid::now_v7());
    let mut session = client
        .open_session(
            client
                .with_machine_access(
                    client
                        .client
                        .get(format!("{}/v1/session", client.base_url))
                        .bearer_auth(&a.token),
                    &a.token,
                )?
                .header("Cf-Access-Jwt-Assertion", forged_assertion(&marker)?),
        )
        .await?;
    session
        .send(&MachineFrame::Hello {
            installation: a.installation.clone(),
            session_id: a.session_id.clone(),
            schema_support: SchemaSupport { max_version: 10 },
        })
        .await?;
    check(
        matches!(session.receive().await?, CentralFrame::Welcome { .. }),
        "a client-supplied assertion header must not block the real one",
    )?;
    session.close().await?;
    println!("STAGING A13 forged header with marker kid {marker}: Welcome");
    let project = ProjectId::new(uuid::Uuid::now_v7().to_string())?;
    let machine_admin = client
        .with_machine_access(
            client
                .client
                .post(format!(
                    "{}/admin/v1/projects/{}/takeover/preview",
                    client.base_url,
                    project.as_str()
                ))
                .bearer_auth(&a.token),
            &a.token,
        )?
        .send()
        .await?;
    check(
        !machine_admin.status().is_success(),
        "a machine must not reach the admin route",
    )?;
    println!(
        "STAGING A8 machine on admin route: status {}",
        machine_admin.status().as_u16()
    );
    let owner_session = client
        .with_owner_access(client.client.get(format!("{}/v1/session", client.base_url)))?
        .upgrade()
        .send()
        .await?
        .into_inner();
    check(
        !owner_session.status().is_success()
            && owner_session.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS,
        "the owner's sign-in must not open a session",
    )?;
    println!(
        "STAGING A9 owner sign-in on session route: status {}",
        owner_session.status().as_u16()
    );
    Ok(())
}

/// A syntactically valid assertion whose key ID and claims carry `marker`, so
/// the Worker's log shows whether it ever saw this header instead of Access's.
fn forged_assertion(marker: &str) -> ConformanceResult<String> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(
        &serde_json::json!({"alg":"RS256","kid":marker,"typ":"JWT"}),
    )?);
    let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(
        &serde_json::json!({"iss":marker,"aud":[marker],"exp":0,"nbf":0,"type":"app","sub":"","common_name":marker}),
    )?);
    Ok(format!(
        "{header}.{claims}.{}",
        URL_SAFE_NO_PAD.encode(marker)
    ))
}
