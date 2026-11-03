//! A configured issuer address and the host's long-lived HTTP client.
use collaboration_central_core::{
    IssuerKeySource, KeyFetchFailure, RawJwks, identity_contract::ResourceUri,
};

#[derive(Clone)]
pub struct TokioIssuerKeySource {
    client: reqwest::Client,
    configured_source: ResourceUri,
}

impl TokioIssuerKeySource {
    pub(crate) fn new(client: reqwest::Client, configured_source: ResourceUri) -> Self {
        Self {
            client,
            configured_source,
        }
    }
}

impl IssuerKeySource for TokioIssuerKeySource {
    fn configured_source(&self) -> &str {
        self.configured_source.as_str()
    }

    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        let response = self
            .client
            .get(self.configured_source.as_str())
            .header(reqwest::header::CACHE_CONTROL, "no-cache, no-store")
            .send()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?;
        let status = response.status();
        if !status.is_success() {
            return Err(KeyFetchFailure::BadResponse {
                status: status.as_u16(),
            });
        }
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
