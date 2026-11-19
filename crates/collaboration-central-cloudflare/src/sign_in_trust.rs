//! Worker sign-in trust configured explicitly for org-scoped central runtimes.

use crate::sign_in_crypto::{WorkerEd25519, WorkerEs256};
use fleet_contracts::FleetOrganizationStatementRefusal;
use fleet_identity_protocol::OrganizationId;
use fleet_runtime_ports::{Clock, IssuerKeySource, KeyFetchFailure, RawJwks};
use fleet_signin::IssuerKeyCache;
use fleet_signin::SignInTrustSettings;
use fleet_signin::organization_authority::SignInVerification;
use std::{future::Future, sync::Arc};
use worker::{Fetch, send::SendFuture};

/// Retained by the host so fresh verifiers share the issuer's bounded key cache.
pub struct CloudflareSignInTrust {
    settings: SignInTrustSettings,
    cache: Arc<IssuerKeyCache>,
}

impl CloudflareSignInTrust {
    #[must_use]
    pub fn new(settings: SignInTrustSettings) -> Self {
        Self {
            settings,
            cache: Arc::default(),
        }
    }

    /// Bind trust to this org and the same clock its runtime uses for admission.
    pub fn for_organization(
        &self,
        organization: OrganizationId,
        clock: Arc<dyn Clock>,
    ) -> Result<Arc<dyn SignInVerification>, FleetOrganizationStatementRefusal> {
        let keys = || WorkerSignInKeys {
            source: self.settings.jwks_url.clone(),
        };
        self.settings.for_organization(
            organization,
            clock,
            &self.cache,
            keys,
            || WorkerEd25519,
            WorkerEs256,
        )
    }
}

struct WorkerSignInKeys {
    source: String,
}

impl IssuerKeySource for WorkerSignInKeys {
    fn configured_source(&self) -> &str {
        &self.source
    }

    fn fetch_issuer_keys(&self) -> impl Future<Output = Result<RawJwks, KeyFetchFailure>> + Send {
        SendFuture::new(async {
            let mut response = Fetch::Url(
                self.source
                    .parse()
                    .map_err(|_| KeyFetchFailure::Unreachable)?,
            )
            .send()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?;
            let status = response.status_code();
            if !(200..300).contains(&status) {
                return Err(KeyFetchFailure::BadResponse { status });
            }
            let cache_control = response
                .headers()
                .get("cache-control")
                .map_err(|_| KeyFetchFailure::Unreachable)?;
            let body = response
                .bytes()
                .await
                .map_err(|_| KeyFetchFailure::Unreachable)?;
            Ok(RawJwks {
                body,
                cache_control,
            })
        })
    }
}
