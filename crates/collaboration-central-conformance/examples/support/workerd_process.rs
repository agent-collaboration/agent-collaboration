use collaboration_central_conformance::ConformanceResult;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
};

pub struct WorkerdSettings {
    pub node: PathBuf,
    /// The pinned Miniflare package's entrypoint (`dist/src/index.js`).
    pub miniflare: PathBuf,
    pub persistence: PathBuf,
    pub port: u16,
    pub jwks_url: String,
    pub holder_answer_timeout: Duration,
    pub copy_ack_wait: Duration,
    pub relay_peer: fleet_sync_protocol::PeerId,
}
pub struct WorkerdProcess {
    child: Child,
    output: JoinHandle<ConformanceResult<()>>,
}
impl WorkerdProcess {
    pub async fn start(settings: &WorkerdSettings) -> ConformanceResult<Self> {
        Self::start_with_relay_value(settings, &settings.relay_peer.to_string()).await
    }
    /// Supply an invalid fixture value to exercise the real Worker's config decoder.
    pub async fn start_with_relay_value(
        settings: &WorkerdSettings,
        relay: &str,
    ) -> ConformanceResult<Self> {
        Self::start_configured(settings, relay, None).await
    }
    /// An optional debug adapter is a separate module in the local harness.
    pub async fn start_configured(
        settings: &WorkerdSettings,
        relay: &str,
        clock_bundle: Option<&std::path::Path>,
    ) -> ConformanceResult<Self> {
        let mut command = Command::new(&settings.node);
        command
            .arg("scripts/tooling/run-central-workerd.mjs")
            .arg(&settings.miniflare)
            .arg(&settings.persistence)
            .arg(settings.port.to_string())
            .arg(&settings.jwks_url)
            .arg(serde_json::to_string(&settings.holder_answer_timeout)?)
            .arg(serde_json::to_string(&settings.copy_ack_wait)?)
            .arg(relay);
        if let Some(bundle) = clock_bundle {
            command.arg(bundle);
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let mut lines =
            BufReader::new(child.stdout.take().ok_or("Miniflare harness stdout")?).lines();
        // Observe the platform's own ready event. No sleep-based readiness or
        // request polling can hide a startup failure.
        tokio::time::timeout(Duration::from_secs(60), async {
            while let Some(line) = lines.next_line().await? {
                println!("{line}");
                if line.contains("Ready on") {
                    return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
                }
            }
            Err("Miniflare harness ended before ready".into())
        })
        .await??;
        let process_id = child.id().ok_or("Miniflare harness exited before ready")?;
        println!("WORKERD owned process identity started pid={process_id}");
        let output = tokio::spawn(async move {
            while let Some(line) = lines.next_line().await? {
                println!("{line}");
            }
            Ok(())
        });
        Ok(Self { child, output })
    }
    pub async fn stop(mut self) -> ConformanceResult<()> {
        let pid = self.child.id().ok_or("Miniflare harness already exited")?;
        // Signal only the process this harness owns; Miniflare then shuts down
        // its own workerd child and closes the bound port before the restart.
        let signal = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .await?;
        if !signal.success() {
            return Err("could not stop the owned Miniflare harness".into());
        }
        let status = tokio::time::timeout(Duration::from_secs(30), self.child.wait()).await??;
        tokio::time::timeout(Duration::from_secs(10), self.output).await???;
        println!(
            "WORKERD owned process identity reaped pid={pid} exit_code={:?}",
            status.code()
        );
        // Miniflare installs its SIGINT exit hook before the script's handler;
        // its normal requested-interrupt path exits 128+SIGINT rather than zero.
        if !matches!(status.code(), Some(0 | 130)) {
            return Err(format!("owned Miniflare harness exited unsuccessfully: {status}").into());
        }
        println!("WORKERD owned process stopped and reaped");
        Ok(())
    }
}
