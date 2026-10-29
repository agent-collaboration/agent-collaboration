//! Startup port choices reported by the Host that actually uses them.

use serde::{Deserialize, Serialize};

/// Authority that selected one port at Host startup.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPortSource {
    Flag,
    #[serde(rename = "ports.json")]
    PortsFile,
    Default,
}

impl HostPortSource {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::PortsFile => "ports.json",
            Self::Default => "default",
        }
    }
}

/// One effective port and its startup authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SelectedHostPort {
    pub port: u16,
    pub source: HostPortSource,
}

/// Host port values with their startup authorities; file edits apply at the next whole-Host start.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostPortSelection {
    pub router: SelectedHostPort,
    pub mcp: SelectedHostPort,
    pub tailscale_https: SelectedHostPort,
}
