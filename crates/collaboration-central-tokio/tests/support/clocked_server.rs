//! The normal native server with its existing injected Fleet clock control.

use collaboration_central_conformance::{
    ClockObservation, ConformanceResult, RuntimeControl, RuntimeControlRequest,
    RuntimeFaultObservation, fixture_policy, owner_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{TokioServer, TokioSettings};

use crate::configuration;

pub struct SuppliedClock {
    base: String,
}
impl SuppliedClock {
    pub async fn set_time(&self, at: RegistryTime) -> ConformanceResult<()> {
        let observed: RuntimeFaultObservation = reqwest::Client::builder()
            .no_proxy()
            .build()?
            .post(format!("{}/admin/v1/conformance/runtime", self.base))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&RuntimeControlRequest {
                address: ObjectAddress::Fleet,
                command: RuntimeControl::SetTime { now: at },
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if !matches!(observed.clock, ClockObservation::Pinned { now } if now == at) {
            return Err("supplied clock did not reach the owning Fleet".into());
        }
        Ok(())
    }
}

pub struct ClockedServer {
    server: TokioServer,
    pub clock: SuppliedClock,
}
impl ClockedServer {
    pub async fn start(root: &std::path::Path, jwks: &str) -> ConformanceResult<Self> {
        let server = TokioServer::start(TokioSettings {
            root: root.into(),
            access_policy: fixture_policy(),
            jwks_url: jwks.into(),
            registry: configuration::registry_config()?,
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            holder_answer_timeout: std::time::Duration::from_secs(2),
            copy_ack_wait: std::time::Duration::from_secs(2),
        })
        .await?;
        let clock = SuppliedClock {
            base: server.base_url().into(),
        };
        Ok(Self { server, clock })
    }
    pub fn base_url(&self) -> &str {
        self.server.base_url()
    }
    pub async fn shutdown(self) -> ConformanceResult<()> {
        self.server.shutdown().await?;
        Ok(())
    }
}
