//! Authenticated service forms use the host's existing long-lived HTTP client.
use collaboration_central_core::{
    ServiceForm, ServiceFormFailure, ServiceFormTransport, identity_contract::ResourceUri,
};

#[derive(Clone)]
pub struct TokioServiceFormTransport {
    client: reqwest::Client,
}
impl TokioServiceFormTransport {
    pub(crate) fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}
impl ServiceFormTransport for TokioServiceFormTransport {
    async fn post_service_form(
        &self,
        endpoint: &ResourceUri,
        form: &ServiceForm,
    ) -> Result<Vec<u8>, ServiceFormFailure> {
        let response = self
            .client
            .post(endpoint.as_str())
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(reqwest::header::CACHE_CONTROL, "no-store")
            .body(form.as_str().to_owned())
            .send()
            .await
            .map_err(|_| ServiceFormFailure::Unreachable)?;
        if !response.status().is_success() {
            return Err(ServiceFormFailure::Refused {
                status: response.status().as_u16(),
            });
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|_| ServiceFormFailure::Unreachable)
    }
}
