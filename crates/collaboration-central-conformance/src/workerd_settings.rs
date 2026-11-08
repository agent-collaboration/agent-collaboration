//! Pure composition shared by timed legacy probes and event-only proofs.
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::process::Command;
pub struct WorkerdSettings {
    pub node: PathBuf,
    pub legacy_organization: fleet_identity_protocol::OrganizationId,
    /// The pinned Miniflare package entrypoint (`dist/src/index.js`).
    pub miniflare: PathBuf,
    pub persistence: PathBuf,
    pub port: u16,
    pub jwks_url: String,
    pub holder_answer_timeout: Duration,
    pub copy_ack_wait: Duration,
    pub relay_peer: fleet_sync_protocol::PeerId,
    pub fixed_access_time: Option<i64>,
}
impl WorkerdSettings {
    pub fn command(&self, relay: &str) -> crate::ConformanceResult<Command> {
        let mut command = Command::new(&self.node);
        command
            .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .arg("scripts/tooling/run-central-workerd.ts")
            .arg(&self.miniflare)
            .arg(&self.persistence)
            .arg(self.port.to_string())
            .arg(&self.jwks_url)
            .arg(serde_json::to_string(&self.holder_answer_timeout)?)
            .arg(serde_json::to_string(&self.copy_ack_wait)?)
            .arg(relay)
            .arg(self.legacy_organization.as_str())
            .arg(
                self.fixed_access_time
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        Ok(command)
    }
}
