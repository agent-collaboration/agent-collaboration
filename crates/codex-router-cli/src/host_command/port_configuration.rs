//! Host-owned port resolution, before any startup effects.

use std::{
    net::SocketAddr,
    num::NonZeroU16,
    path::{Path, PathBuf},
};

use codex_router_host::{HostPortSelection, HostPortSource, SelectedHostPort};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct HostPortOverrides {
    pub router: Option<u16>,
    pub mcp_bind: Option<SocketAddr>,
    pub tailscale_https: Option<NonZeroU16>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ResolvedHostPorts {
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

pub(super) fn resolve_host_ports(
    router_root: &Path,
    isolated_debug: bool,
    overrides: HostPortOverrides,
) -> Result<ResolvedHostPorts, PortConfigurationError> {
    let path = router_root.join("ports.json");
    let file = PortConfigurationFile::load(&path)?;
    ResolvedHostPorts::new(&path, isolated_debug, overrides, file)
}

impl ResolvedHostPorts {
    pub(super) const fn selection(self) -> HostPortSelection {
        self.selection
    }

    pub(super) const fn mcp_bind(self) -> SocketAddr {
        self.mcp_bind
    }

    pub(super) const fn tailscale_https_port(self) -> NonZeroU16 {
        self.tailscale_https_port
    }

    fn new(
        path: &Path,
        isolated_debug: bool,
        overrides: HostPortOverrides,
        file: PortConfigurationFile,
    ) -> Result<Self, PortConfigurationError> {
        let router = select_port(
            overrides.router,
            file.router,
            if isolated_debug {
                18787
            } else {
                codex_native_integration::DEFAULT_ROUTER_PORT
            },
        );
        let mcp = select_port(
            overrides.mcp_bind.map(|bind| bind.port()),
            file.mcp,
            super::default_mcp_bind(isolated_debug).port(),
        );
        let mcp_bind = overrides
            .mcp_bind
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], mcp.port)));
        super::parse_loopback_mcp_bind(&mcp_bind.to_string()).map_err(|message| {
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
            super::DEBUG_TAILSCALE_HTTPS_PORT
        } else {
            super::DEFAULT_TAILSCALE_HTTPS_PORT
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
