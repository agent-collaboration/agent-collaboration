//! Owned workerd readiness and finalization use only output and process events.
use crate::ConformanceResult;
pub use crate::workerd_settings::WorkerdSettings;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
};
pub struct EventWorkerdProcess {
    child: Child,
    output: JoinHandle<ConformanceResult<()>>,
}
impl EventWorkerdProcess {
    pub async fn start(settings: &WorkerdSettings) -> ConformanceResult<Self> {
        let mut child = settings
            .command(&settings.relay_peer.to_string())?
            .spawn()?;
        let mut lines =
            BufReader::new(child.stdout.take().ok_or("owned harness stdout missing")?).lines();
        let ready = format!(
            "Ready on http://127.0.0.1:{} (real workerd, local JWKS redirect)",
            settings.port
        );
        loop {
            let line = lines
                .next_line()
                .await?
                .ok_or("owned harness ended before readiness")?;
            println!("{line}");
            if line == ready {
                break;
            }
        }
        println!(
            "WORKERD event-only owned process started pid={}",
            child.id().ok_or("owned process exited before readiness")?
        );
        let output = tokio::spawn(async move {
            while let Some(line) = lines.next_line().await? {
                println!("{line}");
            }
            Ok(())
        });
        Ok(Self { child, output })
    }
    pub async fn stop(mut self) -> ConformanceResult<()> {
        let pid = self.child.id().ok_or("owned harness already exited")?;
        let signal = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .await?;
        if !signal.success() {
            return Err("could not signal the owned workerd harness".into());
        }
        let status = self.child.wait().await?;
        self.output.await??;
        if !matches!(status.code(), Some(0 | 130)) {
            return Err(format!("owned workerd harness exited: {status}").into());
        }
        println!(
            "WORKERD event-only owned process reaped pid={pid} exit={:?}",
            status.code()
        );
        Ok(())
    }
}
