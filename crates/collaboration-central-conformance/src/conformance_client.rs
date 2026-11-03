//! HTTP credentials and verifier transport for current runtime scenarios.
use collaboration_central_core::AccessRefusal;
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
#[derive(serde::Deserialize)]
struct AdmittedConnectionObservation {
    connection: fleet_sync_protocol::ConnectionId,
    peer: fleet_sync_protocol::PeerId,
    registration: fleet_sync_session::ConnectionRegistration,
}
impl ConformanceClient {
    /// Select the actual live socket for a normally admitted peer.
    pub async fn active_connection(
        &self,
        service: &str,
        peer: fleet_sync_protocol::PeerId,
    ) -> ConformanceResult<fleet_sync_protocol::ConnectionId> {
        let connections = self.session_connections(Some(service)).await?;
        let mut live = connections.into_iter().filter(|connection| {
            connection.peer == peer
                && !matches!(
                    connection.registration,
                    fleet_sync_session::ConnectionRegistration::Closed
                )
        });
        let connection = live
            .next()
            .ok_or("peer has no live normal socket")?
            .connection;
        if live.next().is_some() {
            return Err("peer has multiple live normal sockets".into());
        }
        Ok(connection)
    }

    async fn session_connections(
        &self,
        service: Option<&str>,
    ) -> ConformanceResult<Vec<AdmittedConnectionObservation>> {
        Ok(self
            .with_owner_access(
                self.client
                    .post(format!("{}/__conformance/admission", self.base_url)),
            )?
            .json(&serde_json::json!({"kind":"sessionConnections","value":{"service":service}}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Wake local injected-clock work after this Hello's admission has committed.
    pub async fn wake_due_after_hello(
        &self,
        session: fleet_sync_protocol::SessionId,
    ) -> ConformanceResult<()> {
        if self.staging_access().is_some() {
            return Ok(());
        }
        let observed = crate::composed_client::control(
            self,
            collaboration_central_core::ObjectAddress::Fleet,
            crate::RuntimeControl::Observe,
        )
        .await?;
        let crate::ClockObservation::Pinned { now } = observed.clock else {
            return Ok(());
        };
        // Observe the real committed attachment before waking; an HTTP control may overtake a socket frame.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let connections = self.session_connections(None).await?;
                let admitted = connections.iter().any(|connection| {
                    matches!(connection.registration,
                        fleet_sync_session::ConnectionRegistration::Attaching {session_id,..}
                        | fleet_sync_session::ConnectionRegistration::Attached {session_id,..} if session_id == session)
                        && !connections.iter().any(|other| other.peer == connection.peer
                            && matches!(other.registration, fleet_sync_session::ConnectionRegistration::Opening))
                });
                if admitted { return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        crate::composed_client::control(
            self,
            collaboration_central_core::ObjectAddress::Fleet,
            crate::RuntimeControl::SetTime { now },
        )
        .await?;
        Ok(())
    }

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
        Ok(self
            .with_owner_access(
                self.client
                    .post(format!("{}/admin/v1/conformance/access", self.base_url)),
            )?
            .json(&serde_json::json!({"assertion":assertion,"path":path}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}
