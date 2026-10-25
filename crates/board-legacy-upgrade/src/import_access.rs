//! Credentials stay in memory; the only debug override is an owner-private file.

use board_legacy_upgrade::UpgradePublisher;
use reqwest::header::{HeaderMap, HeaderValue};

pub async fn publisher(
    endpoint: reqwest::Url,
) -> Result<UpgradePublisher, Box<dyn std::error::Error>> {
    let mut headers = HeaderMap::new();
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
