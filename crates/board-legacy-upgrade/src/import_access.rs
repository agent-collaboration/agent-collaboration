//! Credentials stay in memory; the only debug override is an owner-private file.

use board_legacy_upgrade::UpgradePublisher;
use reqwest::header::{HeaderMap, HeaderValue};

pub async fn publisher(
    endpoint: reqwest::Url,
    self_hosted_root: Option<std::path::PathBuf>,
    owner_credential_file: Option<std::path::PathBuf>,
) -> Result<UpgradePublisher, Box<dyn std::error::Error>> {
    let mut headers = HeaderMap::new();
    if self_hosted_root.is_some() || owner_credential_file.is_some() {
        let credential = tokio::task::spawn_blocking(
            move || -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
                use codex_router_secret_store::{
                    SecretStore,
                    central_credentials::{CentralCredential, CentralSecretKind},
                };
                if let Some(root) = self_hosted_root {
                    if !root.is_absolute() {
                        return Err("--self-hosted-root must be absolute".into());
                    }
                    let store = codex_router_host::open_fleet_enrollment_store(&root)?;
                    let encoded =
                        store.read_secret(&CentralSecretKind::OwnerCredential.secret_key()?)?;
                    Ok(CentralCredential::decode_secret(&encoded)?)
                } else if let Some(path) = owner_credential_file {
                    Ok(CentralCredential::consume_private_file(&path)?)
                } else {
                    Err("self-hosted owner credential unavailable".into())
                }
            },
        )
        .await?
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;
        let codex_router_secret_store::central_credentials::CentralCredential::Owner {
            endpoint: origin,
            assertion,
            expires_at,
        } = credential
        else {
            return Err("expected an owner credential; run central mint --kind owner".into());
        };
        if reqwest::Url::parse(&origin)?.origin() != endpoint.origin() {
            return Err("owner credential belongs to a different central endpoint".into());
        }
        if chrono::DateTime::parse_from_rfc3339(&expires_at)? <= chrono::Utc::now() {
            return Err("owner credential has expired; run central mint --kind owner again".into());
        }
        let mut value =
            HeaderValue::from_str(&assertion).map_err(|_| "invalid owner credential")?;
        value.set_sensitive(true);
        headers.insert("Cf-Access-Jwt-Assertion", value);
        return build(endpoint, headers);
    }
    #[cfg(debug_assertions)]
    let assertion = std::env::var_os("CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE")
        .map(std::path::PathBuf::from)
        .map(debug_assertion)
        .transpose()?;
    #[cfg(debug_assertions)]
    if let Some(assertion) = assertion {
        headers.insert("Cf-Access-Jwt-Assertion", assertion);
        return build(endpoint, headers);
    }
    let token = collaboration_client::owner_app_token(endpoint.as_str()).await?;
    let mut token = HeaderValue::from_str(&token)?;
    token.set_sensitive(true);
    headers.insert("cf-access-token", token);
    build(endpoint, headers)
}

fn build(
    endpoint: reqwest::Url,
    headers: HeaderMap,
) -> Result<UpgradePublisher, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    Ok(UpgradePublisher::new(endpoint, client))
}

#[cfg(debug_assertions)]
#[derive(Debug, thiserror::Error)]
enum DebugAssertionError {
    #[error("debug central assertion file must be an absolute path")]
    RelativePath,
    #[error("debug central assertion file must be a regular owner-only file")]
    Permissions,
    #[error("debug central assertion file is unreadable")]
    Read(#[source] std::io::Error),
    #[error("debug central assertion is not a valid HTTP header")]
    Header(#[source] reqwest::header::InvalidHeaderValue),
}

#[cfg(debug_assertions)]
fn debug_assertion(path: std::path::PathBuf) -> Result<HeaderValue, DebugAssertionError> {
    use std::{io::Read, os::unix::fs::MetadataExt};
    if !path.is_absolute() {
        return Err(DebugAssertionError::RelativePath);
    }
    let mut file = std::fs::File::open(path).map_err(DebugAssertionError::Read)?;
    let metadata = file.metadata().map_err(DebugAssertionError::Read)?;
    if !metadata.is_file()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(DebugAssertionError::Permissions);
    }
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(DebugAssertionError::Read)?;
    let mut value = HeaderValue::from_str(contents.trim()).map_err(DebugAssertionError::Header)?;
    value.set_sensitive(true);
    Ok(value)
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn debug_assertion_refuses_relative_and_shared_files_without_echoing_contents()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(matches!(
            debug_assertion("relative.assertion".into()),
            Err(DebugAssertionError::RelativePath)
        ));
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("assertion");
        std::fs::write(&path, "private.fixture.assertion")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
        let error = debug_assertion(path.clone())
            .err()
            .ok_or("shared file accepted")?;
        assert!(matches!(error, DebugAssertionError::Permissions));
        assert!(!error.to_string().contains("private.fixture.assertion"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let value = debug_assertion(path)?;
        assert!(value.is_sensitive());
        assert_eq!(value.to_str()?, "private.fixture.assertion");
        Ok(())
    }
}
