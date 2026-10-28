//! Replacement configuration and point-in-time listener checks before teardown.

use std::net::{SocketAddr, TcpListener};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use crate::{HostConfig, HostPortOverrides, HostPortSource, PortConfigurationError, RemoteAccess};

#[derive(Debug, thiserror::Error)]
pub(crate) enum RestartPortPreflightError {
    #[error(transparent)]
    Configuration(#[from] PortConfigurationError),
    #[error("Host restart port preflight for {path}: cannot bind {endpoint} {address}: {source}")]
    Bind {
        path: PathBuf,
        endpoint: &'static str,
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("Host restart port preflight for {path} could not complete: {source}")]
    Worker {
        path: PathBuf,
        #[source]
        source: tokio::task::JoinError,
    },
}

pub(crate) async fn preflight_restart_ports(
    config: &HostConfig,
    held_router: Option<SocketAddr>,
    held_mcp: Option<SocketAddr>,
) -> Result<(), RestartPortPreflightError> {
    let root = config
        .coordination_paths()
        .router_root()
        .ok_or_else(|| PortConfigurationError::Invalid {
            path: config.coordination_paths().instance_lock().to_owned(),
            message: "the Host coordination path has no Router root".to_owned(),
        })?
        .to_owned();
    let path = root.join("ports.json");
    let config = config.clone();
    // File reads and bind probes stay off the async runtime's worker threads.
    tokio::task::spawn_blocking(move || {
        check_replacement_ports(&root, &config, held_router, held_mcp)
    })
    .await
    .map_err(|source| RestartPortPreflightError::Worker { path, source })?
}

fn check_replacement_ports(
    root: &Path,
    config: &HostConfig,
    held_router: Option<SocketAddr>,
    held_mcp: Option<SocketAddr>,
) -> Result<(), RestartPortPreflightError> {
    let path = root.join("ports.json");
    let overrides = replacement_port_overrides(config, &path)?;
    let ports = crate::resolve_host_ports(root, config.isolated_debug_ports(), overrides)?;
    let mut router = config.router_endpoint();
    router.set_port(ports.selection().router.port);
    let held = [held_router, held_mcp];
    probe_new_binding(&path, "Router", router, &held)?;
    if config.port_selection().is_some()
        || config.collaboration_directory().is_some()
        || held_mcp.is_some()
    {
        probe_new_binding(&path, "MCP", ports.mcp_bind(), &held)?;
    }
    let held_https = config
        .remote_access()
        .map(|RemoteAccess::Tailscale { https_port }| https_port);
    if (config.port_selection().is_some() || held_https.is_some())
        && held_https != Some(ports.tailscale_https_port())
    {
        probe_new_binding(
            &path,
            "Tailscale HTTPS",
            SocketAddr::from(([127, 0, 0, 1], ports.tailscale_https_port().get())),
            &held,
        )?;
    }
    Ok(())
}

fn probe_new_binding(
    path: &Path,
    endpoint: &'static str,
    address: SocketAddr,
    held: &[Option<SocketAddr>],
) -> Result<(), RestartPortPreflightError> {
    if held.contains(&Some(address)) {
        return Ok(());
    }
    // This checks current availability; the requested bind-and-release is not a reservation.
    let listener =
        TcpListener::bind(address).map_err(|source| RestartPortPreflightError::Bind {
            path: path.to_owned(),
            endpoint,
            address,
            source,
        })?;
    drop(listener);
    Ok(())
}

fn replacement_port_overrides(
    config: &HostConfig,
    path: &Path,
) -> Result<HostPortOverrides, PortConfigurationError> {
    let Some(selection) = config.port_selection() else {
        // Programmatic HostConfig inputs are explicit endpoints, not file/default selections.
        return Ok(HostPortOverrides {
            router: Some(config.router_endpoint().port()),
            mcp_bind: Some(config.mcp_bind()),
            tailscale_https: config
                .remote_access()
                .map(|RemoteAccess::Tailscale { https_port }| https_port),
        });
    };
    let tailscale_https = if selection.tailscale_https.source == HostPortSource::Flag {
        Some(
            NonZeroU16::new(selection.tailscale_https.port).ok_or_else(|| {
                PortConfigurationError::Invalid {
                    path: path.to_owned(),
                    message: "explicit Tailscale HTTPS port must not be zero".to_owned(),
                }
            })?,
        )
    } else {
        None
    };
    Ok(HostPortOverrides {
        router: (selection.router.source == HostPortSource::Flag).then_some(selection.router.port),
        mcp_bind: (selection.mcp.source == HostPortSource::Flag).then_some(config.mcp_bind()),
        tailscale_https,
    })
}
