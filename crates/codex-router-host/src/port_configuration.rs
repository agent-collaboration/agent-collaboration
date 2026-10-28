//! Host-owned port resolution, before any startup effects.

use std::{
    net::SocketAddr,
    num::NonZeroU16,
    path::{Path, PathBuf},
};

use crate::{HostPortSelection, HostPortSource, SelectedHostPort};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostPortOverrides {
    pub router: Option<u16>,
    pub mcp_bind: Option<SocketAddr>,
    pub tailscale_https: Option<NonZeroU16>,
}

#[derive(Clone, Copy, Debug)]
pub struct ResolvedHostPorts {
    selection: HostPortSelection,
    mcp_bind: SocketAddr,
    tailscale_https_port: NonZeroU16,
}

#[derive(Debug, thiserror::Error)]
pub enum PortConfigurationError {
    #[error("cannot read Host port configuration {path}: {source}")]
    FileIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid Host port configuration {path}: {message}")]
    Invalid { path: PathBuf, message: String },
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PortConfigurationFile {
    router: Option<NonZeroU16>,
    mcp: Option<NonZeroU16>,
    tailscale_https: Option<NonZeroU16>,
}

impl PortConfigurationFile {
    fn load(path: &Path) -> Result<Self, PortConfigurationError> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(PortConfigurationError::FileIo {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        let configuration: Self =
            serde_json::from_slice(&bytes).map_err(|error| PortConfigurationError::Invalid {
                path: path.to_owned(),
                message: error.to_string(),
            })?;
        if configuration.router.is_some() && configuration.router == configuration.mcp {
            return Err(PortConfigurationError::Invalid {
                path: path.to_owned(),
                message: "router and MCP ports must differ".to_owned(),
            });
        }
        Ok(configuration)
    }
}

pub fn resolve_host_ports(
    router_root: &Path,
    isolated_debug: bool,
    overrides: HostPortOverrides,
) -> Result<ResolvedHostPorts, PortConfigurationError> {
    let path = router_root.join("ports.json");
    let file = PortConfigurationFile::load(&path)?;
    ResolvedHostPorts::new(&path, isolated_debug, overrides, file)
}

/// Resolves the Router endpoint through the same file loader and Router selector.
/// Standalone Serve does not launch MCP or Tailscale endpoints.
pub fn resolve_router_port(
    router_root: &Path,
    isolated_debug: bool,
    flag: Option<u16>,
) -> Result<SelectedHostPort, PortConfigurationError> {
    let path = router_root.join("ports.json");
    let file = PortConfigurationFile::load(&path)?;
    let router = select_router_port(flag, file.router, isolated_debug);
    if isolated_debug {
        validate_debug_port(&path, "Router", router.port)?;
    }
    Ok(router)
}

impl ResolvedHostPorts {
    pub const fn selection(self) -> HostPortSelection {
        self.selection
    }

    pub const fn mcp_bind(self) -> SocketAddr {
        self.mcp_bind
    }

    pub const fn tailscale_https_port(self) -> NonZeroU16 {
        self.tailscale_https_port
    }

    fn new(
        path: &Path,
        isolated_debug: bool,
        overrides: HostPortOverrides,
        file: PortConfigurationFile,
    ) -> Result<Self, PortConfigurationError> {
        let router = select_router_port(overrides.router, file.router, isolated_debug);
        let mcp = select_port(
            overrides.mcp_bind.map(|bind| bind.port()),
            file.mcp,
            default_mcp_bind(isolated_debug).port(),
        );
        let mcp_bind = overrides
            .mcp_bind
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], mcp.port)));
        parse_loopback_mcp_bind(&mcp_bind.to_string()).map_err(|message| {
            PortConfigurationError::Invalid {
                path: path.to_owned(),
                message,
            }
        })?;
        // Explicit port zero retains the CLI's ephemeral-listener behavior. File ports
        // are NonZeroU16, so two stored ports can never bypass the conflict check.
        if router.port != 0 && router.port == mcp.port {
            return Err(PortConfigurationError::Invalid {
                path: path.to_owned(),
                message: "router and MCP ports must differ".to_owned(),
            });
        }
        let https_default = if isolated_debug {
            DEBUG_TAILSCALE_HTTPS_PORT
        } else {
            DEFAULT_TAILSCALE_HTTPS_PORT
        };
        let tailscale_https_port = overrides
            .tailscale_https
            .or(file.tailscale_https)
            .unwrap_or(https_default);
        let tailscale_https = select_port(
            overrides.tailscale_https.map(NonZeroU16::get),
            file.tailscale_https,
            https_default.get(),
        );
        if isolated_debug {
            for (endpoint, selected) in [
                ("Router", router),
                ("MCP", mcp),
                ("Tailscale HTTPS", tailscale_https),
            ] {
                validate_debug_port(path, endpoint, selected.port)?;
            }
        }
        Ok(Self {
            selection: HostPortSelection {
                router,
                mcp,
                tailscale_https,
            },
            mcp_bind,
            tailscale_https_port,
        })
    }
}

fn select_router_port(
    flag: Option<u16>,
    file: Option<NonZeroU16>,
    isolated_debug: bool,
) -> SelectedHostPort {
    select_port(
        flag,
        file,
        if isolated_debug {
            18787
        } else {
            codex_native_integration::DEFAULT_ROUTER_PORT
        },
    )
}

fn validate_debug_port(
    path: &Path,
    endpoint: &str,
    port: u16,
) -> Result<(), PortConfigurationError> {
    if [8787, 8788, 19741, 19742, 19743, 43127].contains(&port) {
        return Err(PortConfigurationError::Invalid {
            path: path.to_owned(),
            message: format!("isolated debug {endpoint} cannot use protected port {port}"),
        });
    }
    Ok(())
}

fn select_port(flag: Option<u16>, file: Option<NonZeroU16>, default_port: u16) -> SelectedHostPort {
    match (flag, file) {
        (Some(port), _) => SelectedHostPort {
            port,
            source: HostPortSource::Flag,
        },
        (None, Some(port)) => SelectedHostPort {
            port: port.get(),
            source: HostPortSource::PortsFile,
        },
        (None, None) => SelectedHostPort {
            port: default_port,
            source: HostPortSource::Default,
        },
    }
}

const DEFAULT_TAILSCALE_HTTPS_PORT: NonZeroU16 =
    NonZeroU16::new(19743).expect("positive Tailscale HTTPS default");
const DEBUG_TAILSCALE_HTTPS_PORT: NonZeroU16 =
    NonZeroU16::new(18789).expect("positive debug Tailscale HTTPS default");

/// Returns the MCP default for the selected Host isolation policy.
#[must_use]
pub const fn default_mcp_bind(isolated_debug: bool) -> SocketAddr {
    SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        if isolated_debug { 18788 } else { 19742 },
    )
}

/// Parses the existing loopback-only MCP CLI input, including an explicit ephemeral port.
pub fn parse_loopback_mcp_bind(value: &str) -> Result<SocketAddr, String> {
    let address: SocketAddr = value
        .parse()
        .map_err(|_| "--mcp-bind requires HOST:PORT".to_owned())?;
    if !address.ip().is_loopback() {
        return Err("--mcp-bind requires a loopback address".to_owned());
    }
    Ok(address)
}
