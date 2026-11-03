//! C7 forms and JSON use the host's existing long-lived HTTP client.
use collaboration_central_core::{
    ServiceForm, ServiceFormTransport, ServiceHttpFailure, identity_contract::ResourceUri,
    service_reply_is_json,
};

#[derive(Clone)]
pub struct TokioServiceHttpTransport {
    client: reqwest::Client,
}
impl TokioServiceHttpTransport {
    pub(crate) fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}
impl ServiceFormTransport for TokioServiceHttpTransport {
    async fn post_service_form(
        &self,
        endpoint: &ResourceUri,
        form: &ServiceForm,
    ) -> Result<Vec<u8>, ServiceHttpFailure> {
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
            .map_err(|_| ServiceHttpFailure::Unreachable)?;
        read_service_json_bytes(response).await
    }
}
impl collaboration_central_core::ServiceJsonTransport for TokioServiceHttpTransport {
    async fn post_service_json(
        &self,
        destination: &ResourceUri,
        bearer: &collaboration_central_core::ServiceBearerCredential,
        body: &[u8],
    ) -> Result<Vec<u8>, ServiceHttpFailure> {
        let response = self
            .client
            .post(destination.as_str())
            .bearer_auth(bearer.as_str())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::CACHE_CONTROL, "no-store")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| ServiceHttpFailure::Unreachable)?;
        read_service_json_bytes(response).await
    }
}
async fn read_service_json_bytes(
    response: reqwest::Response,
) -> Result<Vec<u8>, ServiceHttpFailure> {
    if !response.status().is_success() {
        return Err(ServiceHttpFailure::Refused {
            status: response.status().as_u16(),
        });
    }
    if !service_reply_is_json(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
    ) {
        return Err(ServiceHttpFailure::InvalidReply);
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|_| ServiceHttpFailure::Unreachable)
}
