//! Owner sign-in remains in cloudflared; callers receive only its in-memory token.

#[derive(Debug, thiserror::Error)]
pub enum OwnerAccessError {
    #[error("cannot invoke cloudflared for owner Access credentials: {0}")]
    CloudflaredUnavailable(#[source] std::io::Error),
    #[error("owner sign-in missing: run `cloudflared access login` for the admin app")]
    SignInRequired,
}

/// Reads the owner's existing sign-in without persisting or printing credentials.
pub async fn owner_app_token(owner_app_url: &str) -> Result<String, OwnerAccessError> {
    let output = tokio::process::Command::new("cloudflared")
        .args(["access", "token", &format!("-app={owner_app_url}")])
        .output()
        .await
        .map_err(OwnerAccessError::CloudflaredUnavailable)?;
    let token = String::from_utf8(output.stdout).map_err(|_| OwnerAccessError::SignInRequired)?;
    let token = token.trim();
    if !output.status.success() || token.split('.').count() != 3 {
        return Err(OwnerAccessError::SignInRequired);
    }
    Ok(token.to_owned())
}
