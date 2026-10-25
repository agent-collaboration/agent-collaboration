//! Typed plan, fixture record and evidence views for the native-capabilities journeys.
//!
//! Consumer views mirror the installed Codex 0.163 app-server shapes the journeys read
//! (SkillsListResponse, PluginListResponse, PluginReadResponse, ThreadItem::UserMessage)
//! and decode only the fields an oracle uses; raw replies stay authoritative on disk.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Controlled native skills materialized into the prepared fixture worktree for one run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FixtureRecord {
    pub(crate) workspace: PathBuf,
    /// The commit the linked worktree was checked out at when materialized.
    pub(crate) workspace_head: String,
    pub(crate) empty_workspace: PathBuf,
    pub(crate) visible_file: PathBuf,
    pub(crate) skill_name: String,
    pub(crate) skill_file: PathBuf,
    pub(crate) canary: String,
    pub(crate) second_skill_name: String,
    pub(crate) second_skill_file: PathBuf,
    pub(crate) second_canary: String,
}

/// One binary the candidate run uses, bound by its content hash.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct QualifiedBinary {
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
}

/// One binary in the build's own qualification record; other recorded fields are kept
/// on disk and not judged here.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildRecordBinary {
    pub(crate) target: String,
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
    pub(crate) test_guard_features: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct BuildRecord {
    pub(crate) binaries: Vec<BuildRecordBinary>,
}

/// The three signed normal candidate binaries the Lead launched.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CandidateBinaries {
    pub(crate) codex_router: QualifiedBinary,
    pub(crate) agent_collaboration: QualifiedBinary,
    pub(crate) agent_sessions: QualifiedBinary,
}

/// A process identity recorded when the Lead started it; owner is the real UID.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecordedProcess {
    pub(crate) pid: u32,
    pub(crate) real_uid: u32,
    pub(crate) group: u32,
    pub(crate) birth: String,
    pub(crate) executable: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EndpointRefView {
    pub(crate) service_id: String,
    pub(crate) endpoint_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionRefView {
    pub(crate) endpoint: EndpointRefView,
    pub(crate) session_id: String,
}

/// The ACP actor the verifier presents for read-only queue observation: only its own
/// agent Session. There is no Human variant, so a Human identity never decodes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum ObserverIdentity {
    Session { session: SessionRefView },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum ProviderEndpoint {
    #[serde(rename = "claude-local")]
    ClaudeLocal,
    #[serde(rename = "cursor-local")]
    CursorLocal,
}

impl ProviderEndpoint {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeLocal => "claude-local",
            Self::CursorLocal => "cursor-local",
        }
    }
}

/// One journey run against the candidate the Lead launched through the ordinary
/// `codex-router serve`/`host` lifetimes. Native roots are declared from the
/// provider's own loader source with anchors in the run notes, never fixture-only.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeCapabilitiesPlan {
    pub(crate) repository: PathBuf,
    pub(crate) source_head: String,
    pub(crate) binaries: CandidateBinaries,
    pub(crate) router_root: PathBuf,
    /// The endpoint socket the installed client and consumer requests actually use:
    /// the declared transport, distinct from the canonical root's storage identity.
    /// A short alias of the same socket fits the Unix socket path limit (SUN_LEN) when
    /// the canonical spelling does not.
    pub(crate) endpoint_socket: PathBuf,
    pub(crate) router_port: u16,
    pub(crate) expected_version: String,
    pub(crate) router_process: RecordedProcess,
    pub(crate) host_process: RecordedProcess,
    /// The build's own record of the three binaries (`{"binaries": [{target, path,
    /// sha256, testGuardFeatures}]}`): a normal build has no test-guard features.
    pub(crate) binary_qualification: PathBuf,
    pub(crate) codex_cli: QualifiedBinary,
    pub(crate) native_executable: QualifiedBinary,
    /// The PATH the Lead launched the Host with: the Host binds `claude-local` from it.
    #[serde(default)]
    pub(crate) host_search_path: Option<String>,
    /// For `claude-local`: the ACP adapter entry file the Host actually runs (such as
    /// the npx cache's `dist/acp-agent.js`). Same-version copies elsewhere differ.
    #[serde(default)]
    pub(crate) acp_adapter: Option<QualifiedBinary>,
    pub(crate) endpoint_id: ProviderEndpoint,
    pub(crate) session_id: String,
    /// The verifier's own Session identity, presented as the ACP actor when it reads
    /// the Router delivery queue around each rejected selection (J2 negatives).
    #[serde(default)]
    pub(crate) queue_observer: Option<ObserverIdentity>,
    pub(crate) fixture: PathBuf,
    pub(crate) native_home_skill_roots: Vec<PathBuf>,
    pub(crate) native_workspace_skill_dirs: Vec<String>,
    /// Legacy command roots (Claude `.claude/commands`): their `.md` files are skills.
    #[serde(default)]
    pub(crate) native_home_command_roots: Vec<PathBuf>,
    #[serde(default)]
    pub(crate) native_workspace_command_dirs: Vec<String>,
    /// The marker the provider's loader uses to find the project root above the cwd
    /// (Cursor: `.git`, falling back to the cwd itself); None means the cwd is the root.
    #[serde(default)]
    pub(crate) native_project_root_marker: Option<String>,
}

impl NativeCapabilitiesPlan {
    pub(crate) fn service_directory(&self) -> PathBuf {
        self.router_root.join("agent-communication")
    }

    /// Where the Host keeps endpoint sockets, under the canonical Router root.
    pub(crate) fn router_sessions_directory(&self) -> PathBuf {
        self.service_directory().join("router-sessions")
    }

    pub(crate) fn endpoint_socket_name(&self) -> String {
        format!("{}.sock", self.endpoint_id.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct RpcErrorView {
    pub(crate) code: i64,
    pub(crate) message: String,
}

/// One JSON-RPC reply: exactly one of `result` or `error` decides the outcome.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct RpcReply {
    pub(crate) id: u64,
    #[serde(default)]
    pub(crate) result: Option<Value>,
    #[serde(default)]
    pub(crate) error: Option<RpcErrorView>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct SkillView {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) scope: String,
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct SkillErrorView {
    pub(crate) path: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct SkillsEntryView {
    pub(crate) cwd: String,
    pub(crate) skills: Vec<SkillView>,
    pub(crate) errors: Vec<SkillErrorView>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct SkillsListView {
    pub(crate) data: Vec<SkillsEntryView>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginSummaryView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) installed: bool,
    pub(crate) enabled: bool,
    pub(crate) install_policy: String,
    pub(crate) auth_policy: String,
    #[serde(default)]
    pub(crate) remote_plugin_id: Option<String>,
    #[serde(default)]
    pub(crate) share_context: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct MarketplaceView {
    pub(crate) name: String,
    pub(crate) path: Option<String>,
    pub(crate) plugins: Vec<PluginSummaryView>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarketplaceErrorView {
    pub(crate) marketplace_path: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginListView {
    pub(crate) marketplaces: Vec<MarketplaceView>,
    #[serde(default)]
    pub(crate) marketplace_load_errors: Vec<MarketplaceErrorView>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct SkillSummaryView {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HookSummaryView {
    pub(crate) key: String,
    pub(crate) event_name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginDetailView {
    pub(crate) marketplace_name: String,
    pub(crate) summary: PluginSummaryView,
    pub(crate) description: Option<String>,
    pub(crate) skills: Vec<SkillSummaryView>,
    pub(crate) hooks: Vec<HookSummaryView>,
    pub(crate) mcp_servers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct PluginReadView {
    pub(crate) plugin: PluginDetailView,
}

/// Allow-listed fields of one `claude plugin list --json` row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativePluginRow {
    pub(crate) id: String,
    pub(crate) enabled: bool,
    pub(crate) install_path: PathBuf,
    /// The provider's active package folder when it overrides `installPath`.
    #[serde(default)]
    pub(crate) read_from_folder: Option<PathBuf>,
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) scope: Option<String>,
}

impl NativePluginRow {
    /// The folder the provider actually loads: `readFromFolder`, else `installPath`.
    pub(crate) fn active_root(&self) -> &Path {
        self.read_from_folder
            .as_deref()
            .unwrap_or(self.install_path.as_path())
    }

    /// `name@marketplace` for an ordinary native key; None for special keys such as
    /// `name@npm`, `session[...]`, a missing part or a repeated `@`.
    pub(crate) fn regular_key(&self) -> Option<(&str, &str)> {
        let (name, marketplace) = self.id.split_once('@')?;
        let special = name.is_empty()
            || marketplace.is_empty()
            || marketplace.contains('@')
            || marketplace == "npm"
            || self.id.contains('[');
        (!special).then_some((name, marketplace))
    }
}

/// One marketplace from the provider's own export with its actual manifest file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeMarketplace {
    pub(crate) name: String,
    pub(crate) manifest_path: Option<PathBuf>,
}

/// Per-item facts read from one installed plugin's own files; None means unobserved.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativePluginFacts {
    pub(crate) plugin_id: String,
    pub(crate) skill_descriptions: std::collections::BTreeMap<String, String>,
    pub(crate) hook_events: Option<Vec<String>>,
    pub(crate) mcp_servers: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct HistoryInput {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) text: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct HistoryItem {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) content: Vec<HistoryInput>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct HistoryTurn {
    pub(crate) id: String,
    pub(crate) items: Vec<HistoryItem>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct HistoryThread {
    pub(crate) id: String,
    pub(crate) turns: Vec<HistoryTurn>,
}

/// One Router-owned durable provider operation; never a native-provider counter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperationRow {
    pub(crate) operation_id: String,
    pub(crate) operation_kind: String,
    pub(crate) stage: String,
    pub(crate) admitted_at_ms: i64,
    pub(crate) terminal_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ItemKind {
    pub(crate) kind: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderItem {
    pub(crate) item_id: String,
    pub(crate) kind: ItemKind,
    #[serde(default)]
    pub(crate) text: Option<String>,
}

/// How a turn ended (`turnEnded.outcome`), e.g. `{kind: ended, stop_reason: endTurn}`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct TurnOutcomeView {
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) stop_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObservedEvent {
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) item: Option<ProviderItem>,
    #[serde(default)]
    pub(crate) turn_id: Option<String>,
    #[serde(default)]
    pub(crate) outcome: Option<TurnOutcomeView>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct ObservedHubEvent {
    pub(crate) sequence: u64,
    pub(crate) event: ObservedEvent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObservedEndpoint {
    pub(crate) service_id: String,
    pub(crate) endpoint_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObservedTarget {
    pub(crate) endpoint: ObservedEndpoint,
    pub(crate) session_id: String,
}

/// One bounded public `events observe` page, or several pages merged in order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderObservation {
    pub(crate) target: ObservedTarget,
    pub(crate) events: Vec<ObservedHubEvent>,
    pub(crate) epoch: u64,
    pub(crate) end_reason: String,
}

/// Continue after `after_sequence` within the hub epoch that numbered it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObservationCursor {
    pub(crate) after_sequence: u64,
    pub(crate) epoch: u64,
}

impl ProviderObservation {
    pub(crate) fn cursor(&self) -> ObservationCursor {
        ObservationCursor {
            after_sequence: self
                .events
                .iter()
                .map(|entry| entry.sequence)
                .max()
                .unwrap_or(0),
            epoch: self.epoch,
        }
    }
}

/// How an explicit JSON-RPC error answers a capability: never a pass of the experience.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub(crate) enum CapabilityStatus {
    Supported,
    ExplicitUnsupported { message: String },
    NoNativeReference,
}
