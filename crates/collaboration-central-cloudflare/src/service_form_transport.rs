//! Service forms carry client authentication on the pinned TLS endpoint.
use collaboration_central_core::{
    ServiceForm, ServiceFormFailure, ServiceFormTransport, identity_contract::ResourceUri,
    service_reply_is_json,
};
use worker::{
    CacheMode, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, send::SendFuture,
    wasm_bindgen::JsValue,
};

#[derive(Clone, Copy, Default)]
pub struct CloudflareServiceFormTransport;
impl ServiceFormTransport for CloudflareServiceFormTransport {
    fn post_service_form(
        &self,
        endpoint: &ResourceUri,
        form: &ServiceForm,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, ServiceFormFailure>> + Send {
        SendFuture::new(async move {
            let headers = Headers::new();
            headers
                .set("content-type", "application/x-www-form-urlencoded")
                .map_err(|_| ServiceFormFailure::Unreachable)?;
            headers
                .set("cache-control", "no-store")
                .map_err(|_| ServiceFormFailure::Unreachable)?;
            let mut init = RequestInit::new();
            init.with_method(Method::Post)
                .with_headers(headers)
                .with_redirect(RequestRedirect::Manual)
                .with_cache(CacheMode::NoStore)
                .with_body(Some(JsValue::from_str(form.as_str())));
            let request = Request::new_with_init(endpoint.as_str(), &init)
                .map_err(|_| ServiceFormFailure::Unreachable)?;
            let mut response = Fetch::Request(request)
                .send()
                .await
                .map_err(|_| ServiceFormFailure::Unreachable)?;
            let status = response.status_code();
            if !(200..300).contains(&status) {
                return Err(ServiceFormFailure::Refused { status });
            }
            let content_type = response
                .headers()
                .get("content-type")
                .map_err(|_| ServiceFormFailure::InvalidReply)?;
            if !service_reply_is_json(content_type.as_deref()) {
                return Err(ServiceFormFailure::InvalidReply);
            }
            response
                .bytes()
                .await
                .map_err(|_| ServiceFormFailure::Unreachable)
        })
    }
}
