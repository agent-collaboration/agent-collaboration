//! A separate native server process with explicit readiness and owned shutdown.
use collaboration_central_conformance::{
    ConformanceResult, RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation,
    owner_assertion,
};
use collaboration_central_core::ObjectAddress;
use serde_json::Value;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

pub const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440701";
pub const PEER: &str = "019f0000-0000-7000-8000-000000007011";
pub const CREDENTIAL: &str = "restart-reader";
// A killed drive retains the library's existing 30-second lease. This bound
// observes its ordinary expiry, without changing the clock or stored rows.
pub const RECOVERY_BOUND: Duration = Duration::from_secs(40);

pub struct ServerProcess {
    pub base: String,
    child: Child,
    output: tokio::task::JoinHandle<()>,
}
impl ServerProcess {
    pub async fn start(root: &Path, jwks: &str) -> ConformanceResult<Self> {
        Self::start_with_controls(root, jwks, &[]).await
    }

    pub async fn start_with_controls(
        root: &Path,
        jwks: &str,
        controls: &[RuntimeControlRequest],
    ) -> ConformanceResult<Self> {
        Self::start_configured(root, jwks, controls, None, &[]).await
    }

    pub async fn start_at(
        root: &Path,
        jwks: &str,
        now_millis: i64,
        startup_alarms: &[ObjectAddress],
    ) -> ConformanceResult<Self> {
        Self::start_configured(root, jwks, &[], Some(now_millis), startup_alarms).await
    }

    async fn start_configured(
        root: &Path,
        jwks: &str,
        controls: &[RuntimeControlRequest],
        now_millis: Option<i64>,
        startup_alarms: &[ObjectAddress],
    ) -> ConformanceResult<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .args([
                "--ignored",
                "--exact",
                "native_central_process",
                "--nocapture",
            ])
            .env("CENTRAL_RESTART_ROOT", root)
            .env("CENTRAL_RESTART_JWKS", jwks)
            .env("CENTRAL_RESTART_CONTROLS", serde_json::to_string(controls)?)
            .env(
                "CENTRAL_RESTART_NOW",
                now_millis.map_or_else(String::new, |now| now.to_string()),
            )
            .env(
                "CENTRAL_RESTART_ALARMS",
                serde_json::to_string(startup_alarms)?,
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().ok_or("server stdout missing")?;
        let mut lines = BufReader::new(stdout).lines();
        let ready = tokio::time::timeout(Duration::from_secs(10), async {
            let base = loop {
                let line = lines
                    .next_line()
                    .await?
                    .ok_or("native server exited before readiness")?;
                if let Some(base) = line.strip_prefix("CENTRAL_PROCESS_READY ") {
                    break base.to_owned();
                }
            };
            for expected in startup_alarms {
                loop {
                    let line = lines
                        .next_line()
                        .await?
                        .ok_or("native server exited before startup alarm completion")?;
                    if let Some(address) = line.strip_prefix("CENTRAL_STARTUP_ALARM_COMPLETED ") {
                        let actual: ObjectAddress = serde_json::from_str(address)?;
                        if &actual != expected {
                            return Err("startup alarm receipt named another object".into());
                        }
                        break;
                    }
                }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(base)
        })
        .await;
        let base = match ready {
            Ok(Ok(base)) => base,
            failed => {
                let killed = child.start_kill();
                let reaped = child.wait().await;
                return Err(format!("native startup receipt failed: {failed:?}; terminate={killed:?}; wait={reaped:?}").into());
            }
        };
        // Drain the test runner's small terminal output until the child exits.
        let output =
            tokio::spawn(async move { while matches!(lines.next_line().await, Ok(Some(_))) {} });
        eprintln!("CENTRAL_PROCESS_STARTED pid={:?} base={base}", child.id());
        Ok(Self {
            base,
            child,
            output,
        })
    }
    pub async fn crash(mut self) -> ConformanceResult<()> {
        use std::os::unix::process::ExitStatusExt;
        self.child.start_kill()?;
        let status = self.child.wait().await?;
        self.output.await?;
        if status.signal() != Some(9) {
            return Err(
                format!("native server was not killed at the crash barrier: {status}").into(),
            );
        }
        Ok(())
    }
    pub async fn stop(mut self) -> ConformanceResult<()> {
        self.child
            .stdin
            .take()
            .ok_or("server stdin missing")?
            .write_all(b"stop\n")
            .await?;
        let status = tokio::time::timeout(Duration::from_secs(10), self.child.wait()).await??;
        self.output.await?;
        if !status.success() {
            return Err(format!("native server shutdown failed: {status}").into());
        }
        Ok(())
    }
}

pub async fn control(
    base: &str,
    address: ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeFaultObservation> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest { address, command })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
pub async fn post(base: &str, route: &str, body: &Value) -> ConformanceResult<Value> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}{route}"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
pub async fn directory(client: &reqwest::Client, base: &str) -> ConformanceResult<Value> {
    Ok(client
        .get(format!("{base}/admin/v1/copies?limit=25"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub async fn read_count(path: &Path, sql: &'static str) -> ConformanceResult<i64> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    let result = query::<sqlx_turso::Turso>(sql)
        .fetch_one(&mut connection)
        .await?
        .try_get("count")?;
    connection.close().await?;
    Ok(result)
}
pub async fn operation(
    path: &Path,
    identity: &Value,
) -> ConformanceResult<(String, Option<Vec<u8>>, i64)> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    let id = identity
        .as_str()
        .ok_or("operation identity is not a string")?;
    let row = query::<sqlx_turso::Turso>("SELECT state,result,(SELECT COUNT(*) FROM _fleet_sync_steps WHERE operation_id=?) AS count FROM _fleet_sync_operations WHERE operation_id=?")
        .bind(id).bind(id).fetch_one(&mut connection).await?;
    let result = (
        row.try_get("state")?,
        row.try_get("result")?,
        row.try_get("count")?,
    );
    connection.close().await?;
    Ok(result)
}
pub async fn journal_result(path: &Path, kind: &str) -> ConformanceResult<(String, Vec<u8>)> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    let rows = query::<sqlx_turso::Turso>(
        "SELECT operation_id,result FROM _fleet_sync_operations WHERE kind=? AND state='completed'",
    )
    .bind(kind)
    .fetch_all(&mut connection)
    .await?;
    let [row] = rows.as_slice() else {
        return Err("expected exactly one completed operation of the requested kind".into());
    };
    let result = (row.try_get("operation_id")?, row.try_get("result")?);
    connection.close().await?;
    Ok(result)
}
