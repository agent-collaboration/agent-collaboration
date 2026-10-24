//! The run context every native-capabilities journey shares.
//!
//! A Journey qualifies the declared candidate once, owns one fresh private evidence
//! directory, and opens each installed-Codex drive with the D1 reference launch and the
//! client's own per-invocation session recorder. Each drive records its own child's
//! cleanup; nothing else is ever signalled.
use crate::bounded_process::run_bounded;
use crate::installed_client_terminal::{InstalledClientTerminal, TAB};
use crate::native_exports::{codex_config_digest, native_marketplaces, native_plugin_rows};
use crate::native_fixture::write_private;
use crate::process_identity::{
    observe_process_table, own_real_uid, owned_closure, remaining_owned,
};
use crate::router_evidence::{
    consumer_requests, observe_session, operation_rows, router_health_version,
};
use crate::router_queue_observation::observe_queue;
use crate::target_qualification::{QualifiedTarget, qualify_target};
use crate::verification_models::{
    NativeCapabilitiesPlan, NativeMarketplace, NativePluginRow, ObservationCursor, OperationRow,
    ProviderEndpoint, ProviderObservation, RpcReply,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs::DirBuilder,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) const PICKER_SECONDS: Duration = Duration::from_secs(30);
const PROMPT_SETTLE_MARGIN: Duration = Duration::from_secs(30);
const PROMPT_OUTPUT_MAX_BYTES: usize = 1024 * 1024;
const SHUTDOWN: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JourneyResult {
    pub(crate) journey: &'static str,
    pub(crate) checks: Map<String, Value>,
    pub(crate) limits: Vec<&'static str>,
}

/// The global Codex preferences around one drive (R5). Only equal readable digests
/// prove them unchanged. A changed digest is not attributable to this drive, and an
/// unreadable file is not absence; both leave R5 unknown for the run, never passed,
/// while the feature cases still run.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CodexConfigObservation {
    ProvenUnchanged,
    ChangedUnattributed,
    Unobservable,
}

impl CodexConfigObservation {
    pub(crate) fn compare(before: &Result<String, String>, after: &Result<String, String>) -> Self {
        match (before, after) {
            (Ok(before), Ok(after)) if before == after => Self::ProvenUnchanged,
            (Ok(_), Ok(_)) => Self::ChangedUnattributed,
            _ => Self::Unobservable,
        }
    }
}

/// How one installed-client drive ended: its own child and every descendant acquired
/// through it with this verifier's real UID, observed again after the TUI quit.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClientCleanup {
    /// The actual trust answer for the fixture worktree: a dialog fails the drive.
    pub(crate) trust_dialog_shown: bool,
    pub(crate) capture_fault: Option<String>,
    pub(crate) forced_stop: bool,
    pub(crate) owned_pids: Vec<u32>,
    pub(crate) remaining_pids: Vec<u32>,
    pub(crate) codex_config_digest_before: Result<String, String>,
    pub(crate) codex_config_digest_after: Result<String, String>,
    pub(crate) codex_config: CodexConfigObservation,
}

pub(crate) fn json_value(value: &impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

/// The first `name.len() - 4` characters: enough for the picker to filter, never the
/// whole name, so the row is selected by the client and not typed.
pub(crate) fn name_fragment(name: &str) -> &str {
    let end = name
        .char_indices()
        .map(|(index, _)| index)
        .nth(name.chars().count().saturating_sub(4))
        .unwrap_or(name.len());
    name.get(..end).unwrap_or(name)
}

pub(crate) struct Journey {
    pub(crate) target: QualifiedTarget,
    pub(crate) output: PathBuf,
    pub(crate) home: PathBuf,
}

impl Journey {
    /// Create the fresh evidence directory and qualify the declared candidate before
    /// any drive; a target that fails qualification is never observed.
    pub(crate) async fn open(
        plan: NativeCapabilitiesPlan,
        output: PathBuf,
        home: PathBuf,
    ) -> Result<Self, String> {
        DirBuilder::new()
            .mode(0o700)
            .create(&output)
            .map_err(|error| {
                format!(
                    "evidence directory {} not created: {error}",
                    output.display()
                )
            })?;
        write_private(
            &output.join("binding.json"),
            &serde_json::to_vec_pretty(&plan).map_err(|error| error.to_string())?,
        )?;
        let router_version = router_health_version(plan.router_port).await?;
        let target =
            tokio::task::block_in_place(|| qualify_target(plan, &output, &home, router_version))?;
        write_private(
            &output.join("fixture.json"),
            &serde_json::to_vec_pretty(&target.fixture).map_err(|error| error.to_string())?,
        )?;
        Ok(Self {
            target,
            output,
            home,
        })
    }

    pub(crate) fn recorder(&self, label: &str) -> PathBuf {
        self.output.join(format!("{label}.session.jsonl"))
    }

    pub(crate) fn capture(&self, label: &str) -> PathBuf {
        self.output.join(format!("{label}.pty"))
    }

    /// The declared Session's queued input IDs through the public queue list.
    pub(crate) async fn queue(&self, label: &str) -> Result<Vec<String>, String> {
        observe_queue(
            &self.target.plan,
            &self.target.service_id,
            &self.output,
            label,
        )
        .await
    }

    pub(crate) async fn operations(&self, label: &str) -> Result<Vec<OperationRow>, String> {
        operation_rows(&self.target.plan, &self.output, label).await
    }

    pub(crate) async fn observe(
        &self,
        label: &str,
        cursor: Option<ObservationCursor>,
    ) -> Result<ProviderObservation, String> {
        observe_session(
            &self.target.plan,
            &self.target.service_id,
            &self.output,
            label,
            cursor,
        )
        .await
    }

    pub(crate) async fn requests(
        &self,
        label: &str,
        requests: &[(&str, Value)],
    ) -> Result<Vec<RpcReply>, String> {
        consumer_requests(&self.target.plan, requests, &self.output, label).await
    }

    /// Real native input through the candidate's public `conversation prompt` on the
    /// declared Session; the text, raw output and diagnostics are kept as `{label}.*`.
    /// Returns the CLI's exit code; the caller judges the effect from Router evidence.
    pub(crate) async fn router_prompt(
        &self,
        label: &str,
        text: &str,
        reply_bound: Duration,
    ) -> Result<Option<i32>, String> {
        let plan = &self.target.plan;
        let prompt_file = self.output.join(format!("{label}.txt"));
        write_private(&prompt_file, text.as_bytes())?;
        let mut command = tokio::process::Command::new(&plan.binaries.agent_collaboration.path);
        command
            .args([
                "conversation",
                "prompt",
                "--endpoint",
                plan.endpoint_id.as_str(),
            ])
            .arg("--session")
            .arg(&plan.session_id)
            .arg("--text-file")
            .arg(&prompt_file)
            .arg("--timeout-seconds")
            .arg(reply_bound.as_secs().to_string())
            .arg("--service-directory")
            .arg(plan.service_directory())
            .arg("--json");
        let completed = run_bounded(
            command,
            "Router prompt",
            reply_bound + PROMPT_SETTLE_MARGIN,
            PROMPT_OUTPUT_MAX_BYTES,
            PROMPT_OUTPUT_MAX_BYTES,
        )
        .await?;
        write_private(
            &self.output.join(format!("{label}.stdout")),
            &completed.stdout,
        )?;
        write_private(
            &self.output.join(format!("{label}.stderr")),
            &completed.stderr,
        )?;
        Ok(completed.status.code())
    }

    /// The provider's own plugin export; None when the bound provider has none (Cursor).
    pub(crate) async fn native_plugin_rows(
        &self,
        label: &str,
    ) -> Result<Option<Vec<NativePluginRow>>, String> {
        let plan = &self.target.plan;
        if plan.endpoint_id != ProviderEndpoint::ClaudeLocal {
            return Ok(None);
        }
        native_plugin_rows(
            &plan.native_executable.path,
            &self.target.fixture.workspace,
            &self.output,
            label,
        )
        .await
        .map(Some)
    }

    pub(crate) async fn native_marketplaces(
        &self,
        label: &str,
    ) -> Result<Option<Vec<NativeMarketplace>>, String> {
        let plan = &self.target.plan;
        if plan.endpoint_id != ProviderEndpoint::ClaudeLocal {
            return Ok(None);
        }
        native_marketplaces(
            &plan.native_executable.path,
            &self.target.fixture.workspace,
            &self.output,
            label,
        )
        .await
        .map(Some)
    }

    /// D1 reference launch: per-invocation `mentions_v2=false` puts skills and plugins
    /// under `$`, and the client's own recorder logs its outbound turn items to a fresh
    /// file here. Both are per-invocation; global configuration must stay unchanged.
    fn spawn_client(&self, label: &str) -> Result<InstalledClientTerminal, String> {
        let plan = &self.target.plan;
        let arguments = [
            "--no-alt-screen".to_owned(),
            "--remote".to_owned(),
            format!("unix://{}", plan.endpoint_socket().display()),
            "--cd".to_owned(),
            self.target.fixture.workspace.display().to_string(),
            "-c".to_owned(),
            "features.mentions_v2=false".to_owned(),
            "resume".to_owned(),
            plan.session_id.clone(),
        ];
        let environment = [
            ("CODEX_TUI_RECORD_SESSION", "1".to_owned()),
            (
                "CODEX_TUI_SESSION_LOG_PATH",
                self.recorder(label).display().to_string(),
            ),
        ];
        InstalledClientTerminal::spawn(
            &plan.codex_cli.path,
            &arguments,
            &self.target.fixture.workspace,
            &environment,
            &self.capture(label),
        )
    }

    /// One blocking installed-client drive. The drive's own result is returned only
    /// after its child quit and no owned descendant remains; the cleanup record is kept
    /// as `{label}-cleanup.json`.
    pub(crate) fn drive_client<T>(
        &self,
        label: &str,
        drive: impl FnOnce(&mut InstalledClientTerminal) -> Result<T, String>,
    ) -> Result<T, String> {
        tokio::task::block_in_place(|| {
            let digest_before = codex_config_digest(&self.home);
            let owner = own_real_uid()?;
            let mut client = self.spawn_client(label)?;
            let outcome = drive(&mut client);
            let owned = match client.child_pid() {
                Some(pid) => {
                    owned_closure(pid, owner, &BTreeMap::new(), &observe_process_table()?)?
                }
                None => BTreeMap::new(),
            };
            client.shutdown(SHUTDOWN)?;
            let remaining = remaining_owned(&owned, &observe_process_table()?);
            let digest_after = codex_config_digest(&self.home);
            let cleanup = ClientCleanup {
                trust_dialog_shown: client.trust_dialog_shown(),
                capture_fault: client.capture_fault.clone(),
                forced_stop: client.forced_stop,
                owned_pids: owned.keys().copied().collect(),
                remaining_pids: remaining.keys().copied().collect(),
                codex_config: CodexConfigObservation::compare(&digest_before, &digest_after),
                codex_config_digest_before: digest_before,
                codex_config_digest_after: digest_after,
            };
            write_private(
                &self.output.join(format!("{label}-cleanup.json")),
                &serde_json::to_vec_pretty(&cleanup).map_err(|error| error.to_string())?,
            )?;
            if !cleanup.remaining_pids.is_empty() {
                return Err(format!(
                    "owned client processes remain after quit: {:?}; not signalled",
                    cleanup.remaining_pids
                ));
            }
            if let Some(fault) = &cleanup.capture_fault {
                return Err(format!("installed-client evidence incomplete: {fault}"));
            }
            if cleanup.trust_dialog_shown {
                return Err("installed Codex showed a saved folder-trust dialog: fixture setup is not trusted".to_owned());
            }
            outcome
        })
    }

    /// Type `$` and part of the name, wait for the picker row, select it with Tab.
    pub(crate) fn select_skill(
        &self,
        client: &mut InstalledClientTerminal,
        prefix: &str,
        name: &str,
        row: &str,
    ) -> Result<(), String> {
        let mark = client.mark();
        client.type_text(&format!("{prefix}${}", name_fragment(name)))?;
        let screen = client.wait_for(row, mark, PICKER_SECONDS)?;
        write_private(
            &self.output.join(format!("picker-{name}.txt")),
            screen.as_bytes(),
        )?;
        client.send(TAB)
    }
}

/// The real user's home, for the production-root refusal and the global config digest.
pub(crate) fn user_home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or_else(|| "HOME is not an absolute path".to_owned())
}

pub(crate) fn save_result(output: &Path, result: &JourneyResult) -> Result<String, String> {
    let json = serde_json::to_string_pretty(result).map_err(|error| error.to_string())?;
    write_private(&output.join("result.json"), json.as_bytes())?;
    Ok(json)
}
