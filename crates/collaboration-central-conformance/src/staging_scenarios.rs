use crate::{
    AccessCallerClass, AccessVerificationOutcome, ConformanceClient, ConformanceResult,
    ControlRequest, ControlResult, HolderSession, flip_signature_byte, holder_fixtures::*,
};
use collaboration_central_core::AccessRefusal;
use collaboration_central_core::*;

/// G8: crash controls reach only the intended fault central. The client's
/// host must be the one the private inventory names, and that central must
/// answer a harmless control; a normal central refuses the control route.
pub async fn guard_fault_target(
    client: &ConformanceClient,
    inventory_host: &str,
) -> ConformanceResult<()> {
    let url = reqwest::Url::parse(&client.base_url)?;
    check(
        url.scheme() == "https" && url.host_str() == Some(inventory_host),
        "fault target is not the inventory's fault central",
    )?;
    check(
        matches!(
            client.control(ControlRequest::PendingAlarms).await?,
            ControlResult::Alarms(_)
        ),
        "fault target did not answer a conformance control",
    )
}

/// The shared session groups that need no clock control, no revocation and
/// no more than two installations, so they also run on a staging fault
/// central: lost answer and resume, takeover crash and replay, and the
/// floor-repair and unproven transfer cases.
pub async fn run_staging_session_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    crate::run_call_edge_conformance(client).await?;
    crate::session_scenarios::lost_piece_and_resume(client).await?;
    crate::session_scenarios::transfer_crash_and_preview(client).await?;
    crate::transfer_scenarios::run_transfer_conformance(client).await
}

/// Captures a real Access-issued session assertion for one installation: the
/// `CF_Authorization` cookie Access sets on a service-token request (the
/// team's strict service-token mode is off). It stays in memory for one proof
/// step; nothing else of the response is kept.
pub async fn capture_machine_assertion(
    client: &ConformanceClient,
    label: &str,
) -> ConformanceResult<String> {
    let holder = HolderFixture::enroll(client, label).await?;
    let response = client
        .with_machine_access(
            client.client.get(format!("{}/v1/session", client.base_url)),
            &holder.token,
        )?
        .send()
        .await?;
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|cookie| {
            cookie
                .strip_prefix("CF_Authorization=")
                .and_then(|rest| rest.split(';').next())
                .map(str::to_owned)
        })
        .filter(|token| token.split('.').count() == 3)
        .ok_or_else(|| "Access set no CF_Authorization cookie for the service token".into())
}

/// A1, A3, A4 on a staging fault central with real Access-issued assertions
/// submitted to the edge's verifier: the owner's admin app token and a
/// captured session app token. Each is accepted unmodified, refused with one
/// signature byte changed, accepted again, and refused on the other route's
/// audience. A second admin app's token, when given, is refused here too.
pub async fn run_staging_verifier_rows(
    client: &ConformanceClient,
    machine_token: String,
    other_app_token: Option<String>,
) -> ConformanceResult<()> {
    use AccessVerificationOutcome::{Accepted, Refused};
    let owner = client
        .staging_access()
        .ok_or("verifier rows need real Access")?
        .owner_app_token
        .clone();
    let admin = "/admin/v1/machines/enroll";
    let session = "/v1/session";
    let mut rows = Vec::new();
    for (class, token, own_path, other_path) in [
        (AccessCallerClass::Owner, owner, admin, session),
        (AccessCallerClass::Machine, machine_token, session, admin),
    ] {
        rows.push((
            format!("A1 {class:?} unmodified"),
            token.clone(),
            own_path,
            Accepted(class),
        ));
        rows.push((
            format!("A3 {class:?} one signature byte"),
            flip_signature_byte(&token)?,
            own_path,
            Refused(AccessRefusal::BadSignature),
        ));
        rows.push((
            format!("A3 {class:?} unmodified again"),
            token.clone(),
            own_path,
            Accepted(class),
        ));
        rows.push((
            format!("A4 {class:?} token on the other route"),
            token,
            other_path,
            Refused(AccessRefusal::WrongAudience),
        ));
    }
    if let Some(other) = other_app_token {
        rows.push((
            "A4 other central's admin token on this admin route".to_owned(),
            other,
            admin,
            Refused(AccessRefusal::WrongAudience),
        ));
    }
    for (label, assertion, path, expected) in rows {
        equal(
            client.verify_access_assertion(assertion, path).await?,
            expected,
        )?;
        println!("STAGING {label}: passed");
    }
    Ok(())
}

/// An authenticated session held open across a redeploy. A deploy closes
/// every WebSocket, so its close is the event that says the old version's
/// objects are gone; a socket that survives the bound is reported, not
/// manufactured into a close.
pub struct DeploySentinel {
    session: HolderSession,
}
impl DeploySentinel {
    pub async fn open(client: &ConformanceClient) -> ConformanceResult<Self> {
        let holder = HolderFixture::enroll(client, "deploy-sentinel").await?;
        let (session, _) = holder.connect(client).await?;
        Ok(Self { session })
    }
    pub async fn await_close(mut self, bound: std::time::Duration) -> ConformanceResult<bool> {
        self.session.await_server_close(bound).await
    }
}

/// The `exp` claim of an assertion, read in memory so a run can wait for it.
/// Nothing else of the token is returned.
pub fn assertion_expiry(assertion: &str) -> ConformanceResult<chrono::DateTime<chrono::Utc>> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let payload = assertion.split('.').nth(1).ok_or("assertion payload")?;
    let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
    let seconds = claims
        .get("exp")
        .and_then(serde_json::Value::as_i64)
        .ok_or("assertion exp")?;
    chrono::DateTime::from_timestamp(seconds, 0).ok_or_else(|| "assertion exp range".into())
}

/// A6 on a staging fault central. `inner` is a real session app token whose
/// app has a short session duration; `outer` is the owner's long-lived token,
/// which authenticates every control request. The inner token is accepted
/// while valid, then the same bytes are submitted after the real clock passes
/// its expiry and must be refused Expired, with the outer token accepted
/// around it.
pub async fn run_staging_expiry_row(
    client: &ConformanceClient,
    inner: String,
    outer: String,
) -> ConformanceResult<()> {
    use AccessVerificationOutcome::{Accepted, Refused};
    let expires = assertion_expiry(&inner)?;
    equal(
        client
            .verify_access_assertion(inner.clone(), "/v1/session")
            .await?,
        Accepted(AccessCallerClass::Machine),
    )?;
    println!(
        "STAGING A6 session token accepted; it expires in {}s",
        (expires - chrono::Utc::now()).num_seconds()
    );
    let wait = (expires - chrono::Utc::now() + chrono::TimeDelta::seconds(5))
        .to_std()
        .unwrap_or_default();
    tokio::time::sleep(wait).await;
    equal(
        client.verify_access_assertion(inner, "/v1/session").await?,
        Refused(AccessRefusal::Expired),
    )?;
    equal(
        client
            .verify_access_assertion(outer, "/admin/v1/machines/enroll")
            .await?,
        Accepted(AccessCallerClass::Owner),
    )?;
    println!("STAGING A6 same token after its expiry refused Expired, live token accepted: passed");
    Ok(())
}

/// S4 on a fault central with the temporary installation: its renewal is
/// held at its own dispatch (the hold names it), a newer connection of the
/// same installation replaces the older one, and the renewal resent on the
/// newer connection is answered there once.
pub async fn run_staging_replacement_row(client: &ConformanceClient) -> ConformanceResult<()> {
    use collaboration_central_core::{ObjectAddress, ReservationSource, WireCall, WireResult};
    use collaboration_replication_protocol::*;
    let temporary = HolderFixture::enroll(client, "temporary").await?;
    let (mut older, _) = temporary.connect(client).await?;
    let project = crate::holder_fixtures::project()?;
    check(
        matches!(
            client
                .call(
                    ObjectAddress::Registry,
                    WireCall::Registry(RegistryCall::ReserveFirstEpoch {
                        project: project.clone(),
                        source: ReservationSource::FleetAcceptance {
                            installation: temporary.installation.clone()
                        },
                    })
                )
                .await?,
            WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::FirstLease { .. }))
        ),
        "temporary installation holds a fresh project",
    )?;
    let attempt = RequestId::new(uuid::Uuid::now_v7().to_string())?;
    let renewal = SessionRequest::Renew {
        attempt: attempt.clone(),
        project,
        epoch: epoch(1)?,
    };
    client
        .control(ControlRequest::HoldNextDispatch {
            installation: temporary.installation.clone(),
        })
        .await?;
    older
        .send(&MachineFrame::Request {
            frame_id: FrameId::new(uuid::Uuid::now_v7().to_string())?,
            body: renewal.clone(),
        })
        .await?;
    check(
        matches!(client.control(ControlRequest::AwaitHeldDispatch {installation:temporary.installation.clone()}).await?,
            ControlResult::HeldDispatch(call) if matches!(call.as_ref(),
                WireCall::Registry(RegistryCall::Renew { attempt: held, installation, .. })
                    if *held == attempt && *installation == temporary.installation)),
        "the held dispatch is the temporary installation's renewal",
    )?;
    let (mut newer, _) = temporary.connect(client).await?;
    equal(
        older.receive().await?,
        CentralFrame::Closed {
            reason: SessionCloseReason::ConnectionReplaced,
        },
    )?;
    client
        .control(ControlRequest::ResumeDispatch {
            installation: temporary.installation.clone(),
        })
        .await?;
    check(
        matches!(newer.request(renewal).await?, SessionAnswer::Renewed { .. }),
        "the in-flight renewal resent on the newer connection is answered there",
    )?;
    newer.close().await?;
    println!(
        "STAGING S4 older connection replaced with its renewal held, resent on the newer connection and answered once: passed"
    );
    Ok(())
}

/// A10 on a normal central, at the end of a run: the owner revokes the
/// temporary installation while its session is open. That session is closed
/// as revoked, a fresh upgrade is refused, and installation A still connects.
pub async fn run_staging_revocation_row(client: &ConformanceClient) -> ConformanceResult<()> {
    use collaboration_central_core::{AdminRequest, AdminResult};
    use collaboration_replication_protocol::*;
    let temporary = HolderFixture::enroll(client, "temporary").await?;
    let a = HolderFixture::enroll(client, "old").await?;
    let (mut session, _) = temporary.connect(client).await?;
    equal(
        client
            .admin(AdminRequest::RevokeMachine {
                installation: temporary.installation.clone(),
            })
            .await?,
        AdminResult::MachineRevoked,
    )?;
    equal(
        session.receive().await?,
        CentralFrame::Closed {
            reason: SessionCloseReason::CredentialRejected(CredentialRejection::Revoked),
        },
    )?;
    check(
        temporary.connect(client).await.is_err(),
        "a revoked installation cannot open a session",
    )?;
    let (unrelated, _) = a.connect(client).await?;
    unrelated.close().await?;
    println!(
        "STAGING A10 live session closed as revoked, fresh upgrade refused, installation A unaffected: passed"
    );
    Ok(())
}
