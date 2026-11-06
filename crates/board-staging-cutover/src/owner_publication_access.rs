//! Reuse existing owner credentials without persisting tokens in a cutover artifact.
use codex_router_secret_store::central_credentials::CentralCredential;
use reqwest::header::{HeaderMap, HeaderValue};
use runtime_primitives::Clock;
use std::path::Path;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum OwnerPublicationAccessError {
    #[error("owner publication credentials are unavailable")]
    Unavailable,
    #[error("publication requires an Owner credential")]
    WrongKind,
    #[error("owner credential belongs to another central origin")]
    WrongOrigin,
    #[error("owner credential is expired")]
    Expired,
    #[error("owner credential is malformed")]
    Malformed,
}

pub(crate) fn headers_from_private_file(
    endpoint: &Url,
    path: &Path,
    clock: &impl Clock,
) -> Result<HeaderMap, OwnerPublicationAccessError> {
    let credential = CentralCredential::consume_private_file(path)
        .map_err(|_| OwnerPublicationAccessError::Unavailable)?;
    let CentralCredential::Owner {
        endpoint: origin,
        assertion,
        expires_at,
    } = credential
    else {
        return Err(OwnerPublicationAccessError::WrongKind);
    };
    let origin = Url::parse(&origin).map_err(|_| OwnerPublicationAccessError::Malformed)?;
    if origin.origin() != endpoint.origin() {
        return Err(OwnerPublicationAccessError::WrongOrigin);
    }
    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_at)
        .map_err(|_| OwnerPublicationAccessError::Malformed)?;
    if expires_at.timestamp_millis() <= clock.now().unix_millis() {
        return Err(OwnerPublicationAccessError::Expired);
    }
    let mut assertion =
        HeaderValue::from_str(&assertion).map_err(|_| OwnerPublicationAccessError::Malformed)?;
    assertion.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert("Cf-Access-Jwt-Assertion", assertion);
    Ok(headers)
}

#[cfg(test)]
#[path = "owner_publication_access_test.rs"]
mod tests;

pub(crate) fn build_owner_client(
    headers: HeaderMap,
) -> Result<reqwest::Client, OwnerPublicationAccessError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| OwnerPublicationAccessError::Unavailable)
}

pub(crate) async fn owner_access_client(
    endpoint: &Url,
) -> Result<reqwest::Client, OwnerPublicationAccessError> {
    let token = collaboration_client::owner_app_token(endpoint.as_str())
        .await
        .map_err(|_| OwnerPublicationAccessError::Unavailable)?;
    let mut token =
        HeaderValue::from_str(&token).map_err(|_| OwnerPublicationAccessError::Malformed)?;
    token.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert("cf-access-token", token);
    build_owner_client(headers)
}
