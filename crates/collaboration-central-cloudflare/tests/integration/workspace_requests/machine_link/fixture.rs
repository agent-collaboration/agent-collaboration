use super::super::workerd_host::{RemoteRuntime, WorkerdHost};
use central_link_client::{ClientAction, ClientEvent, ConnectionAttempt, LinkClient};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, LinkRequestBatch, ReplyPayload, RequestPayload,
};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_core::identity_contract::{
    DecideDeviceLogin, DecideMachineToken, DeviceLoginDecision, MachineTokenDecision,
    ProvisionOrganization, ProvisionOrganizationResult,
};
use fleet_identity_protocol::{EnrollmentId, OrganizationId};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{CloseCode, Message, RequestBuilderExt, WebSocket};
use router_clock::ManualClock;
use runtime_primitives::{Clock, SyncTime};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use std::sync::Arc;
use workspace_model::{Caller, SessionAddress, WorkspaceId, WorkspaceRequest};

pub(super) const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000cf01";
pub(super) const INSTALLATION: &str = "019f0000-0000-7000-8000-00000000cf02";
pub(super) const OTHER_ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000cf03";

pub(super) fn session_caller() -> Result<Caller> {
    Ok(Caller::Session(SessionAddress {
        machine_id: INSTALLATION.parse()?,
        endpoint: "codex-local".to_owned().try_into()?,
        session_id: "019f0000-0000-7000-8000-00000000cf07"
            .to_owned()
            .try_into()?,
    }))
}

pub(super) struct Fixture {
    pub host: WorkerdHost,
    pub clock: Arc<ManualClock>,
    pub organization: OrganizationId,
    pub enrollment: EnrollmentId,
    pub token: String,
    pub client: reqwest::Client,
    keys: signed::SignInKeys,
    _plan_root: tempfile::TempDir,
}

impl Fixture {
    pub async fn new() -> Result<Self> {
        let keys = signed::SignInKeys::start().await?;
        let root = tempfile::Builder::new()
            .prefix("c2-machine-link-")
            .tempdir()?;
        let plan = root.path().join("sign-in-plan.json");
        let access = collaboration_central_conformance::fixture_policy();
        let access_team = access
            .issuer
            .strip_prefix("https://")
            .ok_or("fixture Access issuer")?;
        std::fs::write(
            &plan,
            serde_json::to_vec(&json!({"textBindings":{
            "CENTRAL_SIGN_IN_ISSUER":signed::ISSUER,"CENTRAL_SIGN_IN_AUDIENCE":signed::AUDIENCE,
            "CENTRAL_MACHINE_AUDIENCE":signed::MACHINE_AUDIENCE,
            "CENTRAL_SIGN_IN_JWKS_URL":"https://signin.test/.well-known/jwks.json",
            "CENTRAL_ADMIN_AUDIENCE":signed::ADMIN_AUDIENCE,"CENTRAL_PUBLIC_ORIGIN":signed::PUBLIC_ORIGIN,
            "CENTRAL_SIGN_IN_SERVICE_ID":"019f0000-0000-7000-8000-00000000cf04",
            "ACCESS_TEAM_DOMAIN":access_team,"ACCESS_SESSION_AUD":access.session_audience,
            "ACCESS_ADMIN_AUD":access.admin_audience,"ACCESS_OWNER_IDENTITY":access.owner_identity
        },"jwksFixtureUrl":keys.url}))?,
        )?;
        let clock = Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(
            signed::NOW_SECONDS * 1000,
        ))?);
        let host = WorkerdHost::open_c2(clock.clone(), &plan).await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let organization = ORGANIZATION.to_owned().try_into()?;
        let provision = ProvisionOrganization {
            organization_id: organization,
            organization_statement: signed::stamp(ORGANIZATION, signed::NOW_SECONDS)?.try_into()?,
        };
        let _: ProvisionOrganizationResult = post(
            &host,
            &client,
            "/internal/organization-provisioning",
            &provision,
        )
        .await?;
        let enrollment = approve(
            &host,
            &client,
            "019f0000-0000-7000-8000-00000000cf05",
            signed::NOW_SECONDS,
        )
        .await?;
        let grant = grant(&host, &client, &enrollment).await?;
        let MachineTokenDecision::Grant {
            installation,
            enrollment_id,
            grant_expires_at,
            decided_at,
            ..
        } = grant
        else {
            return Err("real machine token grant refused".into());
        };
        if installation.as_str() != INSTALLATION
            || enrollment_id != enrollment
            || decided_at.0 != signed::NOW_SECONDS
            || grant_expires_at.0 != signed::NOW_SECONDS + 600
        {
            return Err("real machine grant does not match the approved fixture contract".into());
        }
        let token = signed::machine_token(
            ORGANIZATION,
            INSTALLATION,
            enrollment.as_str(),
            signed::NOW_SECONDS,
            grant_expires_at.0,
        )?;
        Ok(Self {
            host,
            clock,
            organization: provision.organization_id,
            enrollment,
            token,
            client,
            keys,
            _plan_root: root,
        })
    }

    pub fn link_url(&self, organization: &str) -> String {
        format!(
            "{}/v1/organizations/{organization}/link",
            self.host.public_origin()
        )
    }

    pub fn runtime(&self) -> Arc<RemoteRuntime> {
        self.host.cold_runtime(&self.organization)
    }

    pub async fn allow_workspace(&self, workspace: &WorkspaceId) -> Result<()> {
        // This is an allowance precondition, not proof of an admin authorization journey.
        let timestamp = chrono::DateTime::from_timestamp_millis(self.clock.now().unix_millis())
            .ok_or("fixture UTC")?;
        let result = self.runtime().run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql:"UPDATE machines SET allowed_workspaces=?,updated_at=? WHERE installation_id=?",
                    args:vec![SqlValue::Text(serde_json::to_string(&vec![workspace])?),SqlValue::Text(timestamp.to_rfc3339()),SqlValue::Text(INSTALLATION.into())],
                }, expect:RowExpectation::Exactly(1),
            }], alarm:AlarmUpdate::Unchanged,
        }).await?;
        if !matches!(result, BatchOutcome::Applied { .. }) {
            return Err("fixture allowance precondition failed".into());
        }
        Ok(())
    }

    pub async fn supersede(&self) -> Result<()> {
        let next = approve(
            &self.host,
            &self.client,
            "019f0000-0000-7000-8000-00000000cf06",
            self.host.now_seconds(),
        )
        .await?;
        match grant(&self.host, &self.client, &next).await? {
            MachineTokenDecision::Grant {
                enrollment_id,
                superseded: Some(ended),
                ..
            } if enrollment_id == next && ended.enrollment_id == self.enrollment => Ok(()),
            _ => Err("real second grant did not supersede the original enrollment".into()),
        }
    }

    pub async fn close(self) -> Result<()> {
        self.host.close().await;
        self.keys.shutdown().await
    }
}

async fn post<Answer: DeserializeOwned>(
    host: &WorkerdHost,
    client: &reqwest::Client,
    path: &str,
    body: &impl Serialize,
) -> Result<Answer> {
    let response = client
        .post(format!("{}{path}", host.signin_origin()))
        .json(body)
        .send()
        .await?;
    if response.status().as_u16() != 200
        || response
            .headers()
            .get("x-collaboration-central-entry")
            .is_none_or(|value| value != "collaboration-central-link-entry")
    {
        return Err("real private sign-in route refused the fixture request".into());
    }
    Ok(response.json().await?)
}

async fn approve(
    host: &WorkerdHost,
    client: &reqwest::Client,
    request_id: &str,
    now: i64,
) -> Result<EnrollmentId> {
    let request: DecideDeviceLogin = serde_json::from_value(json!({
        "organizationId":ORGANIZATION,"approver":signed::OWNER,"permissionChecked":"machine:enroll",
        "requestId":request_id,"installation":INSTALLATION,"peer":INSTALLATION,"label":"Hibernating machine",
        "allowedWorkspaces":[],"approverSecondFactorAt":now-60,"codeExpiresAt":now+600,
    }))?;
    match post(host, client, "/internal/device-login-decisions", &request).await? {
        DeviceLoginDecision::Approved { enrollment_id, .. } => Ok(enrollment_id),
        DeviceLoginDecision::Refused { .. } => Err("real device-login decision refused".into()),
    }
}

async fn grant(
    host: &WorkerdHost,
    client: &reqwest::Client,
    enrollment: &EnrollmentId,
) -> Result<MachineTokenDecision> {
    let request: DecideMachineToken = serde_json::from_value(json!({
        "organizationId":ORGANIZATION,"enrollmentId":enrollment,"clientId":enrollment,
    }))?;
    post(host, client, "/internal/machine-token-decisions", &request).await
}

pub(super) struct ConnectedLink {
    pub socket: WebSocket,
    pub welcome: DataFrame,
    client: LinkClient,
    attempt: ConnectionAttempt,
}

pub(super) struct Exchange {
    pub request: DataFrame,
    pub reply: DataFrame,
}

impl ConnectedLink {
    pub async fn open(fixture: &Fixture) -> Result<Self> {
        let mut client = LinkClient::new(INSTALLATION.parse()?, fixture.clock.clone());
        let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
            return Err("ordinary client did not start a connection".into());
        };
        let mut socket = fixture
            .client
            .get(fixture.link_url(ORGANIZATION))
            .bearer_auth(&fixture.token)
            .upgrade()
            .send()
            .await?
            .into_websocket()
            .await?;
        let welcome = receive(&mut socket).await?;
        if client.receive(&attempt, LinkFrame::Data(welcome.clone()))? != ClientEvent::Welcomed {
            return Err("ordinary client did not accept Welcome".into());
        }
        Ok(Self {
            socket,
            welcome,
            client,
            attempt,
        })
    }

    pub async fn call(&mut self, request: WorkspaceRequest) -> Result<Exchange> {
        self.call_as(session_caller()?, request).await
    }

    pub fn request_as(&mut self, caller: Caller, request: WorkspaceRequest) -> Result<DataFrame> {
        match self
            .client
            .begin_request(caller, RequestPayload::Workspace(request))?
        {
            LinkFrame::Data(frame) => Ok(frame),
            _ => Err("ordinary client produced a non-request frame".into()),
        }
    }

    pub async fn call_as(&mut self, caller: Caller, request: WorkspaceRequest) -> Result<Exchange> {
        let frame = self.request_as(caller, request)?;
        self.exchange(frame).await
    }

    pub async fn exchange(&mut self, request: DataFrame) -> Result<Exchange> {
        send(&mut self.socket, &request).await?;
        let reply = receive(&mut self.socket).await?;
        let ClientEvent::Reply {
            correlation_id,
            result: ReplyPayload::Workspace(_),
        } = self
            .client
            .receive(&self.attempt, LinkFrame::Data(reply.clone()))?
        else {
            return Err("ordinary client did not receive a Workspace reply".into());
        };
        let DataFrame::Request { request: original } = &request else {
            return Err("expected Request".into());
        };
        if &correlation_id != original.correlation_id() {
            return Err("reply correlation changed".into());
        }
        Ok(Exchange { request, reply })
    }

    pub async fn batch(
        &mut self,
        payloads: Vec<RequestPayload>,
    ) -> Result<(DataFrame, Vec<DataFrame>)> {
        let mut members = Vec::new();
        for payload in payloads {
            let LinkFrame::Data(DataFrame::Request { request }) =
                self.client.begin_request(session_caller()?, payload)?
            else {
                return Err("ordinary client produced a non-request batch member".into());
            };
            members.push(*request);
        }
        let request = DataFrame::Batch {
            requests: LinkRequestBatch::new(members)?,
        };
        send(&mut self.socket, &request).await?;
        let DataFrame::Batch { requests } = &request else {
            return Err("expected Batch".into());
        };
        let mut replies = Vec::new();
        for member in requests.requests() {
            let reply = receive(&mut self.socket).await?;
            let ClientEvent::Reply { correlation_id, .. } = self
                .client
                .receive(&self.attempt, LinkFrame::Data(reply.clone()))?
            else {
                return Err("ordinary client did not accept an independent batch Reply".into());
            };
            if &correlation_id != member.correlation_id() {
                return Err("batch Replies changed frame order or correlation".into());
            }
            replies.push(reply);
        }
        Ok((request, replies))
    }

    pub async fn close(self) -> Result<()> {
        self.socket.close(CloseCode::Normal, None).await?;
        Ok(())
    }
}

pub(super) async fn send(socket: &mut WebSocket, frame: &DataFrame) -> Result<()> {
    let text = central_link_protocol::encode_text(
        &LinkFrame::Data(frame.clone()),
        FrameDirection::ToCentral,
    )?;
    socket.send(Message::Text(text)).await?;
    Ok(())
}

async fn receive(socket: &mut WebSocket) -> Result<DataFrame> {
    let message = socket
        .next()
        .await
        .ok_or("machine link ended before Reply")??;
    let Message::Text(text) = message else {
        return Err("machine link returned a non-text frame".into());
    };
    match central_link_protocol::decode_text(&text, FrameDirection::ToRouter)? {
        LinkFrame::Data(frame) => Ok(frame),
        _ => Err("machine link returned an unexpected heartbeat".into()),
    }
}
