//! Consumer IPC against both compiled Workers, with signing owned by Rust.
use collaboration_central_conformance::{ConformanceResult, signin_fixtures as signed};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::Path, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStderr, ChildStdin, Command},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    origin: String,
}

struct Bridge {
    child: Child,
    stdin: ChildStdin,
    stderr: tokio::io::Lines<BufReader<ChildStderr>>,
    stdout: tokio::task::JoinHandle<std::io::Result<()>>,
    origin: String,
    client: reqwest::Client,
}

impl Bridge {
    async fn start(plan: &Path) -> ConformanceResult<Self> {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(Path::parent)
            .ok_or("repository root")?;
        let mut child = Command::new("node")
            .arg(manifest.join("tests/support/workerd-bridge.mts"))
            .arg("--signin-plan")
            .arg(plan)
            .arg("--launcher-time-ms")
            .arg((signed::NOW_SECONDS * 1000).to_string())
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take().ok_or("bridge stdin")?;
        let stderr = BufReader::new(child.stderr.take().ok_or("bridge stderr")?).lines();
        let mut output = BufReader::new(child.stdout.take().ok_or("bridge stdout")?).lines();
        let ready = output
            .next_line()
            .await?
            .ok_or("bridge readiness missing")?;
        let ready: Ready = serde_json::from_str(&ready).map_err(|_| "invalid bridge readiness")?;
        let url = reqwest::Url::parse(&ready.origin)?;
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        let stdout = tokio::spawn(async move {
            while output.next_line().await?.is_some() {}
            Ok(())
        });
        Ok(Self {
            child,
            stdin,
            stderr,
            stdout,
            origin: ready.origin,
            client: reqwest::Client::builder()
                .no_proxy()
                .pool_max_idle_per_host(0)
                .build()?,
        })
    }

    async fn command(&mut self, command: Value) -> ConformanceResult<Value> {
        self.stdin
            .write_all(serde_json::to_string(&command)?.as_bytes())
            .await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        while let Some(line) = self.stderr.next_line().await? {
            // Worker diagnostics are not protocol answers. Never copy them or
            // the fixture's raw compact statement into a failed assertion.
            if let Ok(answer) = serde_json::from_str::<Value>(&line)
                && answer.get("answer").is_some()
            {
                return Ok(answer);
            }
        }
        Err("bridge ended before command answer".into())
    }

    async fn set_time(&mut self, milliseconds: i64, live: u64) -> ConformanceResult<()> {
        let answer = self.command(json!({"setTimeMs":milliseconds})).await?;
        assert_eq!(
            answer.get("answer").and_then(Value::as_str),
            Some("timeSet")
        );
        assert_eq!(
            answer.get("setTimeMs").and_then(Value::as_i64),
            Some(milliseconds)
        );
        assert_eq!(answer.get("liveFleets").and_then(Value::as_u64), Some(live));
        Ok(())
    }

    async fn observe(&mut self, organization: &str) -> ConformanceResult<Value> {
        let mut answer = self.command(json!({"observe":organization})).await?;
        assert_eq!(
            answer.get("answer").and_then(Value::as_str),
            Some("observed")
        );
        assert_eq!(
            answer.get("organization").and_then(Value::as_str),
            Some(organization)
        );
        let observation = answer
            .get_mut("observation")
            .ok_or("observation missing")?
            .take();
        assert_eq!(
            observation.get("complete").and_then(Value::as_bool),
            Some(true)
        );
        Ok(observation)
    }

    async fn provision(&self, organization: &str, statement: &str) -> ConformanceResult<()> {
        let response = self
            .client
            .post(format!(
                "{}/private/internal/organization-provisioning",
                self.origin
            ))
            .json(&json!({"organizationId":organization,"organizationStatement":statement}))
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        Ok(())
    }

    async fn close(mut self) -> ConformanceResult<()> {
        self.stdin.write_all(b"close\n").await?;
        drop(self.stdin);
        // Drain stderr while the child closes, so even verbose Worker output
        // cannot fill the pipe and prevent owned process shutdown.
        while self.stderr.next_line().await?.is_some() {}
        assert!(self.child.wait().await?.success());
        self.stdout.await??;
        Ok(())
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Fixture equality must fail without dumping a raw observation into an assertion diff."
)]
fn assert_empty(observation: &Value) -> ConformanceResult<()> {
    let expected: Value = serde_json::from_str(
        collaboration_central_core::signin_test_observation::EMPTY_SIGNIN_OBSERVATION_JSON,
    )?;
    assert!(
        observation == &expected,
        "unknown organization must be completely empty"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Opaque statement equality and literal record times are test assertions; raw statements must not be printed."
)]
fn assert_statement_and_records(
    observation: &Value,
    statement: &str,
    expected_times: &[i64],
) -> ConformanceResult<()> {
    assert!(
        observation.get("statement").and_then(Value::as_str) == Some(statement),
        "observation must return the exact fixture statement"
    );
    let records = observation
        .get("records")
        .and_then(Value::as_array)
        .ok_or("records missing")?;
    let mut times: Vec<i64> = records
        .iter()
        .map(|record| {
            record
                .get("createdAtMillis")
                .and_then(Value::as_i64)
                .ok_or("record time missing")
        })
        .collect::<Result<_, _>>()?;
    times.sort_unstable();
    assert_eq!(times, expected_times);
    Ok(())
}

#[tokio::test]
async fn bridge_controls_share_time_with_live_and_future_orgs_and_fresh_process_is_empty()
-> ConformanceResult<()> {
    // Arrange: actual compiled public/private Workers and a real fixture JWKS server.
    let keys = signed::SignInKeys::start().await?;
    let root = tempfile::tempdir()?;
    let plan = root.path().join("signin-plan.json");
    std::fs::write(
        &plan,
        serde_json::to_vec(&json!({
            "textBindings":{
                "CENTRAL_SIGN_IN_ISSUER":signed::ISSUER,
                "CENTRAL_SIGN_IN_AUDIENCE":signed::AUDIENCE,
                "CENTRAL_SIGN_IN_JWKS_URL":"https://signin.test/.well-known/jwks.json",
                "CENTRAL_ADMIN_AUDIENCE":signed::ADMIN_AUDIENCE,
                "CENTRAL_PUBLIC_ORIGIN":signed::PUBLIC_ORIGIN,
                "CENTRAL_SIGN_IN_SERVICE_ID":"019f0000-0000-7000-8000-00000000d290"
            },"jwksFixtureUrl":keys.url
        }))?,
    )?;
    let organization_a = signed::ORGANIZATION;
    let organization_b = "019f0000-0000-7000-8000-00000000d2f1";
    let now = signed::NOW_SECONDS * 1000;
    let later = now + 3_600_000;
    let mut bridge = Bridge::start(&plan).await?;

    // Act/Assert: observing an unopened org must not register or create an object.
    assert_empty(&bridge.observe(organization_a).await?)?;
    // No setTimeMs command precedes this first signed request: the required
    // initial launcher time must already own verification and record timestamps.
    let first = signed::stamp(organization_a, signed::NOW_SECONDS)?;
    bridge.provision(organization_a, &first).await?;
    assert_statement_and_records(&bridge.observe(organization_a).await?, &first, &[now])?;

    // A statement issued after the old clock is accepted only if the live
    // verifier and the never-before-addressed object's verifier share this move.
    bridge.set_time(later, 1).await?;
    let renewed = signed::renewal(organization_a, &first, later / 1000, 0)?;
    bridge.provision(organization_a, &renewed).await?;
    assert_statement_and_records(
        &bridge.observe(organization_a).await?,
        &renewed,
        &[now, later],
    )?;
    let future = signed::stamp(organization_b, later / 1000)?;
    bridge.provision(organization_b, &future).await?;
    assert_statement_and_records(&bridge.observe(organization_b).await?, &future, &[later])?;
    bridge.close().await?;

    // Fresh persistence belongs to a new bridge process, never an in-process reset.
    let mut fresh = Bridge::start(&plan).await?;
    fresh.set_time(later, 0).await?;
    assert_empty(&fresh.observe(organization_a).await?)?;
    assert_empty(&fresh.observe(organization_b).await?)?;
    fresh.set_time(later, 0).await?;
    fresh.close().await?;
    keys.shutdown().await?;
    Ok(())
}
