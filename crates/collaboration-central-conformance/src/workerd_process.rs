use crate::ConformanceResult;
use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
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
    child: Option<Child>,
    output: Option<std::thread::JoinHandle<ConformanceResult<()>>>,
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
        let child = Command::new(&settings.node)
            .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .arg("scripts/tooling/run-central-workerd.mjs")
            .arg(&settings.miniflare)
            .arg(&settings.persistence)
            .arg(settings.port.to_string())
            .arg(&settings.jwks_url)
            .arg(serde_json::to_string(&settings.holder_answer_timeout)?)
            .arg(serde_json::to_string(&settings.copy_ack_wait)?)
            .arg(relay)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .stdin(Stdio::piped())
            .spawn()?;
        let mut owned = Self {
            child: Some(child),
            output: None,
        };
        let stdout = owned
            .child
            .as_mut()
            .and_then(|child| child.stdout.take())
            .ok_or("Miniflare harness stdout")?;
        let (ready_sender, ready) = tokio::sync::oneshot::channel();
        owned.output = Some(std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            let mut ready_sender = Some(ready_sender);
            for line in &mut lines {
                let line = line?;
                println!("{line}");
                if line.contains("Ready on")
                    && let Some(sender) = ready_sender.take()
                {
                    let _ = sender.send(());
                }
            }
            Ok(())
        }));
        ready
            .await
            .map_err(|_| "Miniflare harness ended before ready")?;
        let process_id = owned.child.as_ref().ok_or("Miniflare harness exited")?.id();
        println!("WORKERD owned process identity started pid={process_id}");
        Ok(owned)
    }
    pub async fn stop(self) -> ConformanceResult<()> {
        tokio::task::spawn_blocking(move || {
            let mut owned = self;
            owned.settle()
        })
        .await?
    }
    fn settle(&mut self) -> ConformanceResult<()> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        let pid = child.id();
        drop(child.stdin.take());
        let status = child.wait()?;
        if let Some(output) = self.output.take() {
            output
                .join()
                .map_err(|_| "Miniflare output reader panicked")??;
        }
        println!(
            "WORKERD owned process identity reaped pid={pid} exit_code={:?}",
            status.code()
        );
        if !matches!(status.code(), Some(0 | 130)) {
            return Err(format!("owned Miniflare harness exited unsuccessfully: {status}").into());
        }
        Ok(())
    }
}
impl Drop for WorkerdProcess {
    fn drop(&mut self) {
        if let Err(error) = self.settle() {
            assert!(
                std::thread::panicking(),
                "Miniflare harness cleanup failed: {error}"
            );
        }
    }
}
