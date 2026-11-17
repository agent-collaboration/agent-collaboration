//! A separate native server process with explicit readiness and owned shutdown.
use collaboration_central_conformance::{
    ConformanceResult, RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation,
    owner_assertion,
};
use collaboration_central_core::{
    ClaimReservationState, ObjectAddress, StepControl, StepControlReply,
};
use durable_steps::StoreError;
use fleet_sync_protocol::{DatabaseId, TransferId};
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
    output: tokio::task::JoinHandle<bool>,
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
        let relative_module = module_path!()
            .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
            .ok_or("restart support must belong to its libtest crate")?;
        let (parent_module, _) = relative_module
            .rsplit_once("::")
            .ok_or("restart support must belong to its case module")?;
        let child_test_name = format!("{parent_module}::native_central_process");
        let mut child = Command::new(std::env::current_exe()?)
            .args(["--ignored", "--exact", &child_test_name, "--nocapture"])
            .env("CENTRAL_RESTART_ROOT", root)
            .env("CENTRAL_RESTART_JWKS", jwks)
            .env("CENTRAL_RESTART_CONTROLS", serde_json::to_string(controls)?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().ok_or("server stdout missing")?;
        let mut lines = BufReader::new(stdout).lines();
        let base = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(line) = lines.next_line().await? {
                if let Some(base) = line.strip_prefix("CENTRAL_PROCESS_READY ") {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(base.to_owned());
                }
            }
            Err("native server exited before readiness".into())
        })
        .await??;
        // Drain the test runner's small terminal output until the child exits.
        let output = tokio::spawn(async move {
            let mut one_test_passed = false;
            while let Ok(Some(line)) = lines.next_line().await {
                one_test_passed |= line.starts_with("test result: ok. 1 passed; 0 failed;");
            }
            one_test_passed
        });
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
        let one_test_passed = self.output.await?;
        if !status.success() {
            return Err(format!("native server shutdown failed: {status}").into());
        }
        if !one_test_passed {
            return Err("native server child did not report exactly one successful test".into());
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

// This observer belongs to all process recovery cases, not the removed publisher handshake.
pub(super) async fn read_claim_reservation(
    base: &str,
    database: DatabaseId,
    attempt: TransferId,
) -> ConformanceResult<Option<ClaimReservationState>> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}/admin/v1/conformance/fleet-receipt"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&StepControl::ReadClaimReservation { database, attempt })
        .send()
        .await?
        .error_for_status()?
        .json::<Result<StepControlReply, StoreError>>()
        .await??;
    match response {
        StepControlReply::ClaimReservation(state) => Ok(state),
        _ => Err("Claim reservation observer returned another result".into()),
    }
}

/// A startup control pinning the Fleet clock `seconds` after the imported
/// fixture's records, so no Fleet work ever runs on the wall clock.
pub fn pinned_after_import(seconds: i64) -> ConformanceResult<RuntimeControlRequest> {
    Ok(RuntimeControlRequest {
        address: ObjectAddress::Fleet,
        command: RuntimeControl::SetTime {
            now: collaboration_central_core::RegistryTime(
                super::imported_board_fixture::time_after_import(seconds)?,
            ),
        },
    })
}

/// A startup control pinning the Fleet clock one millisecond past the stopped
/// operation's recovery deadline (its next attempt or killed-drive lease),
/// read from the owning store, so the restart recovers it on the pinned clock.
pub async fn pinned_at_recovery(
    path: &Path,
    identity: &Value,
) -> ConformanceResult<RuntimeControlRequest> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    let id = identity
        .as_str()
        .ok_or("operation identity is not a string")?;
    let row = query::<sqlx_turso::Turso>(
        "SELECT next_attempt_at,drive_until FROM _fleet_sync_operations WHERE operation_id=?",
    )
    .bind(id)
    .fetch_one(&mut connection)
    .await?;
    let ready = row
        .try_get::<Option<i64>, _>("next_attempt_at")?
        .into_iter()
        .chain(row.try_get::<Option<i64>, _>("drive_until")?)
        .max()
        .and_then(|deadline| deadline.checked_add(1))
        .ok_or("stopped operation has no recoverable deadline")?;
    connection.close().await?;
    Ok(RuntimeControlRequest {
        address: ObjectAddress::Fleet,
        command: RuntimeControl::SetTime {
            now: collaboration_central_core::RegistryTime(
                chrono::DateTime::from_timestamp_millis(ready)
                    .ok_or("recovery deadline outside clock range")?,
            ),
        },
    })
}

/// The Fleet clock this test pinned, observed from the owner.
pub async fn pinned_fleet_time(
    base: &str,
) -> ConformanceResult<collaboration_central_core::RegistryTime> {
    match control(base, ObjectAddress::Fleet, RuntimeControl::Observe)
        .await?
        .clock
    {
        collaboration_central_conformance::ClockObservation::Pinned { now } => Ok(now),
        collaboration_central_conformance::ClockObservation::Wall => {
            Err("the Fleet clock is not pinned".into())
        }
    }
}

/// Advances the pinned Fleet clock to the time the store says a running Fleet
/// notice waits for: its next attempt when that is not yet due, otherwise the
/// expiry of the drive lease still holding it (the killed drive's 30-second
/// lease that RECOVERY_BOUND covers on the wall clock). The test moves its one
/// clock to the stored deadline instead of waiting on wall time.
pub async fn advance_fleet_to_due_notices(base: &str, path: &Path) -> ConformanceResult<()> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    let rows = query::<sqlx_turso::Turso>(
        "SELECT next_attempt_at,drive_until FROM _fleet_sync_operations WHERE kind='central.fleet-notice' AND state='running'",
    )
    .fetch_all(&mut connection)
    .await?;
    connection.close().await?;
    let now = pinned_fleet_time(base).await?.0.timestamp_millis();
    let mut target = now;
    for row in rows {
        let next = row.try_get::<Option<i64>, _>("next_attempt_at")?;
        let lease = row.try_get::<Option<i64>, _>("drive_until")?;
        let waits_for = match (next, lease) {
            (Some(next), _) if next >= now => Some(next),
            (_, Some(lease)) if lease >= now => Some(lease),
            _ => None,
        };
        if let Some(deadline) = waits_for.and_then(|deadline| deadline.checked_add(1)) {
            target = target.max(deadline);
        }
    }
    if target > now {
        control(
            base,
            ObjectAddress::Fleet,
            RuntimeControl::SetTime {
                now: collaboration_central_core::RegistryTime(
                    chrono::DateTime::from_timestamp_millis(target)
                        .ok_or("stored deadline outside clock range")?,
                ),
            },
        )
        .await?;
    }
    Ok(())
}
