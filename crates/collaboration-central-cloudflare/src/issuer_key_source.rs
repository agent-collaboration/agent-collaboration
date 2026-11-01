//! The issuer key address is supplied by deployment configuration, not a JWT.
use collaboration_central_core::{
    IssuerKeySource, KeyFetchFailure, RawJwks, identity_contract::ResourceUri,
};
use std::future::Future;
use worker::{CacheMode, Fetch, Headers, Request, RequestInit, RequestRedirect, send::SendFuture};

#[derive(Clone)]
pub struct CloudflareIssuerKeySource {
    configured_source: ResourceUri,
}

impl CloudflareIssuerKeySource {
    #[must_use]
    pub fn new(configured_source: ResourceUri) -> Self {
        Self { configured_source }
    }
}

impl IssuerKeySource for CloudflareIssuerKeySource {
    fn configured_source(&self) -> &str {
        self.configured_source.as_str()
    }

    fn fetch_issuer_keys(&self) -> impl Future<Output = Result<RawJwks, KeyFetchFailure>> + Send {
        SendFuture::new(async move {
            let mut settings = RequestInit::new();
            let headers = Headers::new();
            headers
                .set("cache-control", "no-cache, no-store")
                .map_err(|_| KeyFetchFailure::Unreachable)?;
            settings
                .with_headers(headers)
                .with_redirect(RequestRedirect::Manual)
                .with_cache(CacheMode::NoStore);
            let request = Request::new_with_init(self.configured_source.as_str(), &settings)
                .map_err(|_| KeyFetchFailure::Unreachable)?;
            let mut response = Fetch::Request(request)
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
