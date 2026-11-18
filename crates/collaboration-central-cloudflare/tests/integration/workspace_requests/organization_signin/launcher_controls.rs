//! Consumer IPC against both compiled Workers, with signing owned by Rust.
mod machine_observation;

use super::super::workerd_host::Process;
use collaboration_central_conformance::ConformanceResult;
use fleet_runtime_test_support::signin_fixtures as signed;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};
use tokio::sync::mpsc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    origin: String,
}

struct Bridge {
    process: Arc<Process>,
    stderr: mpsc::UnboundedReceiver<String>,
    origin: String,
    client: reqwest::Client,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        assert!(
            self.process.stop() || std::thread::panicking(),
            "launcher bridge did not close successfully"
        );
    }
}

#[tokio::test]
#[ignore = "relaunched launcher panic child"]
async fn launcher_panic_child() {
    assert_eq!(std::env::var("WORKERD_LIFECYCLE_CHILD").unwrap(), "panic");
    let fixture = super::support::Fixture::new().await;
    let bridge = Bridge::start(&fixture._plan_root.path().join("signin-plan.json"))
        .await
        .unwrap();
    let bridge_pid = bridge.process.with_child(|child| child.id());
    let workerd = super::super::workerd_host::lifecycle_tests::workerd_child(bridge_pid);
    println!("WORKERD_LIFECYCLE_READY {bridge_pid} {workerd}");
    std::io::stdout().flush().unwrap();
    let mut command = [0];
    std::io::Read::read_exact(&mut std::io::stdin(), &mut command).unwrap();
    assert_eq!(command, [b'P']);
    panic!("intentional launcher panic after readiness");
}

#[test]
fn launcher_panic_disposes_real_workerd() {
    let selector = format!(
        "{}::launcher_panic_child",
        module_path!().split_once("::").unwrap().1
    );
    super::super::workerd_host::lifecycle_tests::run_parent_death(&selector, &["panic"]);
}

#[tokio::test]
async fn launcher_startup_error_survives_unsuccessful_child_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let plan = root.path().join("invalid-plan.json");
    std::fs::write(&plan, "invalid fixture plan").unwrap();
    let Err(error) = Bridge::start(&plan).await else {
        panic!("invalid plan started a bridge");
    };
    assert!(
        error.to_string().contains("invalid bridge readiness"),
        "startup diagnostic was replaced: {error}"
    );
}

#[tokio::test]
async fn launcher_command_error_settles_real_bridge_before_returning() {
    let fixture = super::support::Fixture::new().await;
    let mut bridge = Bridge::start(&fixture._plan_root.path().join("signin-plan.json"))
        .await
        .unwrap();
    let pid = bridge.process.with_child(|child| child.id());
    let workerd = super::super::workerd_host::lifecycle_tests::workerd_child(pid);
    bridge.process.with_child(|child| {
        let input = child.stdin.as_mut().unwrap();
        input.write_all(b"close\n").unwrap();
        input.flush().unwrap();
    });
    let result = bridge
        .command(json!({"observe":signed::ORGANIZATION}))
        .await;
    assert!(result.is_err(), "closing bridge cannot answer a command");
    super::super::workerd_host::lifecycle_tests::require_exited(pid, workerd);
    drop(bridge);
    fixture.close().await;
}

fn stop_after_failure(
    process: &Process,
    error: Box<dyn std::error::Error + Send + Sync>,
) -> Box<dyn std::error::Error + Send + Sync> {
    if process.stop() {
        error
    } else {
        format!("{error}; launcher bridge exited unsuccessfully during cleanup").into()
    }
}

impl Bridge {
    async fn start(plan: &Path) -> ConformanceResult<Self> {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(Path::parent)
            .ok_or("repository root")?;
        let child = Command::new("node")
            .arg(manifest.join("tests/support/workerd-bridge.mts"))
            .arg("--signin-plan")
            .arg(plan)
            .arg("--launcher-time-ms")
            .arg((signed::NOW_SECONDS * 1000).to_string())
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let process = Arc::new(Process::own(child));
        let (output, errors) = process
            .with_child(|child| (child.stdout.take().unwrap(), child.stderr.take().unwrap()));
        let (sender, stderr) = mpsc::unbounded_channel();
        process.set_error_drain(Some(std::thread::spawn(move || {
            for line in std::io::BufReader::new(errors)
                .lines()
                .map_while(Result::ok)
            {
                let _ = sender.send(line);
            }
        })));
        let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
        process.set_output_drain(std::thread::spawn(move || {
            let mut output = std::io::BufReader::new(output);
            let mut ready = String::new();
            let result = output.read_line(&mut ready).map(|_| ready);
            let _ = ready_sender.send(result);
            for _ in output.lines().map_while(Result::ok) {}
        }));
        let startup: ConformanceResult<(Ready, reqwest::Client)> = async {
            let ready = ready_receiver.await??;
            let ready: Ready =
                serde_json::from_str(&ready).map_err(|_| "invalid bridge readiness")?;
            let url = reqwest::Url::parse(&ready.origin)?;
            if url.host_str() != Some("127.0.0.1") {
                return Err("bridge readiness is not loopback".into());
            }
            let client = reqwest::Client::builder()
                .no_proxy()
                .pool_max_idle_per_host(0)
                .build()?;
            Ok((ready, client))
        }
        .await;
        let (ready, client) = startup.map_err(|error| stop_after_failure(&process, error))?;
        Ok(Self {
            process,
            stderr,
            origin: ready.origin,
            client,
        })
    }

    async fn command(&mut self, command: Value) -> ConformanceResult<Value> {
        let result = self.command_inner(command).await;
        result.map_err(|error| stop_after_failure(&self.process, error))
    }

    async fn command_inner(&mut self, command: Value) -> ConformanceResult<Value> {
        let encoded = serde_json::to_string(&command)?;
        self.process.with_child(|child| {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| std::io::Error::other("bridge stdin closed"))?;
            writeln!(stdin, "{encoded}")?;
            stdin.flush()
        })?;
        while let Some(line) = self.stderr.recv().await {
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

    async fn close(self) -> ConformanceResult<()> {
        let process = self.process.clone();
        assert!(tokio::task::spawn_blocking(move || process.stop()).await?);
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

#[expect(
    clippy::panic_in_result_fn,
    reason = "Literal private IPC observations fail without dumping fixture statements or tokens."
)]
#[tokio::test]
async fn setup_private_observe_matches_rows_and_external_fresh_restart() -> ConformanceResult<()> {
    let keys = signed::SignInKeys::start().await?;
    let root = tempfile::tempdir()?;
    let plan = root.path().join("setup-observe-plan.json");
    std::fs::write(
        &plan,
        serde_json::to_vec(&json!({"textBindings":{
        "CENTRAL_SIGN_IN_ISSUER":signed::ISSUER,"CENTRAL_SIGN_IN_AUDIENCE":signed::AUDIENCE,
        "CENTRAL_SIGN_IN_JWKS_URL":"https://signin.test/.well-known/jwks.json",
        "CENTRAL_ADMIN_AUDIENCE":signed::ADMIN_AUDIENCE,"CENTRAL_PUBLIC_ORIGIN":signed::PUBLIC_ORIGIN,
        "CENTRAL_SIGN_IN_SERVICE_ID":"019f0000-0000-7000-8000-00000000d290"
    },"jwksFixtureUrl":keys.url}))?,
    )?;
    let mut bridge = Bridge::start(&plan).await?;
    let organization = signed::ORGANIZATION;
    let operation = "019f0000-0000-7000-8000-00000000d781";
    assert_empty(&bridge.observe(organization).await?)?;
    bridge.set_time(signed::NOW_SECONDS * 1000, 0).await?;
    let statement = signed::stamp(organization, signed::NOW_SECONDS)?;
    bridge.provision(organization, &statement).await?;
    let private = format!("{}/private", bridge.origin);
    for revision in [
        json!({"kind":"organization","organizationId":organization,"revision":3,"state":"active","permissionPolicyDigest":signed::POLICY}),
        json!({"kind":"person","organizationId":organization,"personId":signed::OWNER,"subjectId":signed::OWNER_SUBJECT,"revision":4,"state":"active"}),
    ] {
        let response = bridge
            .client
            .post(format!(
                "{private}/internal/organization-authorization-revisions"
            ))
            .json(&revision)
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        let _: Value = response.json().await?;
    }
    let person = signed::Person::owner(organization, signed::NOW_SECONDS)?;
    let request_id = signed::fresh_request_id(signed::NOW_SECONDS)?;
    let proof = person.headers(
        signed::CREATE,
        &signed::create_intent(organization, operation, "Observed setup"),
        &request_id,
        signed::NOW_SECONDS,
    )?;
    let mut request = bridge
        .client
        .post(format!("{private}{}", signed::CREATE))
        .json(&signed::create_body(
            organization,
            operation,
            "Observed setup",
        ));
    for (name, value) in proof {
        request = request.header(name, value);
    }
    let response = request.send().await?;
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response.json::<Value>().await?,
        json!({"result":"applied","workspace":{"workspaceId":operation,"name":"Observed setup","description":"","state":"active"}})
    );
    let observed = bridge.observe(organization).await?;
    let expected = json!({
        "statement":statement,
        "stamp":{"digest":signed::digest(&statement),"keyId":"stamp-a","expiresAt":signed::NOW_SECONDS-60+signed::LIFETIME_SECONDS},
        "records":[
            {"kind":"organizationProvisioned","requestId":organization,"targetId":organization,"createdAtMillis":signed::NOW_SECONDS*1000},
            {"kind":"workspaceCreated","requestId":operation,"targetId":operation,"createdAtMillis":signed::NOW_SECONDS*1000}
        ],
        "organizationFloor":{"revision":3,"state":"active"},
        "personFloors":[{"subjectId":signed::OWNER_SUBJECT,"personId":signed::OWNER,"revision":4,"state":"active"}],
        "usedRequests":[{"id":request_id}],
        "workspaces":[{"workspaceId":operation,"name":"Observed setup"}],
        "machines":[],
        "machineApprovals":[],
        "complete":true
    });
    assert!(
        observed == expected,
        "setup observation must exactly match the approved minimal owning-store projection"
    );
    bridge.close().await?;
    let mut fresh = Bridge::start(&plan).await?;
    fresh.set_time(signed::NOW_SECONDS * 1000, 0).await?;
    assert_empty(&fresh.observe(organization).await?)?;
    fresh.set_time(signed::NOW_SECONDS * 1000, 0).await?;
    fresh.close().await?;
    keys.shutdown().await?;
    Ok(())
}
