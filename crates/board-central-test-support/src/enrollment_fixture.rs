//! A real central with no pre-enrolled machine or enrollment-file writer.
use crate::{FixtureResult, enrollment_edge::EnrollmentAccessEdge};
use collaboration_central_conformance::{JwksFixtureServer, fixture_policy};
use collaboration_central_core::{EnrollMachine, EnrollmentDecision};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use std::{path::Path, time::Duration};

pub struct EnrollmentTestCentral {
    server: TokioServer,
    jwks: JwksFixtureServer,
    edge: EnrollmentAccessEdge,
}
impl EnrollmentTestCentral {
    pub async fn start(root: &Path) -> FixtureResult<Self> {
        let jwks = JwksFixtureServer::start().await?;
        let server = TokioServer::start_conformance(TokioSettings {
            root: root.into(),
            access_policy: fixture_policy(),
            jwks_url: jwks.url.clone(),
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240".parse()?,
            },
            registry: RegistryConfig {
                lease: LeasePolicy {
                    lease_duration: Duration::from_secs(14_400),
                    renew_interval: Duration::from_secs(900),
                },
                authoritative_receiver: "019f0000-0000-7000-8000-000000000010".parse()?,
                release_unsent_for: Duration::ZERO,
                move_needs_renewal_within: Some(Duration::from_secs(1800)),
            },
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        })
        .await;
        let server = match server {
            Ok(server) => server,
            Err(error) => {
                let _stopped = jwks.shutdown().await;
                return Err(error.into());
            }
        };
        let edge = match EnrollmentAccessEdge::start(server.base_url()).await {
            Ok(edge) => edge,
            Err(error) => {
                let _central_stopped = server.shutdown().await;
                let _jwks_stopped = jwks.shutdown().await;
                return Err(error);
            }
        };
        Ok(Self { server, jwks, edge })
    }
    pub fn endpoint(&self) -> &str {
        &self.edge.endpoint
    }
    pub fn access_client_id(&self) -> &'static str {
        crate::enrollment_edge::CLIENT_ID
    }
    pub fn access_client_secret(&self) -> &'static str {
        crate::enrollment_edge::CLIENT_SECRET
    }
    pub fn owner_token(&self) -> &'static str {
        crate::enrollment_edge::OWNER_TOKEN
    }
    pub fn enrollment_calls(&self) -> usize {
        self.edge.admin_calls()
    }
    pub fn session_connections(&self) -> usize {
        self.edge.sessions()
    }
    pub fn enrollments(&self) -> FixtureResult<Vec<EnrollMachine>> {
        self.edge.observations()
    }
    pub fn session_authorization_hashes(&self) -> FixtureResult<Vec<String>> {
        self.edge.authorization_hashes()
    }
    /// Explicit response controls prove CLI refusal handling, not central decision policy.
    pub fn next_enrollment_reply(&self, reply: EnrollmentDecision) -> FixtureResult {
        self.edge.force_reply(reply)
    }
    pub fn next_raw_enrollment_reply(&self, status: u16, body: Vec<u8>) -> FixtureResult {
        self.edge.raw_reply(status, body)
    }
    pub fn next_enrollment_redirect(&self, location: String) -> FixtureResult {
        self.edge.redirect_reply(location)
    }
    pub async fn wait_for_credential_rejection(&self) -> FixtureResult<u16> {
        self.edge.wait_for_credential_rejection().await
    }
    pub async fn shutdown(self) -> FixtureResult {
        let edge = self.edge.shutdown().await;
        let server = self.server.shutdown().await;
        let jwks = self.jwks.shutdown().await;
        edge?;
        server?;
        jwks?;
        Ok(())
    }
}
