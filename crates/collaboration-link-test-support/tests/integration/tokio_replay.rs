//! Drive the real WebSocket handler and request log over the Turmoil transport.
use central_link_client::{ClientAction, ClientEvent, ConnectionAttempt, LinkClient};
use central_link_protocol::{
    FrameDirection, LinkFrame, MachineTokenText, ReplyPayload, RequestPayload, decode_text,
    encode_text,
};
use collaboration_central_conformance::{ConformanceResult, signin_fixtures as signed};
use collaboration_central_core::{
    SignInTrustSettings,
    identity_contract::{DeviceLoginDecision, MachineTokenDecision, ProvisionOrganization},
};
use collaboration_central_tokio::TokioWorkspaceHost;
use collaboration_link_test_support::{
    central_request_log::{CENTRAL_HOST, LINK_PORT},
    router_host::{MACHINE_ID, ROUTER_HOST},
    simulated_clock::SimulatedClock,
    tokio_central::serve_tokio_central,
    turmoil_transport::TurmoilLinkTransport,
};
use collaboration_runtime::{LinkInbound, LinkSocket, LinkTransport};
use router_clock::RuntimeClock;
use serde_json::json;
use std::{
    cell::RefCell,
    error::Error,
    path::Path,
    rc::Rc,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use workspace_model::{Caller, RefusalReason, WorkspaceAnswer, WorkspaceResult};

type FixtureResult<T> = Result<T, Box<dyn std::error::Error>>;
type TokenSlot = Rc<RefCell<Option<MachineTokenText>>>;
const WRITE_ID: &str = "019f0000-0000-7000-8000-00000000df01";

async fn provision_host(
    root: &Path,
    clock: Arc<dyn RuntimeClock>,
    keys: &signed::SignInKeys,
) -> ConformanceResult<(Arc<TokioWorkspaceHost>, MachineTokenText)> {
    let host = Arc::new(
        TokioWorkspaceHost::open_with_signin(
            root,
            clock.clone(),
            SignInTrustSettings {
                issuer: signed::ISSUER.into(),
                audience: signed::AUDIENCE.into(),
                admin_audience: signed::ADMIN_AUDIENCE.into(),
                machine_audience: signed::MACHINE_AUDIENCE.into(),
                public_origin: signed::PUBLIC_ORIGIN.into(),
                jwks_url: keys.url.clone(),
                service_id: Some(
                    "019f0000-0000-7000-8000-00000000df03"
                        .to_owned()
                        .try_into()?,
                ),
            },
        )
        .await
        .map_err(|error| format!("open native host at {:?}: {error}", turmoil::elapsed()))?,
    );
    let organization = signed::ORGANIZATION.to_owned().try_into()?;
    let runtime = host.central(&organization).await.map_err(|error| {
        format!(
            "open native organization at {:?}: {error}",
            turmoil::elapsed()
        )
    })?;
    let now = clock.utc_now().timestamp();
    runtime
        .provision_organization(&ProvisionOrganization {
            organization_id: organization,
            organization_statement: signed::stamp(signed::ORGANIZATION, now)?.try_into()?,
        })
        .await
        .map_err(|error| {
            format!(
                "provision organization at {:?}: {error}",
                turmoil::elapsed()
            )
        })?;
    let now = clock.utc_now().timestamp();
    let approval = serde_json::from_value(json!({
        "organizationId": signed::ORGANIZATION, "approver": signed::OWNER,
        "permissionChecked": "machine:enroll", "requestId": "019f0000-0000-7000-8000-00000000df02",
        "installation": MACHINE_ID, "peer": MACHINE_ID, "label": "Turmoil Router",
        "allowedWorkspaces": [], "approverSecondFactorAt": now - 60,
        "codeExpiresAt": now + 600,
    }))?;
    let DeviceLoginDecision::Approved { enrollment_id, .. } = runtime
        .decide_device_login(&approval)
        .await
        .map_err(|error| {
            format!(
                "approve fixture machine at {:?}: {error}",
                turmoil::elapsed()
            )
        })?
    else {
        return Err("fixture could not enroll the Router".into());
    };
    let grant_request = serde_json::from_value(json!({
        "organizationId": signed::ORGANIZATION, "enrollmentId": enrollment_id,
        "clientId": enrollment_id,
    }))?;
    let MachineTokenDecision::Grant {
        grant_expires_at, ..
    } = runtime
        .decide_machine_token(&grant_request)
        .await
        .map_err(|error| format!("grant fixture token at {:?}: {error}", turmoil::elapsed()))?
    else {
        return Err("fixture could not grant a machine token".into());
    };
    let token = signed::machine_token(
        signed::ORGANIZATION,
        MACHINE_ID,
        enrollment_id.as_str(),
        clock.utc_now().timestamp(),
        grant_expires_at.0,
    )?
    .try_into()?;
    Ok((host, token))
}

async fn run_central(
    root: Rc<tempfile::TempDir>,
    tokens: TokenSlot,
    stop: CancellationToken,
    drained: Arc<Notify>,
) -> turmoil::Result {
    let clock: Arc<dyn RuntimeClock> = Arc::new(SimulatedClock::for_host(CENTRAL_HOST));
    let keys = signed::SignInKeys::start()
        .await
        .map_err(simulation_error)?;
    let (host, token) = provision_host(root.path(), clock, &keys)
        .await
        .map_err(simulation_error)?;
    *tokens.borrow_mut() = Some(token);
    let result = serve_tokio_central(host.clone(), LINK_PORT, stop).await;
    host.close().await;
    keys.shutdown().await.map_err(simulation_error)?;
    drained.notify_one();
    result
}

fn simulation_error(error: Box<dyn Error + Send + Sync>) -> Box<dyn Error> {
    error
}

async fn receive(socket: &mut dyn LinkSocket) -> FixtureResult<LinkFrame> {
    let LinkInbound::Text(text) = socket.next_inbound().await else {
        return Err("the native link closed before replying".into());
    };
    Ok(decode_text(&text, FrameDirection::ToRouter)?)
}

async fn write_workspace(
    client: &mut LinkClient,
    attempt: &ConnectionAttempt,
    socket: &mut dyn LinkSocket,
    name: &str,
) -> FixtureResult<WorkspaceResult> {
    let caller = Caller::Person {
        machine_id: MACHINE_ID.parse()?,
        person_id: signed::OWNER.parse()?,
    };
    let payload = RequestPayload::Workspace(serde_json::from_value(json!({
        "requestId": WRITE_ID, "workspaceId": null,
        "operation": {"kind": "createWorkspace", "name": name},
    }))?);
    let request = client.begin_request(caller, payload)?;
    socket
        .send_text(encode_text(&request, FrameDirection::ToCentral)?)
        .await?;
    let ClientEvent::Reply {
        result: ReplyPayload::Workspace(reply),
        ..
    } = client.receive(attempt, receive(socket).await?)?
    else {
        return Err("the native link did not return a workspace reply".into());
    };
    Ok(reply.result)
}

async fn run_router(
    tokens: TokenSlot,
    stop: CancellationToken,
    drained: Arc<Notify>,
) -> turmoil::Result {
    let clock: Arc<dyn RuntimeClock> = Arc::new(SimulatedClock::for_host(ROUTER_HOST));
    let token = loop {
        if let Some(token) = tokens.borrow().clone() {
            break token;
        }
        clock
            .sleep_until(clock.monotonic_now().after(Duration::from_secs(1)))
            .await;
    };
    let transport = TurmoilLinkTransport::new(
        CENTRAL_HOST,
        LINK_PORT,
        format!("/v1/organizations/{}/link", signed::ORGANIZATION),
    );
    let mut socket = transport.connect(&token).await?;
    let mut client = LinkClient::new(MACHINE_ID.parse()?, clock);
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        return Err("the Router did not begin a connection".into());
    };
    if client.receive(&attempt, receive(socket.as_mut()).await?)? != ClientEvent::Welcomed {
        return Err("the native link did not Welcome the Router".into());
    }
    let first =
        write_workspace(&mut client, &attempt, socket.as_mut(), "Turmoil workspace").await?;
    if !matches!(
        first,
        WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { .. })
    ) {
        return Err(format!("workspace creation was not applied: {first:?}").into());
    }
    let replay =
        write_workspace(&mut client, &attempt, socket.as_mut(), "Turmoil workspace").await?;
    if replay != first {
        return Err(format!("real central replay changed the first answer: {replay:?}").into());
    }
    let conflict = write_workspace(&mut client, &attempt, socket.as_mut(), "Changed").await?;
    if conflict != WorkspaceResult::Refused(RefusalReason::RequestIdReused) {
        return Err(format!("real central did not reject changed replay: {conflict:?}").into());
    }
    socket.close().await;
    stop.cancel();
    drained.notified().await;
    Ok(())
}

#[test]
fn authenticated_tokio_route_replays_a_real_write_over_the_simulated_network() -> turmoil::Result {
    let root = Rc::new(tempfile::tempdir()?);
    let tokens = Rc::new(RefCell::new(None));
    let stop = CancellationToken::new();
    let drained = Arc::new(Notify::new());
    let mut sim = turmoil::Builder::new()
        // The sign-in key fixture uses real loopback IO. Link traffic still
        // goes through TurmoilLinkTransport and the simulated listener.
        .enable_tokio_io()
        .rng_seed(8)
        .epoch(SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(signed::NOW_SECONDS)?))
        .tick_duration(Duration::from_millis(5))
        .simulation_duration(Duration::from_secs(12 * 60))
        .build();
    let central_tokens = Rc::clone(&tokens);
    let central_stop = stop.clone();
    let central_drained = Arc::clone(&drained);
    sim.host(CENTRAL_HOST, move || {
        run_central(
            root.clone(),
            central_tokens.clone(),
            central_stop.clone(),
            central_drained.clone(),
        )
    });
    sim.client(ROUTER_HOST, run_router(tokens, stop, drained));
    sim.run()
}
