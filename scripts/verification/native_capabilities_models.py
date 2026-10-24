"""Typed plan, fixture record and evidence views for the native-capabilities journeys.

Consumer views mirror the installed Codex 0.163 app-server shapes the journeys read
(SkillsListResponse, PluginListResponse, PluginReadResponse, ThreadItem::UserMessage).
They validate only the fields an oracle uses; raw responses stay authoritative on disk.
"""

import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field, JsonValue

from scripts.verification.typed_observations import EvidenceView


class FixtureRecord(BaseModel):
    """Controlled native skills materialized into owned workspaces for one run."""

    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")
    workspace: Path
    empty_workspace: Path
    visible_file: Path
    skill_name: str
    skill_file: Path
    canary: str
    second_skill_name: str
    second_skill_file: Path
    second_canary: str


class NativeCapabilitiesPlan(BaseModel):
    """One journey run against the candidate an `owned_runtime` RuntimePlan launched.

    Repository, source, signed binaries, Router root and native executables all come
    from that RuntimePlan and its qualification; nothing is declared twice.
    """

    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid")
    runtime_plan: Path
    host_phase: t.Literal["A", "B"]
    codex_cli: Path
    codex_sha256: str
    endpoint_id: t.Literal["claude-local", "cursor-local"]
    session_id: str
    fixture: Path
    # The bound provider's own skill roots, declared from its native loader source with
    # anchors in the run notes: absolute home roots plus directories relative to the
    # provider's project root for the requested cwd. A shared root such as
    # ~/.codex/skills belongs here only when that provider's own loader reads it.
    native_home_skill_roots: list[Path]
    native_workspace_skill_dirs: list[str]
    # The marker the provider's loader uses to find the project root above the cwd
    # (Cursor: the nearest `.git`); None means the cwd itself is the root.
    native_project_root_marker: str | None = None
    # SKILL.md frontmatter values that make the provider's own loader skip a skill
    # (Cursor: `user-invocable: false`, CustomCommandsLoader). Declared from native
    # source with anchors; empty means no disabled fact is qualified for this provider.
    native_disabled_markers: dict[str, str] = Field(default_factory=dict)


class NativeSkillAuthority(BaseModel):
    """What the bound provider itself loads: its advertised names, roots and namespaces."""

    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")
    advertised_names: frozenset[str]
    home_roots: tuple[Path, ...]
    workspace_dirs: tuple[str, ...]
    project_root_marker: str | None = None
    # Installed plugin directories and the namespace the provider advertises their
    # skills under (`namespace:skill`), from the provider's own plugin export.
    plugin_namespaces: tuple[tuple[Path, str], ...] = ()
    # Frontmatter key/value pairs that the provider's loader treats as disabled.
    disabled_markers: tuple[tuple[str, str], ...] = ()

    def project_root(self, cwd: Path) -> Path:
        """The nearest ancestor holding the marker, else the cwd itself.

        The cwd fallback is native semantics, not a guess: Cursor's installed
        `utils/git.ts` root finder returns its input cwd when no `.git` is found.
        """
        if self.project_root_marker is None:
            return cwd
        for candidate in (cwd, *cwd.parents):
            if (candidate / self.project_root_marker).exists():
                return candidate
        return cwd

    def roots_for(self, cwd: Path) -> list[Path]:
        root = self.project_root(cwd)
        return [
            *self.home_roots,
            *(root / directory for directory in self.workspace_dirs),
        ]

    def namespace_for(self, path: Path) -> str | None:
        resolved = path.resolve()
        for directory, namespace in self.plugin_namespaces:
            if resolved.is_relative_to(directory.resolve()):
                return namespace
        return None


class NativeMarketplace(BaseModel):
    """One marketplace from the provider's own export, with its actual manifest file.

    `manifest_path` is None when the export names no manifest file that exists.
    """

    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")
    name: str
    manifest_path: Path | None


class AdvertisedCommand(EvidenceView):
    name: str


class CommandCatalogueNotice(EvidenceView):
    """ACP `available_commands_update` as Router retains it in Notice text (C3)."""

    availableCommands: list[AdvertisedCommand]


class RpcErrorView(EvidenceView):
    code: int
    message: str


class RpcReply(EvidenceView):
    id: int
    result: JsonValue = None
    error: RpcErrorView | None = None


class SkillView(EvidenceView):
    name: str
    path: str
    scope: str
    enabled: bool


class SkillErrorView(EvidenceView):
    path: str
    message: str


class SkillsEntryView(EvidenceView):
    cwd: str
    skills: list[SkillView]
    errors: list[SkillErrorView]


class SkillsListView(EvidenceView):
    data: list[SkillsEntryView]


class PluginSummaryView(EvidenceView):
    id: str
    name: str
    installed: bool
    enabled: bool
    installPolicy: str
    authPolicy: str
    remotePluginId: str | None = None
    shareContext: JsonValue = None


class MarketplaceView(EvidenceView):
    name: str
    path: str | None
    plugins: list[PluginSummaryView]


class MarketplaceErrorView(EvidenceView):
    marketplacePath: str
    message: str


class PluginListView(EvidenceView):
    marketplaces: list[MarketplaceView]
    marketplaceLoadErrors: list[MarketplaceErrorView] = Field(default_factory=list)


class SkillSummaryView(EvidenceView):
    name: str
    description: str
    enabled: bool


class HookSummaryView(EvidenceView):
    key: str
    eventName: str


class PluginDetailView(EvidenceView):
    marketplaceName: str
    summary: PluginSummaryView
    description: str | None
    skills: list[SkillSummaryView]
    hooks: list[HookSummaryView]
    mcpServers: list[str]


class PluginReadView(EvidenceView):
    plugin: PluginDetailView


class NativePluginRow(EvidenceView):
    """Allow-listed fields of one `claude plugin list --json` row."""

    id: str
    enabled: bool
    installPath: str
    version: str | None = None
    scope: str | None = None


class RecordedInput(EvidenceView):
    """One UserInput item from the installed client's own session recorder."""

    type: str
    text: str | None = None
    name: str | None = None
    path: str | None = None


class RecordedSelectionCheck(BaseModel):
    item_types: list[str]
    skill_names: list[str]
    text: str
    text_element_count: int


class NativePluginFacts(BaseModel):
    """Per-item facts read from one installed plugin's own files; None means unobserved."""

    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")
    plugin_id: str
    skill_descriptions: dict[str, str]
    hook_events: list[str] | None
    mcp_servers: list[str] | None


class HistoryInput(EvidenceView):
    type: str
    text: str | None = None


class HistoryItem(EvidenceView):
    id: str
    type: str
    text: str | None = None
    content: list[HistoryInput] = Field(default_factory=list)


class HistoryTurn(EvidenceView):
    id: str
    items: list[HistoryItem]


class HistoryThread(EvidenceView):
    id: str
    turns: list[HistoryTurn]


class OperationRow(BaseModel):
    """One Router-owned durable provider operation; not a native-provider counter."""

    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")
    operation_id: str
    operation_kind: str
    stage: str
    admitted_at_ms: int
    terminal_at_ms: int | None


type CapabilityStatus = t.Literal["supported", "explicitUnsupported"]


class SkillCatalogueCheck(BaseModel):
    status: CapabilityStatus
    unsupported_message: str | None = None
    skill_count: int = 0
    error_count: int = 0
    fixture_path: str | None = None


class PluginCatalogueCheck(BaseModel):
    status: CapabilityStatus | t.Literal["noNativeReference"]
    unsupported_message: str | None = None
    installed_ids: list[str] = []
    defaulted_policy_ids: list[str] = []
    load_error_paths: list[str] = []


class CommandCatalogueCheck(BaseModel):
    catalogue_item_ids: list[str]
    current_names: list[str]
    removed_names: list[str]
    replacement: t.Literal["observed", "unobserved"]


class SelectedTurnCheck(BaseModel):
    turn_ids: list[str]
    prompt_operation_ids: list[str]
    reply_item_id: str


class RejectedInputCheck(BaseModel):
    new_operation_ids: list[str]
    client_error_seen: bool


class HistoryCheck(BaseModel):
    thread_id: str
    user_item_ids: list[str]
    reply_item_ids: list[str]
    trimmed_outer_whitespace: bool


class DisplayCheck(BaseModel):
    allowed_kinds: list[str]
    new_operation_kinds: list[str]
