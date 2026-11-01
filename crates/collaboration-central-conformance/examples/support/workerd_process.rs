use collaboration_central_conformance::ConformanceResult;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    process::{Child, ChildStdout, Command},
    sync::oneshot,
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
    stop_output: oneshot::Sender<oneshot::Sender<()>>,
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
        let mut child = Command::new(&settings.node)
            .arg("verification/tooling/run-central-workerd.ts")
            .arg(&settings.miniflare)
            .arg(&settings.persistence)
            .arg(settings.port.to_string())
            .arg(&settings.jwks_url)
            .arg(serde_json::to_string(&settings.holder_answer_timeout)?)
            .arg(serde_json::to_string(&settings.copy_ack_wait)?)
            .arg(relay)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let mut lines =
            BufReader::new(child.stdout.take().ok_or("Miniflare harness stdout")?).lines();
        // Observe the platform's own ready event. No sleep-based readiness or
        // request polling can hide a startup failure.
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                tokio::select! {
                    biased;
                    status = child.wait() => {
                        return Err::<(), Box<dyn std::error::Error + Send + Sync>>(
                            format!("Miniflare harness exited before ready: {}", status?).into(),
                        );
                    }
                    line = lines.next_line() => {
                        let Some(line) = line? else {
                            if let Some(status) = child.try_wait()? {
                                return Err(format!("Miniflare harness exited before ready: {status}").into());
                            }
                            return Err("Miniflare harness stdout EOF before ready".into());
                        };
                        println!("{line}");
                        if line.contains("Ready on") {
                            return Ok(());
                        }
                    }
                }
            }
        })
        .await??;
        let process_id = child.id().ok_or("Miniflare harness exited before ready")?;
        println!("WORKERD owned process identity started pid={process_id}");
        let (stop_output, stopped) = oneshot::channel();
        let output = tokio::spawn(drain_output(lines, stopped));
        Ok(Self {
            child,
            output,
            stop_output,
        })
    }
    pub async fn stop(mut self) -> ConformanceResult<()> {
        let pid = self.child.id().ok_or("Miniflare harness already exited")?;
        let status = tokio::time::timeout(Duration::from_secs(30), async {
            // Observe the output reader's stop transition before signalling
            // the process, so a normal shutdown's EOF cannot race that event.
            let (acknowledge, acknowledged) = oneshot::channel();
            if self.stop_output.send(acknowledge).is_ok() {
                let _ = acknowledged.await;
            }
            // Signal only this owned child. Always reap it, including when
            // the reader already failed; report the reader's error afterwards.
            let signal = Command::new("kill")
                .args(["-INT", &pid.to_string()])
                .status()
                .await?;
            if !signal.success() {
                return Err::<std::process::ExitStatus, Box<dyn std::error::Error + Send + Sync>>(
                    "could not stop the owned Miniflare harness".into(),
                );
            }
            Ok(self.child.wait().await?)
        })
        .await??;
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

async fn drain_output(
    mut lines: Lines<BufReader<ChildStdout>>,
    mut stopped: oneshot::Receiver<oneshot::Sender<()>>,
) -> ConformanceResult<()> {
    loop {
        tokio::select! {
            biased;
            line = lines.next_line() => {
                let Some(line) = line? else {
                    return Err("Miniflare harness stdout EOF before requested stop".into());
                };
                println!("{line}");
            }
            requested = &mut stopped => {
                let acknowledge = requested.map_err(|_| "Miniflare output lost its stop owner")?;
                let _ = acknowledge.send(());
                break;
            }
        }
    }
    while let Some(line) = lines.next_line().await? {
        println!("{line}");
    }
    Ok(())
}
