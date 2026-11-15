//! Private Worker entry and routing to the organization's modern central object.

use collaboration_central_core::{
    SignInTrustSettings,
    identity_contract::{
        AUTHORIZATION_REVISIONS_ROUTE, ORGANIZATION_STATEMENT_DIGEST_ROUTE,
        PROVISION_ORGANIZATION_ROUTE,
    },
    person_request::AdminRoute,
};
use worker::Env;

pub(crate) fn is_signin_path(path: &str) -> bool {
    [
        PROVISION_ORGANIZATION_ROUTE,
        ORGANIZATION_STATEMENT_DIGEST_ROUTE,
        AUTHORIZATION_REVISIONS_ROUTE,
        AdminRoute::CreateWorkspace.path(),
        AdminRoute::WorkspaceLookup.path(),
        AdminRoute::ReadChangeRecords.path(),
    ]
    .contains(&path)
}

/// A deployment with no sign-in configuration keeps ordinary central operation.
/// Once any field is supplied, all required settings must be present and valid.
pub(crate) fn trust_settings(env: &Env) -> worker::Result<Option<SignInTrustSettings>> {
    let variable = |name: &str| {
        let present = js_sys::Reflect::has(env, &worker::wasm_bindgen::JsValue::from_str(name))
            .map_err(|_| invalid_configuration())?;
        if !present {
            return Ok(None);
        }
        env.var(name)
            .map(|value| Some(value.to_string()))
            .map_err(|_| invalid_configuration())
    };
    let issuer = variable("CENTRAL_SIGN_IN_ISSUER")?;
    let audience = variable("CENTRAL_SIGN_IN_AUDIENCE")?;
    let jwks_url = variable("CENTRAL_SIGN_IN_JWKS_URL")?;
    let admin_audience = variable("CENTRAL_ADMIN_AUDIENCE")?;
    let public_origin = variable("CENTRAL_PUBLIC_ORIGIN")?;
    let service_id = variable("CENTRAL_SIGN_IN_SERVICE_ID")?;
    if [
        &issuer,
        &audience,
        &jwks_url,
        &admin_audience,
        &public_origin,
        &service_id,
    ]
    .iter()
    .all(|value| value.is_none())
    {
        return Ok(None);
    }
    let required = |value: Option<String>| {
        value
            .filter(|value| !value.is_empty() && !value.contains('\0'))
            .ok_or_else(invalid_configuration)
    };
    let settings = SignInTrustSettings {
        issuer: required(issuer)?,
        audience: required(audience)?,
        admin_audience: required(admin_audience)?,
        public_origin: required(public_origin)?,
        jwks_url: required(jwks_url)?,
        service_id: service_id
            .map(message_board::ServiceId::try_from)
            .transpose()
            .map_err(|_| invalid_configuration())?,
    };
    // These are deployment addresses, never addresses read from a token.
    let origin =
        worker::Url::parse(&settings.public_origin).map_err(|_| invalid_configuration())?;
    if origin.scheme() != "https"
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
        || origin.path() != "/"
    {
        return Err(invalid_configuration());
    }
    let keys = worker::Url::parse(&settings.jwks_url).map_err(|_| invalid_configuration())?;
    if !matches!(keys.scheme(), "http" | "https") || keys.host_str().is_none() {
        return Err(invalid_configuration());
    }
    Ok(Some(settings))
}

fn invalid_configuration() -> worker::Error {
    worker::Error::RustError("invalid sign-in trust configuration".into())
}

#[cfg(feature = "link")]
mod private_entry {
    use super::is_signin_path;
    use crate::{cloudflare_transport, http_body};
    use collaboration_central_core::ObjectAddress;
    use fleet_identity_protocol::OrganizationId;
    use serde::Deserialize;
    use worker::{Context, Env, Request, Response, Result, event};

    const ENTRY_HEADER: &str = "x-collaboration-central-entry";
    const LINK_ENTRY: &str = "collaboration-central-link-entry";

    /// Used only to address the object. Its runtime parses the whole request.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Addressed {
        organization_id: OrganizationId,
    }

    #[event(fetch)]
    pub async fn main(request: Request, env: Env, _context: Context) -> Result<Response> {
        let response = forward(request, &env).await?;
        // A fetched response may carry an immutable header guard.
        let headers = response.headers().clone();
        headers.set(ENTRY_HEADER, LINK_ENTRY)?;
        Ok(response.with_headers(headers))
    }

    async fn forward(request: Request, env: &Env) -> Result<Response> {
        let request: worker::HttpRequest = request.try_into()?;
        let (mut parts, body) = request.into_parts();
        let body = match http_body::complete(body).await? {
            http_body::BodyCompletion::Complete(body) => body,
            http_body::BodyCompletion::OverLimit => return http_body::over_limit(),
        };
        if !is_signin_path(parts.uri.path()) {
            return Response::empty().map(|response| response.with_status(404));
        }
        // Completion above has already enforced the upload bound. Preserve these
        // exact bytes when forwarding DPoP's signed request intent.
        let bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|_| worker::Error::RustError("sign-in body unavailable".into()))?;
        let Ok(Addressed { organization_id }) = serde_json::from_slice(&bytes) else {
            return Response::empty().map(|response| response.with_status(400));
        };
        let address = ObjectAddress::Central(organization_id);
        let stub = match cloudflare_transport::stub(env, &address) {
            Ok(stub) => stub,
            Err(_) => return Response::empty().map(|response| response.with_status(503)),
        };
        parts.headers.insert(
            "x-central-object-address",
            http::HeaderValue::from_str(&serde_json::to_string(&address)?)
                .map_err(|_| worker::Error::RustError("central address unavailable".into()))?,
        );
        let request = http::Request::from_parts(parts, axum::body::Body::from(bytes));
        match stub.fetch_with_request(request.try_into()?).await {
            Ok(response) => Ok(response),
            Err(_) => Response::empty().map(|response| response.with_status(503)),
        }
    }
}
