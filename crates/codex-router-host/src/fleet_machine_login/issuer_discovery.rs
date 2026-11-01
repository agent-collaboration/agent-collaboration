//! RFC8414 metadata locates OAuth endpoints; the closed Fleet document names only its public client.
use codex_router_fleet_state::fleet_login::SignInAddress;
use serde::{Deserialize, Serialize};

use super::device_code_flow::IssuerFailure;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(super) struct IssuerEndpoint(String, IssuerEndpointTransport);

#[derive(Clone, Copy, Debug)]
enum IssuerEndpointTransport {
    Https,
    LoopbackHttp,
}
impl TryFrom<String> for IssuerEndpoint {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let url = reqwest::Url::parse(&value).map_err(|_| "invalid issuer endpoint")?;
        if value.len() > 4096
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err("invalid issuer endpoint");
        }
        let transport = if url.scheme() == "https" {
            IssuerEndpointTransport::Https
        } else if url.scheme() == "http"
            && matches!(
                url.host_str().map(|host| host.trim_matches(['[', ']'])),
                Some("localhost" | "127.0.0.1" | "::1")
            )
        {
            IssuerEndpointTransport::LoopbackHttp
        } else {
            return Err("invalid issuer endpoint");
        };
        Ok(Self(value, transport))
    }
}
impl From<IssuerEndpoint> for String {
    fn from(value: IssuerEndpoint) -> Self {
        value.0
    }
}
impl IssuerEndpoint {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(super) struct DeviceLoginClientId(String);
impl TryFrom<String> for DeviceLoginClientId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 256
            || value.trim().is_empty()
            || value.chars().any(char::is_control)
        {
            return Err("invalid device-login client ID");
        }
        Ok(Self(value))
    }
}
impl From<DeviceLoginClientId> for String {
    fn from(value: DeviceLoginClientId) -> Self {
        value.0
    }
}
impl DeviceLoginClientId {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(super) struct DiscoveredIssuer {
    pub device_authorization_endpoint: IssuerEndpoint,
    pub token_endpoint: IssuerEndpoint,
    pub client_id: DeviceLoginClientId,
}

// OAuth metadata is extensible. Only the fields this client consumes are decoded;
// the Fleet-specific companion document is closed, as its C8 contract requires.
#[derive(Deserialize)]
struct AuthorizationServerMetadata {
    issuer: SignInAddress,
    device_authorization_endpoint: IssuerEndpoint,
    token_endpoint: IssuerEndpoint,
}

impl AuthorizationServerMetadata {
    fn validate_for_issuer(&self, sign_in: &SignInAddress) -> Result<(), IssuerFailure> {
        // The canonical sign-in constructor permits HTTP only on loopback.
        let issuer_uses_loopback_http = sign_in.as_str().starts_with("http://");
        let endpoint_uses_loopback_http =
            [&self.device_authorization_endpoint, &self.token_endpoint]
                .iter()
                .any(|endpoint| matches!(endpoint.1, IssuerEndpointTransport::LoopbackHttp));
        if self.issuer != *sign_in || (endpoint_uses_loopback_http && !issuer_uses_loopback_http) {
            return Err(IssuerFailure::InvalidResponse);
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FleetLoginDocument {
    client_id: DeviceLoginClientId,
}

pub(super) fn metadata_address(sign_in: &SignInAddress) -> Result<reqwest::Url, IssuerFailure> {
    let mut metadata =
        reqwest::Url::parse(sign_in.as_str()).map_err(|_| IssuerFailure::InvalidResponse)?;
    let issuer_path = metadata.path().trim_end_matches('/').to_owned();
    metadata.set_path(&format!(
        "/.well-known/oauth-authorization-server{issuer_path}"
    ));
    Ok(metadata)
}

fn origin_address(sign_in: &SignInAddress, path: &str) -> Result<reqwest::Url, IssuerFailure> {
    let mut address =
        reqwest::Url::parse(sign_in.as_str()).map_err(|_| IssuerFailure::InvalidResponse)?;
    address.set_path(path);
    Ok(address)
}

pub(super) fn session_address(sign_in: &SignInAddress) -> Result<reqwest::Url, IssuerFailure> {
    origin_address(sign_in, "/fleet/v1/session")
}

pub(super) async fn discover_issuer(
    client: &reqwest::Client,
    sign_in: &SignInAddress,
) -> Result<DiscoveredIssuer, IssuerFailure> {
    let metadata = client
        .get(metadata_address(sign_in)?)
        .send()
        .await
        .map_err(|_| IssuerFailure::Unavailable)?;
    if !metadata.status().is_success() {
        return Err(IssuerFailure::Unavailable);
    }
    let metadata: AuthorizationServerMetadata = metadata
        .json()
        .await
        .map_err(|_| IssuerFailure::InvalidResponse)?;
    // RFC8414 section3.3 forbids using metadata for another issuer, even if its
    // endpoints would be reachable. No fallback document or default URL exists.
    metadata.validate_for_issuer(sign_in)?;
    let document = client
        .get(origin_address(sign_in, "/.well-known/fleet-login")?)
        .send()
        .await
        .map_err(|_| IssuerFailure::Unavailable)?;
    if !document.status().is_success() {
        return Err(IssuerFailure::Unavailable);
    }
    let document: FleetLoginDocument = document
        .json()
        .await
        .map_err(|_| IssuerFailure::InvalidResponse)?;
    Ok(DiscoveredIssuer {
        device_authorization_endpoint: metadata.device_authorization_endpoint,
        token_endpoint: metadata.token_endpoint,
        client_id: document.client_id,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        AuthorizationServerMetadata, FleetLoginDocument, IssuerEndpoint, metadata_address,
        origin_address, session_address,
    };

    // Wrong behavior: a remote HTTPS issuer downgrades OAuth to HTTP, or local
    // metadata directs plaintext credentials off loopback. Oracle: PD 426a6cc7.
    // No pre-fix red observed; this is metadata-boundary coverage, not HTTP proof.
    #[test]
    fn fleet_machine_login_oauth_http_endpoints_require_a_loopback_http_issuer() {
        // Arrange
        for (issuer, device, token, expected) in [
            (
                "http://localhost:8787/api/auth",
                "http://localhost:8787/device?client=router",
                "http://[::1]:8787/token",
                true,
            ),
            (
                "http://127.0.0.1:8787",
                "https://issuer.example.invalid/device",
                "https://issuer.example.invalid/token",
                true,
            ),
            (
                "https://issuer.example.invalid",
                "http://localhost/device",
                "https://issuer.example.invalid/token",
                false,
            ),
            (
                "https://localhost",
                "https://localhost/device",
                "HTTP://localhost/token",
                false,
            ),
            (
                "https://issuer.example.invalid",
                "http://issuer.example.invalid/device",
                "https://issuer.example.invalid/token",
                false,
            ),
            (
                "http://localhost:8787",
                "http://remote.example.invalid/device",
                "http://localhost:8787/token",
                false,
            ),
        ] {
            let sign_in = issuer.to_owned().try_into().unwrap();
            let wire = serde_json::json!({
                "issuer": issuer,
                "device_authorization_endpoint": device,
                "token_endpoint": token,
            });

            // Act
            let metadata = serde_json::from_value::<AuthorizationServerMetadata>(wire);
            let accepted =
                metadata.is_ok_and(|metadata| metadata.validate_for_issuer(&sign_in).is_ok());

            // Assert
            assert_eq!(
                accepted, expected,
                "issuer={issuer}, device={device}, token={token}"
            );
        }
        for endpoint in [
            "http://user:secret@localhost/token",
            "http://localhost/token#fragment",
        ] {
            assert!(IssuerEndpoint::try_from(endpoint.to_owned()).is_err());
        }
    }

    // Wrong behavior: appending the issuer's path to Fleet routes. Oracle:
    // PD 19f8ff2b pins Fleet at the origin; only metadata uses the issuer path.
    // No pre-fix red observed for this newly specified derivation.
    #[test]
    fn fleet_machine_login_fleet_routes_ignore_the_issuer_path_and_trailing_slash() {
        // Arrange
        for (issuer, origin) in [
            (
                "https://sign-in.example.invalid",
                "https://sign-in.example.invalid",
            ),
            (
                "https://sign-in.example.invalid/",
                "https://sign-in.example.invalid",
            ),
            (
                "https://sign-in.example.invalid/api/auth",
                "https://sign-in.example.invalid",
            ),
            (
                "https://sign-in.example.invalid/api/auth/",
                "https://sign-in.example.invalid",
            ),
            ("http://localhost:8787/api/auth/", "http://localhost:8787"),
            ("http://127.0.0.1:8787/api/auth", "http://127.0.0.1:8787"),
            ("http://[::1]:8787/api/auth/", "http://[::1]:8787"),
        ] {
            let sign_in = issuer.to_owned().try_into().unwrap();

            // Act
            let session = session_address(&sign_in).unwrap();
            let document = origin_address(&sign_in, "/.well-known/fleet-login").unwrap();

            // Assert
            assert_eq!(session.as_str(), format!("{origin}/fleet/v1/session"));
            assert_eq!(
                document.as_str(),
                format!("{origin}/.well-known/fleet-login")
            );
            assert_eq!(sign_in.as_str(), issuer);
        }
    }
    #[test]
    fn fleet_machine_login_discovery_uses_path_insertion_and_a_closed_public_client_document() {
        let issuer = "https://issuer.example.invalid/api/auth"
            .to_owned()
            .try_into()
            .unwrap();
        assert_eq!(
            metadata_address(&issuer).unwrap().as_str(),
            "https://issuer.example.invalid/.well-known/oauth-authorization-server/api/auth"
        );
        assert!(
            serde_json::from_value::<FleetLoginDocument>(
                serde_json::json!({"client_id":"fixture-client","unexpected":true})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<FleetLoginDocument>(
                serde_json::json!({"client_id":"fixture-client"})
            )
            .is_ok()
        );
        assert!(
            IssuerEndpoint::try_from("http://issuer.example.invalid/token".to_owned()).is_err()
        );
    }
}
