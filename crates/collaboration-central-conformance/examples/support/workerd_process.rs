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
}
pub struct WorkerdProcess {
    child: Child,
    output: JoinHandle<ConformanceResult<()>>,
    panic_observed: tokio::sync::watch::Receiver<bool>,
    central_lines: tokio::sync::watch::Receiver<Vec<String>>,
}
impl WorkerdProcess {
    pub async fn start(settings: &WorkerdSettings) -> ConformanceResult<Self> {
        let mut child = Command::new(&settings.node)
            .arg("scripts/tooling/run-central-workerd.mjs")
            .arg(&settings.miniflare)
            .arg(&settings.persistence)
            .arg(settings.port.to_string())
            .arg(&settings.jwks_url)
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
        let (panic_signal, panic_observed) = tokio::sync::watch::channel(false);
        let (central_signal, central_lines) = tokio::sync::watch::channel(Vec::new());
        let output = tokio::spawn(async move {
            while let Some(line) = lines.next_line().await? {
                if line
                    .contains("G18 deliberate panic after a completed SQL write inside transaction")
                {
                    let _ = panic_signal.send(true);
                }
                if let Some((_, rest)) = line.split_once("central ") {
                    let central = format!("central {rest}");
                    central_signal.send_modify(|lines| lines.push(central));
                }
                println!("{line}");
            }
            Ok(())
        });
        Ok(Self {
            child,
            output,
            panic_observed,
            central_lines,
        })
    }
    pub async fn await_panic(&mut self) -> ConformanceResult<()> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if *self.panic_observed.borrow() {
                    return Ok(());
                }
                self.panic_observed.changed().await?;
            }
        })
        .await?
    }
    /// Join the constructor observation for an object explicitly woken by the caller.
    pub async fn await_object_instance(&self, kind: &str) -> ConformanceResult<Vec<String>> {
        let prefix = format!("central object instance: kind={kind} instance=");
        let mut observed = self.central_lines.clone();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let lines = observed.borrow().clone();
                if lines.iter().any(|line| line.starts_with(&prefix)) {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(lines);
                }
                observed.changed().await?;
            }
        })
        .await?
    }
    /// The Worker's own `central …` log lines seen so far.
    pub fn central_lines(&self) -> Vec<String> {
        self.central_lines.borrow().clone()
    }
    /// Waits up to `bound` for a `central …` line after index `from` that
    /// `matches` accepts. Log output trails the response, so this waits on
    /// the output stream rather than reading once.
    pub async fn await_central_line(
        &mut self,
        from: usize,
        bound: Duration,
        matches: impl Fn(&str) -> bool,
    ) -> ConformanceResult<String> {
        tokio::time::timeout(bound, async {
            loop {
                if let Some(line) = self
                    .central_lines
                    .borrow()
                    .iter()
                    .skip(from)
                    .find(|line| matches(line))
                {
                    return Ok(line.clone());
                }
                self.central_lines.changed().await?;
            }
        })
        .await?
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
        tokio::time::timeout(Duration::from_secs(30), self.child.wait()).await??;
        tokio::time::timeout(Duration::from_secs(10), self.output).await???;
        println!("WORKERD owned process stopped and reaped");
        Ok(())
    }
}
