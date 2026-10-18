use crate::{ControlRequest, ControlResult};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
pub type ConformanceResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;
/// How requests satisfy the Access gate. Local runtimes verify assertions the
/// fixture key signs; a staging central sits behind real Cloudflare Access.
#[derive(Clone)]
enum AccessCredentials {
    Fixture,
    Staging(std::sync::Arc<crate::StagingAccess>),
}
#[derive(Clone)]
pub struct ConformanceClient {
    pub(crate) client: reqwest::Client,
    pub(crate) base_url: String,
    access: AccessCredentials,
}
impl ConformanceClient {
    pub fn new(base_url: &str) -> ConformanceResult<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .http1_only()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            base_url: base_url.into(),
            access: AccessCredentials::Fixture,
        })
    }
    /// A client for a deployed central behind real Access. Redirects are never
    /// followed, so an Access login redirect is observed rather than chased.
    pub fn staging(
        base_url: &str,
        access: std::sync::Arc<crate::StagingAccess>,
    ) -> ConformanceResult<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .http1_only()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url: base_url.into(),
            access: AccessCredentials::Staging(access),
        })
    }
    #[must_use]
    pub fn staging_access(&self) -> Option<&crate::StagingAccess> {
        match &self.access {
            AccessCredentials::Fixture => None,
            AccessCredentials::Staging(access) => Some(access),
        }
    }
    fn as_owner(
        &self,
        request: reqwest::RequestBuilder,
    ) -> ConformanceResult<reqwest::RequestBuilder> {
        Ok(match &self.access {
            AccessCredentials::Fixture => {
                request.header("Cf-Access-Jwt-Assertion", crate::owner_assertion()?)
            }
            AccessCredentials::Staging(access) => {
                request.header("cf-access-token", &access.owner_app_token)
            }
        })
    }
    fn as_machine(
        &self,
        request: reqwest::RequestBuilder,
        machine_credential: &str,
    ) -> ConformanceResult<reqwest::RequestBuilder> {
        self.with_machine_access(request.bearer_auth(machine_credential), machine_credential)
    }
    /// Adds only the Access half of a machine's session credentials: the
    /// installation's assertion locally, its service token on staging. The
    /// bearer machine credential is the caller's to add.
    pub(crate) fn with_machine_access(
        &self,
        request: reqwest::RequestBuilder,
        machine_credential: &str,
    ) -> ConformanceResult<reqwest::RequestBuilder> {
        Ok(match &self.access {
            AccessCredentials::Fixture => request.header(
                "Cf-Access-Jwt-Assertion",
                crate::machine_assertion(machine_credential)?,
            ),
            AccessCredentials::Staging(access) => {
                let installation = access
                    .for_credential(machine_credential)
                    .ok_or("no staging installation holds this machine credential")?;
                request
                    .header(
                        "CF-Access-Client-Id",
                        installation.access_client_id.as_str(),
                    )
                    .header(
                        "CF-Access-Client-Secret",
                        &installation.access_client_secret,
                    )
            }
        })
    }
    pub(crate) fn with_owner_access(
        &self,
        request: reqwest::RequestBuilder,
    ) -> ConformanceResult<reqwest::RequestBuilder> {
        self.as_owner(request)
    }
    pub async fn control(&self, request: ControlRequest) -> ConformanceResult<ControlResult> {
        Ok(self
            .as_owner(
                self.client
                    .post(format!("{}/__conformance/control", self.base_url)),
            )?
            .json(&request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    pub async fn call(
        &self,
        address: ObjectAddress,
        call: WireCall,
    ) -> ConformanceResult<WireResult> {
        match self
            .control(ControlRequest::Call {
                address,
                call: Box::new(call),
            })
            .await?
        {
            ControlResult::Call(result) => Ok(*result),
            _ => Err("control Call response".into()),
        }
    }
    pub async fn admin(&self, request: AdminRequest) -> ConformanceResult<AdminResult> {
        if let AdminRequest::ImportProject { project, upload } = &request {
            return self
                .import_project(project, upload)
                .await
                .map(AdminResult::ProjectImport);
        }
        let path = match &request {
            AdminRequest::ImportProject { .. } => return Err("import route handled above".into()),
            AdminRequest::EnrollMachine { .. } => "/admin/v1/machines/enroll".into(),
            AdminRequest::RevokeMachine { installation } => {
                format!("/admin/v1/machines/{}/revoke", installation.as_str())
            }
            AdminRequest::TakeoverPreview { project } => {
                format!("/admin/v1/projects/{}/takeover/preview", project.as_str())
            }
            AdminRequest::Move { project, .. } => {
                format!("/admin/v1/projects/{}/move", project.as_str())
            }
            AdminRequest::Takeover { project, .. } => {
                format!("/admin/v1/projects/{}/takeover", project.as_str())
            }
        };
        Ok(self
            .as_owner(self.client.post(format!("{}{path}", self.base_url)))?
            .json(&request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    pub async fn import_project(
        &self,
        project: &ProjectId,
        upload: &ProjectImportPage,
    ) -> ConformanceResult<ProjectImportAnswer> {
        Ok(self
            .as_owner(self.client.post(format!(
                "{}/admin/v1/import/projects/{}",
                self.base_url,
                project.as_str()
            )))?
            .json(upload)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    pub async fn import_fleet(
        &self,
        bundle: &fleet_admission::FleetImportBundle,
    ) -> ConformanceResult<fleet_admission::FleetImportResult> {
        let response = self
            .as_owner(
                self.client
                    .post(format!("{}/admin/v1/import/fleet", self.base_url)),
            )?
            .json(bundle)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let refusal = response.json::<AccessRefusal>().await.ok();
            return Err(format!("fleet import owner route {status}: {refusal:?}").into());
        }
        Ok(response.json().await?)
    }
    /// Runs the edge's verifier on one assertion through the conformance
    /// control; the answer carries the caller class or refusal only.
    pub async fn verify_access_assertion(
        &self,
        assertion: String,
        path: &str,
    ) -> ConformanceResult<crate::AccessVerificationOutcome> {
        match self
            .control(ControlRequest::VerifyAccessAssertion {
                assertion,
                path: path.into(),
            })
            .await?
        {
            ControlResult::AccessVerification(outcome) => Ok(outcome),
            _ => Err("control AccessVerification response".into()),
        }
    }
    pub async fn evict(&self, address: ObjectAddress) -> ConformanceResult<()> {
        self.control(ControlRequest::Evict { address }).await?;
        Ok(())
    }
    /// Pins the registry's test clock locally. A staging central keeps its real
    /// clock, so this does nothing there; scenarios that advance time don't run
    /// on staging.
    pub async fn set_time(&self, now: RegistryTime) -> ConformanceResult<()> {
        if let AccessCredentials::Fixture = self.access {
            self.control(ControlRequest::SetTime { now }).await?;
        }
        Ok(())
    }
    pub async fn connect(&self, token: &str) -> ConformanceResult<HolderSession> {
        let response = self
            .as_machine(
                self.client.get(format!("{}/v1/session", self.base_url)),
                token,
            )?
            .upgrade()
            .send()
            .await?;
        Ok(HolderSession {
            socket: response.into_websocket().await?,
        })
    }
    /// Opens a session from a caller-built upgrade request, for probes that
    /// mix or add headers the ordinary connect never sends.
    pub(crate) async fn open_session(
        &self,
        request: reqwest::RequestBuilder,
    ) -> ConformanceResult<HolderSession> {
        Ok(HolderSession {
            socket: request.upgrade().send().await?.into_websocket().await?,
        })
    }
    pub async fn connect_owner(&self) -> ConformanceResult<HolderSession> {
        let response = self
            .as_owner(self.client.get(format!("{}/v1/session", self.base_url)))?
            .upgrade()
            .send()
            .await?;
        Ok(HolderSession {
            socket: response.into_websocket().await?,
        })
    }
}
pub struct HolderSession {
    socket: WebSocket,
}
impl HolderSession {
    /// Sends deliberately malformed protocol input without the typed serializer.
    pub(crate) async fn send_raw(&mut self, bytes: Vec<u8>) -> ConformanceResult<()> {
        self.socket.send(Message::Binary(bytes.into())).await?;
        Ok(())
    }
    pub async fn send(&mut self, frame: &MachineFrame) -> ConformanceResult<()> {
        self.socket
            .send(Message::Binary(serde_json::to_vec(frame)?.into()))
            .await?;
        Ok(())
    }
    pub async fn receive(&mut self) -> ConformanceResult<CentralFrame> {
        loop {
            let message =
                tokio::time::timeout(std::time::Duration::from_secs(10), self.socket.next())
                    .await?
                    .ok_or("socket ended")??;
            match message {
                Message::Binary(bytes) => return Ok(serde_json::from_slice(&bytes)?),
                Message::Ping(_) | Message::Pong(_) => continue,
                Message::Close { code, reason } => {
                    return Err(format!(
                        "CentralFrame socket closed: code {code:?}, reason bytes {}",
                        reason.len()
                    )
                    .into());
                }
                Message::Text(text) => {
                    return Err(
                        format!("expected binary CentralFrame, text bytes {}", text.len()).into(),
                    );
                }
            }
        }
    }
    pub async fn request(&mut self, body: SessionRequest) -> ConformanceResult<SessionAnswer> {
        let frame_id = FrameId::new(uuid::Uuid::now_v7().to_string())?;
        self.send(&MachineFrame::Request {
            frame_id: frame_id.clone(),
            body,
        })
        .await?;
        loop {
            match self.receive().await? {
                CentralFrame::Answer {
                    frame_id: actual,
                    body,
                } if actual == frame_id => return Ok(body),
                CentralFrame::Notice { .. } => continue,
                _ => return Err("request must have one matching answer".into()),
            }
        }
    }
    /// Waits up to `bound` for the server to end this socket, past any pings
    /// or notices. True when it ended; false when it was still open.
    pub async fn await_server_close(
        &mut self,
        bound: std::time::Duration,
    ) -> ConformanceResult<bool> {
        let deadline = tokio::time::Instant::now() + bound;
        loop {
            match tokio::time::timeout_at(deadline, self.socket.next()).await {
                Err(_) => return Ok(false),
                Ok(None | Some(Err(_)) | Some(Ok(Message::Close { .. }))) => return Ok(true),
                Ok(Some(Ok(_))) => continue,
            }
        }
    }
    pub async fn close(self) -> ConformanceResult<()> {
        self.socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
}
