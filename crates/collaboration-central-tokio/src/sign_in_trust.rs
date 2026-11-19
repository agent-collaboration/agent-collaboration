//! Native sign-in trust: real HTTP key publication and core-owned cache policy.

use crate::sign_in_crypto::{NativeEd25519, NativeEs256};
use fleet_contracts::FleetOrganizationStatementRefusal;
use fleet_identity_protocol::OrganizationId;
use fleet_runtime_ports::Clock;
use fleet_runtime_ports::{IssuerKeySource, KeyFetchFailure, RawJwks};
use fleet_signin::IssuerKeyCache;
use fleet_signin::SignInTrustSettings;
use fleet_signin::organization_authority::SignInVerification;
use std::sync::Arc;

/// The host retains one trust configuration and key cache across its org runtimes.
pub struct NativeSignInTrust {
    settings: SignInTrustSettings,
    client: reqwest::Client,
    cache: Arc<IssuerKeyCache>,
}

impl NativeSignInTrust {
    pub fn new(settings: SignInTrustSettings) -> Result<Self, reqwest::Error> {
        Ok(Self {
            settings,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            cache: Arc::default(),
        })
    }

    /// Bind trust to this org and the same clock its runtime uses for admission.
    pub fn for_organization(
        &self,
        organization: OrganizationId,
        clock: Arc<dyn Clock>,
    ) -> Result<Arc<dyn SignInVerification>, FleetOrganizationStatementRefusal> {
        let keys = || NativeSignInKeys {
            client: self.client.clone(),
            source: self.settings.jwks_url.clone(),
        };
        self.settings.for_organization(
            organization,
            clock,
            &self.cache,
            keys,
            || NativeEd25519,
            NativeEs256,
        )
    }
}

struct NativeSignInKeys {
    client: reqwest::Client,
    source: String,
}

impl IssuerKeySource for NativeSignInKeys {
    fn configured_source(&self) -> &str {
        &self.source
    }

    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        let response = self
            .client
            .get(&self.source)
            .send()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?;
        let status = response.status();
        let response = response
            .error_for_status()
            .map_err(|_| KeyFetchFailure::BadResponse {
                status: status.as_u16(),
            })?;
        let cache_control = response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response
            .bytes()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?
            .to_vec();
        Ok(RawJwks {
            body,
            cache_control,
        })
    }
}

#[cfg(test)]
#[path = "sign_in_trust_test.rs"]
mod tests;
