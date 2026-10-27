use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use crate::driver_error::{DriverError, DriverResult};
use crate::process_identity::ProcessIdentity;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpsEndpoint {
    pub url: String,
}

impl HttpsEndpoint {
    pub fn parsed(&self) -> DriverResult<reqwest::Url> {
        let parsed = reqwest::Url::parse(&self.url)
            .map_err(|_| DriverError::Invalid("expected private ts.net HTTPS /mcp URL"))?;
        if parsed.scheme() != "https"
            || !parsed
                .host_str()
                .is_some_and(|host| host.ends_with(".ts.net"))
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/mcp"
            || !literal_mcp_path(&self.url)
            || parsed.port() == Some(0)
        {
            return Err(DriverError::Invalid(
                "expected private ts.net HTTPS /mcp URL",
            ));
        }
        Ok(parsed)
    }
    pub fn authority(&self) -> DriverResult<String> {
        let parsed = self.parsed()?;
        Ok(format!(
            "{}:{}",
            parsed
                .host_str()
                .ok_or(DriverError::Invalid("HTTPS host absent"))?,
            parsed
                .port_or_known_default()
                .ok_or(DriverError::Invalid("HTTPS port absent"))?
        ))
    }
    pub fn origin(&self) -> DriverResult<String> {
        Ok(format!("https://{}", self.authority()?))
    }
}

#[must_use]
pub fn literal_mcp_path(value: &str) -> bool {
    let normalized = value.replace(['\t', '\r', '\n'], "");
    normalized
        .split_once("://")
        .and_then(|(_, tail)| tail.find('/').and_then(|start| tail.get(start..)))
        .and_then(|path| path.split(['?', '#']).next())
        == Some("/mcp")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPlan {
    pub repository: PathBuf,
    pub source_head: String,
    pub qualification: PathBuf,
    pub qualification_sha256: String,
    pub expected_version: String,
    pub router_root: PathBuf,
    pub port: u16,
    #[serde(default = "default_https_port")]
    pub https_port: u16,
    pub https: HttpsEndpoint,
    pub profile_sha256: String,
    pub tailscale: PathBuf,
    pub deadline: DateTime<FixedOffset>,
    pub protected: Vec<ProcessIdentity>,
}

const fn default_https_port() -> u16 {
    8443
}
pub fn require_deadline(deadline: &DateTime<FixedOffset>) -> DriverResult<()> {
    if *deadline <= Utc::now() {
        return Err(DriverError::Invalid(
            "allocation deadline is absent, naive or expired",
        ));
    }
    Ok(())
}
fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
pub fn validate_ports(port: u16, https_port: u16) -> DriverResult<()> {
    if port == 0 || https_port == 0 || [19741, 18787, 43127].contains(&port) || port == https_port {
        return Err(DriverError::Invalid("unsafe or conflicting selected ports"));
    }
    Ok(())
}
impl HostPlan {
    pub fn validate(&self) -> DriverResult<()> {
        validate_ports(self.port, self.https_port)?;
        require_deadline(&self.deadline)?;
        if self.https.parsed()?.port_or_known_default() != Some(self.https_port) {
            return Err(DriverError::Invalid("HTTPS URL and selected port differ"));
        }
        if !lowercase_hex(&self.source_head, 40)
            || !lowercase_hex(&self.qualification_sha256, 64)
            || !lowercase_hex(&self.profile_sha256, 64)
            || self.protected.is_empty()
        {
            return Err(DriverError::Invalid(
                "invalid Host qualification, source or protected processes",
            ));
        }
        for path in [
            &self.repository,
            &self.qualification,
            &self.router_root,
            &self.tailscale,
        ] {
            if !path.is_absolute()
                || path
                    .components()
                    .any(|component| component == Component::ParentDir)
            {
                return Err(DriverError::Invalid(
                    "plan paths must be absolute without parent traversal",
                ));
            }
        }
        Ok(())
    }
    #[must_use]
    pub fn evidence(&self) -> PathBuf {
        self.router_root.join("evidence")
    }
    #[must_use]
    pub fn service(&self) -> PathBuf {
        self.router_root.join("agent-communication")
    }
}

pub fn validate_fresh_root(root: &Path) -> DriverResult<()> {
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if root.parent() != Some(Path::new("/private/tmp"))
        || root.symlink_metadata().is_ok()
        || name.is_empty()
        || name.len() > 48
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(DriverError::Invalid(
            "fresh root must be a new direct /private/tmp child",
        ));
    }
    Ok(())
}
