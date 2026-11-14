//! Owned launcher process, typed private IPC and the real Unix HTTP client.
use super::TestResult;
use collaboration_central_conformance::signin_fixtures as signed;
use collaboration_central_core::signin_test_observation::SignInObservation;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
};

const SERVICE: &str = "019f0000-0000-7000-8000-00000000e501";
const LEGACY: &str = "019f0000-0000-7000-8000-00000000e5f0";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Readiness {
    base_url: String,
    socket: PathBuf,
}

#[derive(Serialize)]
#[serde(untagged)]
enum StdinCommand<'organization> {
    SetTime {
        #[serde(rename = "setTimeMs")]
        set_time_ms: i64,
    },
    Observe {
        observe: &'organization str,
    },
}

// No Debug: an observed statement is allowed only in fixture IPC, never a
// failed assertion, parser error, diagnostic, or test receipt.
#[derive(Deserialize)]
#[serde(
    tag = "answer",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum StderrAnswer {
    TimeSet {
        set_time_ms: i64,
        live_fleets: usize,
    },
    Observed {
        organization: String,
        observation: Box<SignInObservation>,
    },
    Refused {
        #[serde(rename = "error")]
        _error: String,
    },
    Failed {
        #[serde(rename = "error")]
        _error: String,
    },
}

pub(super) struct LauncherChild {
    // kill_on_drop terminates only this owned direct child on error/unwind.
    // The successful journey explicitly waits/reaps before its temp root drops.
    child: Child,
    input: Option<ChildStdin>,
    output: Lines<BufReader<ChildStdout>>,
    diagnostics: Lines<BufReader<ChildStderr>>,
    pub(super) root: PathBuf,
    socket: PathBuf,
    base_url: String,
    client: reqwest::Client,
}

impl LauncherChild {
    pub(super) async fn start(
        root: &Path,
        socket: &Path,
        keys: &signed::SignInKeys,
    ) -> TestResult<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_central_sign_in_launcher"))
            .arg("--root")
            .arg(root)
            .arg("--socket")
            .arg(socket)
            .arg("--issuer")
            .arg(signed::ISSUER)
            .arg("--audience")
            .arg(signed::AUDIENCE)
            .arg("--admin-audience")
            .arg(signed::ADMIN_AUDIENCE)
            .arg("--public-origin")
            .arg(signed::PUBLIC_ORIGIN)
            .arg("--jwks-url")
            .arg(&keys.url)
            .arg("--service-id")
            .arg(SERVICE)
            .arg("--legacy-organization")
            .arg(LEGACY)
            .arg("--fixed-time-ms")
            .arg((signed::NOW_SECONDS * 1000).to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let input = child.stdin.take().ok_or("launcher stdin missing")?;
        let mut output =
            BufReader::new(child.stdout.take().ok_or("launcher stdout missing")?).lines();
        let diagnostics =
            BufReader::new(child.stderr.take().ok_or("launcher stderr missing")?).lines();
        let line = output
            .next_line()
            .await?
            .ok_or("launcher exited before readiness")?;
        let ready: Readiness = serde_json::from_str(&line)
            .map_err(|_| "launcher readiness did not match its exact JSON contract")?;
        if ready.base_url != "http://localhost" || ready.socket != socket {
            return Err("launcher readiness authority/socket differs from the request".into());
        }
        let metadata = std::fs::symlink_metadata(socket)?;
        if !metadata.file_type().is_socket() || metadata.permissions().mode() & 0o777 != 0o600 {
            return Err("ready private socket is not an owner-only Unix socket".into());
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .unix_socket(socket)
            .build()?;
        Ok(Self {
            child,
            input: Some(input),
            output,
            diagnostics,
            root: root.to_owned(),
            socket: socket.to_owned(),
            base_url: ready.base_url,
            client,
        })
    }

    pub(super) fn organization_file(&self, organization: &str) -> PathBuf {
        self.root
            .join("central")
            .join(format!("{organization}.sqlite"))
    }

    pub(super) fn legacy_file_exists(&self) -> bool {
        self.root.join("fleet.db").exists() || self.organization_file(LEGACY).exists()
    }

    async fn command(&mut self, command: &impl Serialize) -> TestResult<StderrAnswer> {
        let mut bytes =
            serde_json::to_vec(command).map_err(|_| "launcher command serialization failed")?;
        bytes.push(b'\n');
        let input = self.input.as_mut().ok_or("launcher stdin was closed")?;
        input.write_all(&bytes).await?;
        input.flush().await?;
        loop {
            let line = tokio::select! {
                line = self.diagnostics.next_line() => line?.ok_or("launcher stderr closed before its answer")?,
                _ = self.output.next_line() => return Err("launcher stdout carried more than its readiness line or closed early".into()),
            };
            // stderr may also contain safe diagnostics. Classify only the
            // unknown outer envelope here, then decode the closed IPC answer.
            let Ok(envelope) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if envelope.get("answer").is_none() {
                continue;
            }
            return serde_json::from_value(envelope)
                .map_err(|_| "launcher stderr answer did not match its typed IPC contract".into());
        }
    }

    pub(super) async fn expect_refused(&mut self, command: &Value) -> TestResult {
        match self.command(command).await? {
            StderrAnswer::Refused { .. } => Ok(()),
            _ => Err("invalid launcher command did not receive a refused answer".into()),
        }
    }

    pub(super) async fn set_time_ms(&mut self, time_ms: i64) -> TestResult<usize> {
        match self
            .command(&StdinCommand::SetTime {
                set_time_ms: time_ms,
            })
            .await?
        {
            StderrAnswer::TimeSet {
                set_time_ms,
                live_fleets,
            } if set_time_ms == time_ms => Ok(live_fleets),
            _ => Err("launcher did not acknowledge the requested time with timeSet".into()),
        }
    }

    pub(super) async fn observe(&mut self, organization: &str) -> TestResult<SignInObservation> {
        match self
            .command(&StdinCommand::Observe {
                observe: organization,
            })
            .await?
        {
            StderrAnswer::Observed {
                organization: observed,
                observation,
            } if observed == organization => Ok(*observation),
            _ => {
                Err("launcher did not return an observation for the addressed organization".into())
            }
        }
    }

    pub(super) async fn post(
        &self,
        path: &str,
        body: &Value,
        headers: &[(&str, String)],
    ) -> TestResult<Value> {
        let mut request = self
            .client
            .post(format!("{}{path}", self.base_url))
            .json(body);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let response = request.send().await?;
        if response.status().as_u16() != 200 {
            return Err("launcher private HTTP request did not succeed".into());
        }
        serde_json::from_slice(&response.bytes().await?)
            .map_err(|_| "launcher private HTTP answer was not JSON".into())
    }

    pub(super) async fn terminate_with_stdin_open(self) -> TestResult {
        self.shutdown_with_stdin_open(rustix::process::Signal::TERM)
            .await
    }

    pub(super) async fn interrupt_with_stdin_open(self) -> TestResult {
        self.shutdown_with_stdin_open(rustix::process::Signal::INT)
            .await
    }

    async fn shutdown_with_stdin_open(mut self, signal: rustix::process::Signal) -> TestResult {
        if self.input.is_none() || self.child.try_wait()?.is_some() {
            return Err("launcher stopped or stdin closed before the shutdown signal".into());
        }
        let pid = self.child.id().ok_or("launcher child PID missing")?;
        let pid = rustix::process::Pid::from_raw(i32::try_from(pid)?)
            .ok_or("launcher child PID invalid")?;
        rustix::process::kill_process(pid, signal)?;
        // Keep the write end alive until the actual child has exited. Closing
        // stdin first would hide a reader that cannot be cancelled by the signal.
        let status = self.child.wait().await?;
        self.input.take();
        if status.code() != Some(0) {
            return Err("launcher shutdown signal exit was not zero".into());
        }
        if self.socket.exists() {
            return Err("launcher shutdown signal left its private socket behind".into());
        }
        if self.output.next_line().await?.is_some() {
            return Err("launcher stdout contained a line beyond readiness".into());
        }
        while let Some(line) = self.diagnostics.next_line().await? {
            if serde_json::from_str::<Value>(&line).is_ok_and(|value| value.get("answer").is_some())
            {
                return Err("launcher emitted an extra unrequested stderr answer".into());
            }
        }
        Ok(())
    }
}
