//! Client-authentication forms and bearer JSON use their pinned C7 endpoints.
use collaboration_central_core::{
    ServiceForm, ServiceFormTransport, ServiceHttpFailure, identity_contract::ResourceUri,
    service_reply_is_json,
};
use worker::{
    CacheMode, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, send::SendFuture,
    wasm_bindgen::JsValue,
};

#[derive(Clone, Copy, Default)]
pub struct CloudflareServiceHttpTransport;
impl ServiceFormTransport for CloudflareServiceHttpTransport {
    fn post_service_form(
        &self,
        endpoint: &ResourceUri,
        form: &ServiceForm,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, ServiceHttpFailure>> + Send {
        SendFuture::new(async move {
            let headers = Headers::new();
            headers
                .set("content-type", "application/x-www-form-urlencoded")
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            headers
                .set("cache-control", "no-store")
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            let mut init = RequestInit::new();
            init.with_method(Method::Post)
                .with_headers(headers)
                .with_redirect(RequestRedirect::Manual)
                .with_cache(CacheMode::NoStore)
                .with_body(Some(JsValue::from_str(form.as_str())));
            let request = Request::new_with_init(endpoint.as_str(), &init)
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            let response = Fetch::Request(request)
                .send()
                .await
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            read_service_json_bytes(response).await
        })
    }
}
impl collaboration_central_core::ServiceJsonTransport for CloudflareServiceHttpTransport {
    fn post_service_json(
        &self,
        destination: &ResourceUri,
        bearer: &collaboration_central_core::ServiceBearerCredential,
        body: &[u8],
    ) -> impl std::future::Future<Output = Result<Vec<u8>, ServiceHttpFailure>> + Send {
        SendFuture::new(async move {
            let headers = Headers::new();
            headers
                .set("content-type", "application/json")
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            headers
                .set("authorization", &format!("Bearer {}", bearer.as_str()))
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            headers
                .set("cache-control", "no-store")
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            let mut init = RequestInit::new();
            init.with_method(Method::Post)
                .with_headers(headers)
                .with_redirect(RequestRedirect::Manual)
                .with_cache(CacheMode::NoStore)
                .with_body(Some(js_sys::Uint8Array::from(body).into()));
            let request = Request::new_with_init(destination.as_str(), &init)
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            let response = Fetch::Request(request)
                .send()
                .await
                .map_err(|_| ServiceHttpFailure::Unreachable)?;
            read_service_json_bytes(response).await
        })
    }
}
async fn read_service_json_bytes(
    mut response: worker::Response,
) -> Result<Vec<u8>, ServiceHttpFailure> {
    let status = response.status_code();
    if !(200..300).contains(&status) {
        return Err(ServiceHttpFailure::Refused { status });
    }
    let content_type = response
        .headers()
        .get("content-type")
        .map_err(|_| ServiceHttpFailure::InvalidReply)?;
    if !service_reply_is_json(content_type.as_deref()) {
        return Err(ServiceHttpFailure::InvalidReply);
    }
    response
        .bytes()
        .await
        .map_err(|_| ServiceHttpFailure::Unreachable)
}
