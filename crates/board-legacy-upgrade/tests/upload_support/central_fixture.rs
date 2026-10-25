//! Real local central configuration shared by importer route tests.
use collaboration_central_conformance::{JwksFixtureServer, fixture_policy, owner_assertion};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::ReceiverId;
use reqwest::header::{HeaderMap, HeaderValue};
use std::{net::SocketAddr, path::Path, time::Duration};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[allow(
    dead_code,
    reason = "Publication and restart integration tests share this stopped-owner fixture."
)]
pub struct RestartableCentral {
    root: tempfile::TempDir,
    jwks: Option<JwksFixtureServer>,
    server: Option<TokioServer>,
    listen: SocketAddr,
}

#[allow(
    dead_code,
    reason = "Some importer test targets only need the original one-start fixture."
)]
impl RestartableCentral {
    pub async fn start() -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        let jwks = JwksFixtureServer::start()
            .await
            .map_err(|error| error as Box<dyn std::error::Error>)?;
        let server = match start_server(root.path(), &jwks).await {
            Ok(server) => server,
            Err(error) => {
                let cleanup = jwks.shutdown().await;
                let detail = match cleanup {
                    Ok(()) => format!("central startup failed: {error}"),
                    Err(cleanup) => {
                        format!("central startup failed: {error}; JWKS cleanup failed: {cleanup}")
                    }
                };
                return Err(std::io::Error::other(detail).into());
            }
        };
        let listen = match listen_address(server.base_url()) {
            Ok(address) => address,
            Err(error) => {
                let central_cleanup = server.shutdown().await;
                let jwks_cleanup = jwks.shutdown().await;
                let detail = format!(
                    "central address parsing failed: {error}; central cleanup: {}; JWKS cleanup: {}",
                    central_cleanup.map_or_else(|cleanup| cleanup.to_string(), |_| "ok".to_owned()),
                    jwks_cleanup.map_or_else(|cleanup| cleanup.to_string(), |_| "ok".to_owned()),
                );
                return Err(std::io::Error::other(detail).into());
            }
        };
        Ok(Self {
            root,
            jwks: Some(jwks),
            server: Some(server),
            listen,
        })
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn base_url(&self) -> TestResult<&str> {
        self.server
            .as_ref()
            .map(TokioServer::base_url)
            .ok_or_else(|| std::io::Error::other("central owner is stopped").into())
    }

    pub async fn stop_owner(&mut self) -> TestResult {
        let server = self
            .server
            .take()
            .ok_or_else(|| std::io::Error::other("central owner already stopped"))?;
        server.shutdown().await?;
        Ok(())
    }

    pub async fn reopen_owner(&mut self) -> TestResult<String> {
        if self.server.is_some() {
            return Err(std::io::Error::other("central owner is already live").into());
        }
        let jwks = self
            .jwks
            .as_ref()
            .ok_or_else(|| std::io::Error::other("JWKS fixture is stopped"))?;
        let server = start_server_on(self.root.path(), jwks, self.listen).await?;
        let endpoint = server.base_url().to_owned();
        self.server = Some(server);
        Ok(endpoint)
    }

    pub async fn shutdown(mut self) -> TestResult {
        let mut failures = Vec::new();
        if let Some(server) = self.server.take()
            && let Err(error) = server.shutdown().await
        {
            failures.push(format!("central cleanup failed: {error}"));
        }
        if let Some(jwks) = self.jwks.take()
            && let Err(error) = jwks.shutdown().await
        {
            failures.push(format!("JWKS cleanup failed: {error}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(std::io::Error::other(failures.join("; ")).into())
        }
    }
}

fn listen_address(base_url: &str) -> TestResult<SocketAddr> {
    Ok(base_url
        .strip_prefix("http://")
        .ok_or_else(|| std::io::Error::other("central fixture did not bind HTTP"))?
        .parse()?)
}

pub async fn start_server(
    root: &std::path::Path,
    jwks: &JwksFixtureServer,
) -> TestResult<TokioServer> {
    Ok(TokioServer::start(settings(root, jwks)?).await?)
}

pub async fn start_server_on(
    root: &std::path::Path,
    jwks: &JwksFixtureServer,
    listen: SocketAddr,
) -> TestResult<TokioServer> {
    Ok(TokioServer::start_on(settings(root, jwks)?, listen).await?)
}

fn settings(root: &std::path::Path, jwks: &JwksFixtureServer) -> TestResult<TokioSettings> {
    Ok(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: RegistryConfig {
            lease: LeasePolicy {
                lease_duration: Duration::from_secs(14_400),
                renew_interval: Duration::from_secs(900),
            },
            authoritative_receiver: ReceiverId::try_from(
                "019f0000-0000-7000-8000-000000000010".to_owned(),
            )?,
            release_unsent_for: Duration::ZERO,
            move_needs_renewal_within: Some(Duration::from_secs(1800)),
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}
pub fn owner_client() -> TestResult<reqwest::Client> {
    let mut headers = HeaderMap::new();
    let mut assertion = HeaderValue::from_str(
        &owner_assertion().map_err(|error| error as Box<dyn std::error::Error>)?,
    )?;
    assertion.set_sensitive(true);
    headers.insert("Cf-Access-Jwt-Assertion", assertion);
    Ok(reqwest::Client::builder()
        .no_proxy()
        .default_headers(headers)
        .timeout(Duration::from_secs(10))
        .build()?)
}
