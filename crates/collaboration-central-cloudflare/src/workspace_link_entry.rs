//! The public machine-link address selects an organization, never its authority.
use collaboration_central_core::ObjectAddress;
use fleet_identity_protocol::OrganizationId;
use worker::{Env, Method, Request, Response, Result};

pub(crate) fn organization(path: &str) -> Option<std::result::Result<OrganizationId, ()>> {
    let rest = path.strip_prefix("/v1/organizations/")?;
    let id = rest.strip_suffix("/link")?;
    Some(if id.contains('/') {
        Err(())
    } else {
        id.to_owned().try_into().map_err(|_| ())
    })
}

pub(crate) fn bearer(request: &Request) -> Result<Option<String>> {
    let Some(value) = request.headers().get("authorization")? else {
        return Ok(None);
    };
    let Some((scheme, token)) = value.split_once(' ') else {
        return Ok(None);
    };
    Ok((scheme.eq_ignore_ascii_case("bearer")
        && !token.is_empty()
        && !token
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b','))
    .then(|| token.to_owned()))
}

pub(crate) fn is_upgrade(request: &Request) -> Result<bool> {
    Ok(request.method() == Method::Get
        && request
            .headers()
            .get("upgrade")?
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket")))
}

pub(crate) async fn forward(request: Request, env: &Env) -> Result<Response> {
    let Some(Ok(organization)) = organization(&request.path()) else {
        return Response::error("invalid organization", 400);
    };
    if !is_upgrade(&request)? {
        return Response::error("websocket upgrade required", 426);
    }
    if bearer(&request)?.is_none() {
        return Response::error("machine token required", 401);
    }
    let address = ObjectAddress::Central(organization);
    let mut forwarded = request.clone_mut()?;
    crate::cloudflare_transport::address_header(forwarded.headers_mut()?, &address)?;
    crate::cloudflare_transport::stub(env, &address)?
        .fetch_with_request(forwarded)
        .await
}
