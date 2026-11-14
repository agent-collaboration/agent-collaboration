//! Rust-owned control transport for one isolated real workerd process.
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use collaboration_central_core::{AlarmResult, DrainResult, WorkspaceCaller};
use collaboration_protocol::central_mail::{CentralMailRequest, CentralMailResult};
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::Clock;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
};
use workspace_model::{RequestId, WorkspaceId, WorkspaceReply, WorkspaceRequest};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    origin: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MissingBundle {
    path: std::path::PathBuf,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AlarmReceipt {
    pub organization: Option<OrganizationId>,
    pub initialized_cold: bool,
    pub alarm_before: Option<i64>,
    pub alarm_after: Option<i64>,
    pub result: AlarmReceiptResult,
}
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum AlarmReceiptResult {
    Applied {
        wakes_fired: usize,
        automations_fired: usize,
        windows_fired: usize,
        workspaces_drained: usize,
    },
    Error {
        message: String,
        failure: Option<StoreFailure>,
    },
    Unprovisioned,
}
impl AlarmReceipt {
    fn into_result(
        self,
        organization: &OrganizationId,
        require_cold: bool,
    ) -> Result<AlarmResult, StoreFailure> {
        if require_cold && !self.initialized_cold {
            return Err(StoreFailure::Unavailable);
        }
        match self.result {
            AlarmReceiptResult::Applied {
                wakes_fired,
                automations_fired,
                windows_fired,
                workspaces_drained,
            } if self.organization.as_ref() == Some(organization) => Ok(AlarmResult {
                wakes_fired,
                automations_fired,
                windows_fired,
                workspaces_drained,
            }),
            AlarmReceiptResult::Unprovisioned if self.organization.is_none() => {
                Ok(AlarmResult::default())
            }
            AlarmReceiptResult::Error { message, failure } => {
                eprintln!("workerd alarm failed: {message}");
                Err(failure.unwrap_or(StoreFailure::Unavailable))
            }
            _ => Err(StoreFailure::Unavailable),
        }
    }
}
struct Process {
    child: Mutex<Option<Child>>,
    output: Mutex<Option<std::thread::JoinHandle<()>>>,
    errors: Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl Process {
    fn stop(&self) -> bool {
        if let Some(mut child) = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(b"close\n");
            }
            let stopped = child.wait().is_ok_and(|status| status.success());
            if let Some(output) = self
                .output
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                && output.join().is_err()
            {
                return false;
            }
            if let Some(errors) = self
                .errors
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                assert!(errors.join().is_ok());
            }
            return stopped;
        }
        true
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
struct Transport {
    origin: String,
    client: reqwest::Client,
    process: Arc<Process>,
}
impl Transport {
    async fn control<T: DeserializeOwned>(
        &self,
        organization: &OrganizationId,
        operation: &str,
        body: &impl Serialize,
    ) -> Result<T, StoreFailure> {
        let response = self
            .client
            .post(format!(
                "{}/test-support/workspace/{}/{operation}",
                self.origin,
                organization.as_str()
            ))
            .json(body)
            .send()
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
        if !response.status().is_success() {
            eprintln!("workerd control {operation}: HTTP {}", response.status());
            return Err(StoreFailure::Unavailable);
        }
        response.json().await.map_err(|error| {
            eprintln!("workerd control {operation} decode: {error}");
            StoreFailure::Unavailable
        })
    }
}
pub(super) struct WorkerdHost {
    transport: Arc<Transport>,
    clock: Arc<dyn Clock>,
    orgs: Mutex<BTreeMap<OrganizationId, Arc<RemoteRuntime>>>,
    captured_logs: Arc<Mutex<Vec<String>>>,
    log_changed: tokio::sync::watch::Sender<usize>,
}
impl WorkerdHost {
    pub async fn open(clock: Arc<dyn Clock>) -> Result<Self, StoreFailure> {
        Self::open_configured(clock, None).await
    }
    pub async fn open_signin(clock: Arc<dyn Clock>, plan: &Path) -> Result<Self, StoreFailure> {
        Self::open_configured(clock, Some(plan.to_owned())).await
    }
    async fn open_configured(
        clock: Arc<dyn Clock>,
        plan: Option<std::path::PathBuf>,
    ) -> Result<Self, StoreFailure> {
        let signin = plan.is_some();
        let captured_logs = Arc::new(Mutex::new(Vec::new()));
        let log_changed = tokio::sync::watch::channel(0usize).0;
        let process_logs = captured_logs.clone();
        let process_changed = log_changed.clone();
        let (origin, process) = tokio::task::spawn_blocking(move || {
            let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
            let root = manifest.parent().unwrap().parent().unwrap();
            let mut command = Command::new("node");
            command.arg(manifest.join("tests/support/workerd-bridge.mts"));
            if let Some(plan) = plan {
                command.arg("--signin-plan").arg(plan);
            }
            let mut child = command
                .current_dir(root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(if signin {
                    Stdio::piped()
                } else {
                    Stdio::inherit()
                })
                .spawn()
                .unwrap();
            let error_drain = child.stderr.take().map(|stderr| {
                let logs = process_logs.clone();
                let changed = process_changed.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(stderr)
                        .lines()
                        .map_while(std::result::Result::ok)
                    {
                        logs.lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(line);
                        changed.send_modify(|count| *count += 1);
                    }
                })
            });
            let mut output = BufReader::new(child.stdout.take().unwrap());
            let process = Arc::new(Process {
                child: Mutex::new(Some(child)),
                output: Mutex::new(None),
                errors: Mutex::new(error_drain),
            });
            let mut line = String::new();
            let read = output.read_line(&mut line).unwrap();
            if read == 0 {
                // Join the stderr drain before reading its final diagnostic. Never
                // print arbitrary bridge logs: only a known build artifact path.
                process.stop();
                let logs = process_logs.lock().unwrap_or_else(PoisonError::into_inner);
                let missing = logs.iter().find_map(|line| {
                    let diagnostic = line.strip_prefix("WORKERD_BUNDLE_MISSING ")?;
                    let missing: MissingBundle = serde_json::from_str(diagnostic).ok()?;
                    ["build-test-support", "build-link-test-support"]
                        .iter()
                        .flat_map(|directory| {
                            ["index.js", "index_bg.wasm"]
                                .map(|name| manifest.join(directory).join(name))
                        })
                        .find(|expected| *expected == missing.path)
                });
                if let Some(path) = missing {
                    panic!("workerd bridge missing bundle: {}", path.display());
                }
            }
            assert!(read > 0, "workerd bridge exited before readiness");
            let ready: Ready = serde_json::from_str(&line).unwrap();
            let url = reqwest::Url::parse(&ready.origin).unwrap();
            assert_eq!(url.host_str(), Some("127.0.0.1"));
            let drain = std::thread::spawn(move || {
                let mut line = String::new();
                while output.read_line(&mut line).unwrap_or(0) > 0 {
                    if signin {
                        process_logs
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(line.clone());
                        process_changed.send_modify(|count| *count += 1);
                    } else {
                        eprint!("{line}");
                    }
                    line.clear();
                }
            });
            *process
                .output
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(drain);
            (ready.origin, process)
        })
        .await
        .map_err(|_| StoreFailure::Unavailable)?;
        Ok(Self {
            transport: Arc::new(Transport {
                origin: if signin {
                    format!("{origin}/public")
                } else {
                    origin
                },
                client: reqwest::Client::new(),
                process,
            }),
            clock,
            orgs: Mutex::new(BTreeMap::new()),
            captured_logs,
            log_changed,
        })
    }
    pub fn captured_logs(&self) -> Vec<String> {
        self.captured_logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub async fn until_log(&self, fragment: &str) {
        let mut changed = self.log_changed.subscribe();
        loop {
            if self
                .captured_logs()
                .iter()
                .any(|line| line.contains(fragment))
            {
                return;
            }
            assert!(
                changed.changed().await.is_ok(),
                "worker log stream ended before event"
            );
        }
    }
    pub fn signin_origin(&self) -> String {
        format!(
            "{}/private",
            self.transport
                .origin
                .strip_suffix("/public")
                .expect("sign-in topology")
        )
    }
    pub fn public_origin(&self) -> &str {
        &self.transport.origin
    }
    pub async fn central(&self, organization: &OrganizationId) -> Arc<RemoteRuntime> {
        let runtime = self.cold_runtime(organization);
        runtime.apply_clock().await;
        runtime
    }
    pub fn cold_runtime(&self, organization: &OrganizationId) -> Arc<RemoteRuntime> {
        self.orgs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(organization.clone())
            .or_insert_with(|| {
                Arc::new(RemoteRuntime {
                    organization: organization.clone(),
                    transport: self.transport.clone(),
                    clock: self.clock.clone(),
                    lost_answers: Mutex::new(vec![]),
                })
            })
            .clone()
    }
    pub async fn request(
        &self,
        caller: &WorkspaceCaller,
        request: WorkspaceRequest,
    ) -> Result<WorkspaceReply, StoreFailure> {
        self.central(&caller.organization_id)
            .await
            .request(caller, request)
            .await
    }
    pub async fn request_central(
        &self,
        caller: &WorkspaceCaller,
        request: CentralMailRequest,
    ) -> Result<CentralMailResult, StoreFailure> {
        self.central(&caller.organization_id)
            .await
            .request_central(caller, request)
            .await
    }
    pub async fn close(&self) {
        let process = self.transport.process.clone();
        assert!(
            tokio::task::spawn_blocking(move || process.stop())
                .await
                .unwrap(),
            "workerd bridge did not close successfully"
        );
    }
    pub async fn reset_owners(&self, organization: &OrganizationId, workspace: &WorkspaceId) {
        let _: bool = self
            .transport
            .control(organization, "reset-store", workspace)
            .await
            .unwrap();
        // The reset route deliberately destroys its own request; transport failure is expected.
        let _ = self
            .transport
            .client
            .post(format!(
                "{}/test-support/workspace/{}/crash",
                self.transport.origin,
                organization.as_str()
            ))
            .body("null")
            .send()
            .await;
        self.central(organization).await.apply_clock().await;
    }
}
pub(super) struct RemoteRuntime {
    organization: OrganizationId,
    transport: Arc<Transport>,
    clock: Arc<dyn Clock>,
    lost_answers: Mutex<Vec<RequestId>>,
}
impl RemoteRuntime {
    async fn apply_clock(&self) {
        let _: bool = self
            .control("clock", &self.clock.now().unix_millis())
            .await
            .unwrap();
    }
    pub async fn control<T: DeserializeOwned>(
        &self,
        operation: &str,
        body: &impl Serialize,
    ) -> Result<T, StoreFailure> {
        self.transport
            .control(&self.organization, operation, body)
            .await
    }
    pub fn statement_host(&self) -> &Self {
        self
    }
    pub async fn request_central(
        &self,
        caller: &WorkspaceCaller,
        request: CentralMailRequest,
    ) -> Result<CentralMailResult, StoreFailure> {
        self.apply_clock().await;
        let losses = std::mem::take(
            &mut *self
                .lost_answers
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for id in losses {
            let _: bool = self.control("lose-reply", &id).await?;
        }
        self.control("mail", &serde_json::json!({"organizationId":caller.organization_id,"caller":caller.caller,"credentialHash":caller.credential_hash,"request":request})).await?
    }
    pub async fn drain_workspace(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<DrainResult, StoreFailure> {
        self.apply_clock().await;
        self.control("drain", workspace).await?
    }
    pub async fn on_alarm(&self) -> Result<AlarmResult, StoreFailure> {
        self.apply_clock().await;
        let _: bool = self
            .control("raw-arm-alarm", &self.clock.now().unix_millis())
            .await?;
        let receipt: AlarmReceipt = self.control("raw-until-alarm", &()).await?;
        receipt.into_result(&self.organization, false)
    }
    pub async fn cold_alarm(&self) -> Result<AlarmResult, StoreFailure> {
        let _: bool = self.control("raw-clear-alarm-receipt", &()).await?;
        let reset: Result<bool, StoreFailure> = self
            .control("raw-arm-cold-alarm", &self.clock.now().unix_millis())
            .await;
        if reset.is_ok() {
            return Err(StoreFailure::Unavailable);
        }
        let receipt: AlarmReceipt = self.control("raw-until-alarm", &()).await?;
        receipt.into_result(&self.organization, true)
    }
    pub async fn run_batch(&self, batch: StatementBatch) -> Result<BatchOutcome, StoreFailure> {
        self.control("central-batch", &batch).await?
    }
    pub async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        self.control("central-read", &statement).await?
    }
    pub async fn inspect_workspace(
        &self,
        workspace: &WorkspaceId,
        statement: Statement,
    ) -> Result<ReadOutcome, StoreFailure> {
        match self.store(workspace, StoreCall::Read(statement)).await? {
            StoreReply::Read(rows) => Ok(rows),
            StoreReply::Batch(_) => panic!("read returned a batch"),
        }
    }
    pub async fn workspace_batch(
        &self,
        workspace: &WorkspaceId,
        batch: StatementBatch,
    ) -> Result<BatchOutcome, StoreFailure> {
        match self.store(workspace, StoreCall::Batch(batch)).await? {
            StoreReply::Batch(result) => Ok(result),
            StoreReply::Read(_) => panic!("batch returned a read"),
        }
    }
    async fn store(
        &self,
        workspace: &WorkspaceId,
        call: StoreCall,
    ) -> Result<StoreReply, StoreFailure> {
        self.control(
            "store",
            &serde_json::json!({"workspace":workspace,"call":call}),
        )
        .await?
    }
    pub fn lose_reply_after_apply(&self, id: RequestId) {
        self.lost_answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(id);
    }
    pub async fn request(
        &self,
        caller: &WorkspaceCaller,
        request: WorkspaceRequest,
    ) -> Result<WorkspaceReply, StoreFailure> {
        self.apply_clock().await;
        let losses = std::mem::take(
            &mut *self
                .lost_answers
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for id in losses {
            let _: bool = self.control("lose-reply", &id).await?;
        }
        self.control("request",&serde_json::json!({"organizationId":caller.organization_id,"caller":caller.caller,"credentialHash":caller.credential_hash,"request":request})).await?
    }
}
