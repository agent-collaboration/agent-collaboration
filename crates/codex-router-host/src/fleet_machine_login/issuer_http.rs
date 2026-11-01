//! The configured C8 OAuth endpoints are the only destination for machine credentials.
use std::{sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use codex_router_core::redaction::SecretString;
use fleet_identity_protocol::EnrollmentId;
use runtime_primitives::SyncTime;
use serde::Deserialize;

use super::{
    device_code_flow::{
        DeviceCodeAuthorization, DeviceLoginRequest, DevicePollResult, IssuedMachineToken,
        IssuerFailure, MachineTokenIssuer, RefreshCredentialBundle,
    },
    login_clock::{FleetLoginClock, LoginFuture},
    login_contract::{UserCode, VerificationUri},
    login_files::{DeviceLoginClientId, HttpServiceUrl},
};

pub(super) struct HttpMachineTokenIssuer {
    client: reqwest::Client,
    device_authorization_url: HttpServiceUrl,
    token_url: HttpServiceUrl,
    device_client_id: DeviceLoginClientId,
    machine_resource: HttpServiceUrl,
    clock: Arc<dyn FleetLoginClock>,
}

impl HttpMachineTokenIssuer {
    pub(super) fn new(
        device_authorization_url: HttpServiceUrl,
        token_url: HttpServiceUrl,
        device_client_id: DeviceLoginClientId,
        machine_resource: HttpServiceUrl,
        clock: Arc<dyn FleetLoginClock>,
    ) -> Result<Self, IssuerFailure> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| IssuerFailure::Unavailable)?;
        Ok(Self {
            client,
            device_authorization_url,
            token_url,
            device_client_id,
            machine_resource,
            clock,
        })
    }

    async fn request_token(
        &self,
        form: &[(&str, &str)],
    ) -> Result<reqwest::Response, IssuerFailure> {
        self.client
            .post(self.token_url.as_str())
            .form(form)
            .send()
            .await
            .map_err(network_failure)
    }

    async fn read_token(
        &self,
        response: reqwest::Response,
    ) -> Result<IssuedMachineToken, IssuerFailure> {
        let wire: TokenResponse = response
            .json()
            .await
            .map_err(|_| IssuerFailure::InvalidResponse)?;
        if !wire.token_type.eq_ignore_ascii_case("bearer")
            || wire.expires_in == 0
            || wire.expires_in > 3600
            || !valid_secret(&wire.access_token)
            || !valid_secret(&wire.refresh_token)
        {
            return Err(IssuerFailure::InvalidResponse);
        }
        let now = self.clock.now();
        let valid_until =
            token_expiry(&wire.access_token)?.min(now.after(Duration::from_secs(wire.expires_in)));
        if valid_until <= now {
            return Err(IssuerFailure::InvalidResponse);
        }
        Ok(IssuedMachineToken {
            token: SecretString::new(wire.access_token),
            valid_until,
            credential: RefreshCredentialBundle {
                machine_client_id: wire.machine_client_id,
                refresh_credential: SecretString::new(wire.refresh_token),
            },
        })
    }
}

impl MachineTokenIssuer for HttpMachineTokenIssuer {
    fn authorize<'a>(
        &'a self,
        request: &'a DeviceLoginRequest,
    ) -> LoginFuture<'a, Result<DeviceCodeAuthorization, IssuerFailure>> {
        Box::pin(async move {
            let peer = request.peer.to_string();
            let form = [
                ("client_id", self.device_client_id.as_str()),
                ("scope", "offline_access"),
                ("resource", self.machine_resource.as_str()),
                ("machine_installation_id", request.installation.as_str()),
                ("machine_peer_id", peer.as_str()),
                ("machine_label", request.label.as_str()),
                ("organization_id", request.organization_id.as_str()),
            ];
            let response = self
                .client
                .post(self.device_authorization_url.as_str())
                .form(&form)
                .send()
                .await
                .map_err(network_failure)?;
            if !response.status().is_success() {
                return Err(IssuerFailure::Unavailable);
            }
            let wire: DeviceAuthorizationResponse = response
                .json()
                .await
                .map_err(|_| IssuerFailure::InvalidResponse)?;
            if !valid_secret(&wire.device_code) || wire.expires_in == 0 || wire.expires_in > 600 {
                return Err(IssuerFailure::InvalidResponse);
            }
            let user_code =
                UserCode::try_from(wire.user_code).map_err(|_| IssuerFailure::InvalidResponse)?;
            let verification_uri = VerificationUri::try_from(wire.verification_uri)
                .map_err(|_| IssuerFailure::InvalidResponse)?;
            if verification_uri.as_str().contains(&wire.device_code)
                || verification_uri.as_str().contains(user_code.as_str())
            {
                return Err(IssuerFailure::InvalidResponse);
            }
            // RFC 8628 section 3.2 defines five seconds when interval is omitted.
            let interval_seconds = wire.interval.unwrap_or(5);
            if interval_seconds == 0 {
                return Err(IssuerFailure::InvalidResponse);
            }
            Ok(DeviceCodeAuthorization {
                device_code: SecretString::new(wire.device_code),
                user_code,
                verification_uri,
                expires_at: self.clock.now().after(Duration::from_secs(wire.expires_in)),
                interval_seconds,
            })
        })
    }

    fn poll<'a>(
        &'a self,
        device_code: &'a SecretString,
    ) -> LoginFuture<'a, Result<DevicePollResult, IssuerFailure>> {
        Box::pin(async move {
            let response = self
                .request_token(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device_code.expose_secret()),
                    ("client_id", self.device_client_id.as_str()),
                    ("resource", self.machine_resource.as_str()),
                ])
                .await?;
            if response.status().is_success() {
                return self
                    .read_token(response)
                    .await
                    .map(DevicePollResult::Issued);
            }
            if response.status().is_server_error() {
                return Err(IssuerFailure::Unavailable);
            }
            match read_oauth_error(response).await? {
                OAuthError::AuthorizationPending => Ok(DevicePollResult::Pending),
                OAuthError::SlowDown => Ok(DevicePollResult::SlowDown),
                OAuthError::AccessDenied => Err(IssuerFailure::AccessDenied),
                OAuthError::ExpiredToken => Err(IssuerFailure::CodeExpired),
                OAuthError::InvalidGrant => Err(IssuerFailure::InvalidGrant),
                OAuthError::InvalidClient
                | OAuthError::InvalidRequest
                | OAuthError::InvalidScope
                | OAuthError::UnsupportedGrantType => Err(IssuerFailure::InvalidResponse),
            }
        })
    }

    fn refresh<'a>(
        &'a self,
        credential: &'a RefreshCredentialBundle,
    ) -> LoginFuture<'a, Result<IssuedMachineToken, IssuerFailure>> {
        Box::pin(async move {
            let response = self
                .request_token(&[
                    ("grant_type", "refresh_token"),
                    (
                        "refresh_token",
                        credential.refresh_credential.expose_secret(),
                    ),
                    ("client_id", credential.machine_client_id.as_str()),
                    ("resource", self.machine_resource.as_str()),
                ])
                .await?;
            if response.status().is_success() {
                let token = self.read_token(response).await?;
                if token.credential.machine_client_id != credential.machine_client_id {
                    return Err(IssuerFailure::InvalidResponse);
                }
                return Ok(token);
            }
            if response.status().is_server_error() {
                return Err(IssuerFailure::Unavailable);
            }
            match read_oauth_error(response).await? {
                OAuthError::InvalidGrant | OAuthError::AccessDenied | OAuthError::ExpiredToken => {
                    Err(IssuerFailure::InvalidGrant)
                }
                OAuthError::AuthorizationPending
                | OAuthError::SlowDown
                | OAuthError::InvalidClient
                | OAuthError::InvalidRequest
                | OAuthError::InvalidScope
                | OAuthError::UnsupportedGrantType => Err(IssuerFailure::InvalidResponse),
            }
        })
    }
}

fn valid_secret(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64 * 1024
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn network_failure(error: reqwest::Error) -> IssuerFailure {
    if error.is_timeout() {
        IssuerFailure::Timeout
    } else {
        IssuerFailure::Unavailable
    }
}

// These are partial OAuth responses: provider extension fields and registered
// JWT claims are intentionally ignored. They never grant admission at central.
#[derive(Deserialize)]
struct DeviceAuthorizationResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: u64,
    machine_client_id: EnrollmentId,
}
#[derive(Deserialize)]
struct TokenTimingClaims {
    exp: i64,
    grant_expires_at: i64,
}

fn token_expiry(token: &str) -> Result<SyncTime, IssuerFailure> {
    let mut parts = token.split('.');
    let (Some(_header), Some(payload), Some(_signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(IssuerFailure::InvalidResponse);
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| IssuerFailure::InvalidResponse)?;
    let timing: TokenTimingClaims =
        serde_json::from_slice(&bytes).map_err(|_| IssuerFailure::InvalidResponse)?;
    let milliseconds = timing
        .exp
        .min(timing.grant_expires_at)
        .checked_mul(1000)
        .ok_or(IssuerFailure::InvalidResponse)?;
    Ok(SyncTime::from_unix_millis(milliseconds))
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum OAuthError {
    AuthorizationPending,
    SlowDown,
    AccessDenied,
    ExpiredToken,
    InvalidGrant,
    InvalidClient,
    InvalidRequest,
    InvalidScope,
    UnsupportedGrantType,
}
#[derive(Deserialize)]
struct OAuthErrorResponse {
    error: OAuthError,
}
async fn read_oauth_error(response: reqwest::Response) -> Result<OAuthError, IssuerFailure> {
    response
        .json::<OAuthErrorResponse>()
        .await
        .map(|body| body.error)
        .map_err(|_| IssuerFailure::InvalidResponse)
}

#[cfg(test)]
#[path = "issuer_http_tests.rs"]
mod tests;
