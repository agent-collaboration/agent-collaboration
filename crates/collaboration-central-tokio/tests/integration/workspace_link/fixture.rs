use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use central_link_protocol::{FrameDirection, LinkFrame, decode_text, encode_text};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_core::{
    SignInTrustSettings,
    identity_contract::{DeviceLoginDecision, MachineTokenDecision, ProvisionOrganization},
};
use collaboration_central_tokio::{TokioWorkspaceHost, WorkspaceLink};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use router_clock::ManualClock;
use runtime_primitives::SyncTime;
use serde_json::json;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(super) const INSTALLATION: &str = "019f0000-0000-7000-8000-00000000ce01";
pub(super) const OTHER_MACHINE: &str = "019f0000-0000-7000-8000-00000000ce02";
pub(super) const OTHER_ORG: &str = "019f0000-0000-7000-8000-00000000ce03";
const AUDIENCE: &str = "https://central.example.invalid/machine";

pub(super) struct Fixture {
    pub host: Arc<TokioWorkspaceHost>,
    pub clock: Arc<ManualClock>,
    pub token: String,
    address: std::net::SocketAddr,
    link: WorkspaceLink,
    stop: CancellationToken,
    serving: JoinHandle<std::io::Result<()>>,
    keys: signed::SignInKeys,
    _root: tempfile::TempDir,
}

impl Fixture {
    pub async fn new() -> Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("workspace-link-")
            .tempdir()?;
        let clock = Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(
            signed::NOW_SECONDS * 1000,
        ))?);
        let keys = signed::SignInKeys::start().await?;
        let host = Arc::new(
            TokioWorkspaceHost::open_with_signin(
                &root.path().join("modern"),
                clock.clone(),
                SignInTrustSettings {
                    issuer: signed::ISSUER.into(),
                    audience: signed::AUDIENCE.into(),
                    admin_audience: signed::ADMIN_AUDIENCE.into(),
                    machine_audience: AUDIENCE.into(),
                    public_origin: signed::PUBLIC_ORIGIN.into(),
                    jwks_url: keys.url.clone(),
                    service_id: Some(
                        "019f0000-0000-7000-8000-00000000ce05"
                            .to_owned()
                            .try_into()?,
                    ),
                },
            )
            .await?,
        );
        let organization = signed::ORGANIZATION.to_owned().try_into()?;
        let runtime = host.central(&organization).await?;
        runtime
            .provision_organization(&ProvisionOrganization {
                organization_id: organization,
                organization_statement: signed::stamp(signed::ORGANIZATION, signed::NOW_SECONDS)?
                    .try_into()?,
            })
            .await?;
        let approval = serde_json::from_value(json!({
            "organizationId": signed::ORGANIZATION, "approver": signed::OWNER,
            "permissionChecked":"machine:enroll", "requestId":"019f0000-0000-7000-8000-00000000ce04",
            "installation":INSTALLATION, "peer":INSTALLATION, "label":"Link test machine",
            "allowedWorkspaces":[], "approverSecondFactorAt":signed::NOW_SECONDS-60,
            "codeExpiresAt":signed::NOW_SECONDS+600,
        }))?;
        let DeviceLoginDecision::Approved { enrollment_id, .. } =
            runtime.decide_device_login(&approval).await?
        else {
            return Err("real machine approval refused".into());
        };
        let grant = runtime.decide_machine_token(&serde_json::from_value(json!({
            "organizationId":signed::ORGANIZATION,"enrollmentId":enrollment_id,"clientId":enrollment_id,
        }))?).await?;
        let MachineTokenDecision::Grant {
            grant_expires_at, ..
        } = grant
        else {
            return Err("real machine token grant refused".into());
        };
        let token = signed::sign(
            &json!({"alg":"EdDSA","typ":"at+jwt","kid":"stamp-a"}),
            &json!({"iss":signed::ISSUER,"aud":AUDIENCE,"iat":signed::NOW_SECONDS,
                "exp":signed::NOW_SECONDS+3600,"grant_expires_at":grant_expires_at.0,
                "organization_id":signed::ORGANIZATION,"client_id":enrollment_id,"azp":enrollment_id,
                "principal":{"kind":"machine","installation_id":INSTALLATION,"enrollment_id":enrollment_id}}),
            7,
        )?;
        let listener =
            tokio::net::TcpListener::bind(std::net::SocketAddr::from(([127, 0, 0, 1], 0))).await?;
        let address = listener.local_addr()?;
        let link = WorkspaceLink::new(host.clone());
        let router = link.router();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let serving = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(stopped.cancelled_owned())
                .await
        });
        Ok(Self {
            host,
            clock,
            token,
            address,
            link,
            stop,
            serving,
            keys,
            _root: root,
        })
    }

    pub fn keys(&self) -> &signed::SignInKeys {
        &self.keys
    }

    pub fn root_path(&self) -> &std::path::Path {
        self._root.path()
    }

    pub fn url(&self, organization: &str) -> String {
        format!(
            "http://{}/v1/organizations/{organization}/link",
            self.address
        )
    }

    pub fn client(&self) -> Result<reqwest::Client> {
        Ok(reqwest::Client::builder().no_proxy().build()?)
    }

    pub fn token_for_installation(&self, machine_id: &str) -> Result<String> {
        let encoded = self.token.split('.').nth(1).ok_or("fixture token claims")?;
        let mut claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded)?)?;
        signed::set_claim(
            claims
                .get_mut("principal")
                .ok_or("fixture machine principal")?,
            "installation_id",
            json!(machine_id),
        )?;
        signed::sign(
            &json!({"alg":"EdDSA","typ":"at+jwt","kid":"stamp-a"}),
            &claims,
            7,
        )
    }

    pub async fn connect(&self) -> Result<WebSocket> {
        Ok(self
            .client()?
            .get(self.url(signed::ORGANIZATION))
            .bearer_auth(&self.token)
            .upgrade()
            .send()
            .await?
            .into_websocket()
            .await?)
    }

    pub async fn close(self) -> Result<()> {
        self.link.begin_shutdown();
        self.stop.cancel();
        self.serving.await??;
        self.link.shutdown().await;
        self.host.close().await;
        self.keys.shutdown().await
    }
}

pub(super) async fn receive(socket: &mut WebSocket) -> Result<LinkFrame> {
    let message = socket.next().await.ok_or("link ended before a frame")??;
    let Message::Text(text) = message else {
        return Err("expected a link text frame".into());
    };
    Ok(decode_text(&text, FrameDirection::ToRouter)?)
}

pub(super) async fn send(socket: &mut WebSocket, frame: &LinkFrame) -> Result<()> {
    socket
        .send(Message::Text(encode_text(
            frame,
            FrameDirection::ToCentral,
        )?))
        .await?;
    Ok(())
}
