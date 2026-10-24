"""Target qualification and read-only evidence collection for the native-capabilities journeys.

Qualification reuses the owned-runtime checks unchanged: strict debug signatures and
hashes (`qualify_normal_binaries`), the live Router identity, health and Host
publication (`verify_ready`), the started Host's recorded process identity, and the
Host's own Claude selection (`qualify_global_claude`). Evidence functions only read:
consumer requests on the endpoint socket, bounded public `events observe`, read-only
Router operation rows, the native provider's own plugin and marketplace exports and
files, and the installed client's own session recorder. Nothing here writes except
evidence files in the run's own directory.
"""

import asyncio
import hashlib
import json
import os
import re
import sqlite3
import subprocess
from collections import abc
from dataclasses import dataclass
from pathlib import Path

from pydantic import JsonValue, TypeAdapter
from websockets.asyncio.client import unix_connect

from scripts.verification.acp_metadata_observation import (
    ObservedHubEvent,
    ProviderObservation,
    save_raw,
)
from scripts.verification.native_capabilities_models import (
    FixtureRecord,
    NativeCapabilitiesPlan,
    NativeMarketplace,
    NativePluginFacts,
    NativePluginRow,
    OperationRow,
    RpcReply,
)
from scripts.verification.owned_runtime import (
    RuntimePlan,
    current_identity_fields,
    verify_ready,
)
from scripts.verification.process_ownership import same_identity
from scripts.verification.runtime_qualification import (
    qualify_global_claude,
    qualify_normal_binaries,
    sha256,
)
from scripts.verification.typed_observations import (
    EvidenceView,
    HostManifest,
    JsonObject,
    read_observed_json,
)

# `events observe` parser bounds (crates/agent-collaboration/src/event_commands.rs:49-79).
OBSERVE_MAX_EVENTS = 4096
OBSERVE_MAX_BYTES = 1_048_576
OBSERVE_MAX_PAGES = 64
# Schema: crates/collaboration-service/migrations/202609200001_provider_operations.sql
OPERATIONS_FOR_SESSION = "SELECT operation_id, operation_kind, stage, admitted_at_ms, terminal_at_ms FROM provider_operations WHERE target_session_id = ? ORDER BY admitted_at_ms, operation_id"

_native_rows = TypeAdapter(list[dict[str, JsonValue]])


class NativeMarketplaceRow(EvidenceView):
    """Used fields of one `claude plugin marketplace list --json` row."""

    name: str
    installLocation: str


_native_marketplace_rows = TypeAdapter(list[NativeMarketplaceRow])


@dataclass(frozen=True, kw_only=True)
class QualifiedTarget:
    plan: NativeCapabilitiesPlan
    runtime: RuntimePlan
    fixture: FixtureRecord
    service_directory: Path
    collaboration_cli: Path
    native_executable: Path


def git_output(repository: Path, *arguments: str) -> str:
    return subprocess.check_output(
        ["git", *arguments], cwd=repository, text=True
    ).strip()


def qualify_target(plan: NativeCapabilitiesPlan, evidence: Path) -> QualifiedTarget:
    """Refuse to observe anything but the declared committed, signed, running candidate."""
    runtime = RuntimePlan.model_validate_json(plan.runtime_plan.read_bytes())
    if runtime.router_root.resolve() == (Path.home() / ".codex-router").resolve():
        raise ValueError("production Router root is outside this journey")
    head = git_output(runtime.repository, "rev-parse", "HEAD")
    if head != runtime.source_head or git_output(
        runtime.repository, "status", "--porcelain"
    ):
        raise ValueError("candidate source is not the declared clean commit")
    binaries = qualify_normal_binaries(runtime.qualification)
    ready = verify_ready(runtime)
    phase = runtime.evidence / f"host-{plan.host_phase}"
    if phase.with_name(f"{phase.name}-settlement.json").exists():
        raise ValueError("declared Host phase has already been settled")
    started = read_observed_json(
        phase.with_name(f"{phase.name}-started.json").read_bytes()
    )
    identity = started["identity"]
    argv = started["argv"]
    if (
        not isinstance(identity, dict)
        or not same_identity(
            current_identity_fields(int(str(started["pid"]))), identity
        )
        or not isinstance(argv, list)
        or Path(str(argv[0])).resolve() != Path(binaries["codex-router"].path).resolve()
    ):
        raise ValueError("declared Host phase is not the live signed candidate Host")
    if sha256(plan.codex_cli) != plan.codex_sha256:
        raise ValueError("installed Codex hash mismatch")
    native, native_record = qualify_native_executable(plan, runtime)
    save_raw(
        evidence / "qualification.json",
        json.dumps(
            {
                "sourceHead": head,
                "binaries": {
                    name: {"path": item.path, "sha256": item.sha256}
                    for name, item in binaries.items()
                },
                "router": ready,
                "host": {"phase": plan.host_phase, "started": started},
                "codex": {"path": str(plan.codex_cli), "sha256": plan.codex_sha256},
                "native": native_record,
            },
            indent=2,
            default=str,
        ),
    )
    return QualifiedTarget(
        plan=plan,
        runtime=runtime,
        fixture=FixtureRecord.model_validate_json(plan.fixture.read_bytes()),
        service_directory=runtime.router_root / "agent-communication",
        collaboration_cli=Path(binaries["agent-collaboration"].path),
        native_executable=native,
    )


def qualify_native_executable(
    plan: NativeCapabilitiesPlan, runtime: RuntimePlan
) -> tuple[Path, JsonObject]:
    """The native executable the Host binding uses, with its version, hash and scope."""
    if plan.endpoint_id == "claude-local":
        environment = os.environ.copy()
        _ = environment.pop("CLAUDE_CODE_EXECUTABLE", None)
        binding = qualify_global_claude(
            runtime.claude_executable, environment.get("PATH")
        )
        launched = read_observed_json(
            (runtime.evidence / "driver-binding.json").read_bytes()
        )["claudeGlobalExecutable"]
        if launched != binding.model_dump(mode="json"):
            raise ValueError("Claude binding changed since the Host launched")
        executable = binding.canonicalPath
    else:
        executable = runtime.cursor_executable.resolve(strict=True)
    version = subprocess.run(
        [str(executable), "--version"],
        capture_output=True,
        text=True,
        timeout=30,
        check=True,
    ).stdout.strip()
    return executable, {
        "endpoint": plan.endpoint_id,
        "path": str(executable),
        "sha256": sha256(executable),
        "version": version,
        # The Host launches providers with the normal home; no config-dir override.
        "home": str(Path.home()),
        "configDirectoryOverride": "CLAUDE_CONFIG_DIR" in os.environ,
    }


def endpoint_socket(target: QualifiedTarget) -> Path:
    return (
        target.service_directory / "router-sessions" / f"{target.plan.endpoint_id}.sock"
    )


async def _consumer_requests(
    socket: Path,
    requests: list[tuple[str, dict[str, JsonValue]]],
    evidence: Path,
    label: str,
) -> list[RpcReply]:
    replies: list[RpcReply] = []
    async with unix_connect(
        str(socket), uri="ws://localhost/rpc", max_size=16_777_216
    ) as client:
        initialize: dict[str, JsonValue] = {
            "clientInfo": {"name": "native-capabilities-verifier", "version": "1"}
        }
        for request_id, (method, params) in enumerate(
            [("initialize", initialize), *requests], start=1
        ):
            await client.send(
                json.dumps({"id": request_id, "method": method, "params": params})
            )
            while True:
                raw = await asyncio.wait_for(client.recv(), timeout=30)
                text = raw.decode() if isinstance(raw, bytes) else raw
                if read_observed_json(text).get("id") == request_id:
                    break
            save_raw(
                evidence / f"{label}-{request_id}-{method.replace('/', '-')}.json", text
            )
            if request_id > 1:
                replies.append(RpcReply.model_validate_json(text))
    return replies


def consumer_requests(
    target: QualifiedTarget,
    requests: list[tuple[str, dict[str, JsonValue]]],
    evidence: Path,
    label: str,
) -> list[RpcReply]:
    """Send app-server requests as the installed client would, retaining raw replies."""
    return asyncio.run(
        _consumer_requests(endpoint_socket(target), requests, evidence, label)
    )


@dataclass(frozen=True, kw_only=True)
class ObservationCursor:
    """Continue after `after_sequence` within the hub epoch that numbered it."""

    after_sequence: int
    epoch: int


def observation_cursor(observation: ProviderObservation) -> ObservationCursor:
    return ObservationCursor(
        after_sequence=max((entry.sequence for entry in observation.events), default=0),
        epoch=observation.epoch,
    )


def observe_command(
    collaboration_cli: Path,
    endpoint_id: str,
    session_id: str,
    service_directory: Path,
    cursor: ObservationCursor | None,
) -> list[str]:
    """One `events observe` page within the CLI's parser bounds."""
    command = [
        str(collaboration_cli),
        "events",
        "observe",
        "--endpoint",
        endpoint_id,
        "--session",
        session_id,
        "--attach",
        "--timeout-seconds",
        "2",
        "--max-events",
        str(OBSERVE_MAX_EVENTS),
        "--max-bytes",
        str(OBSERVE_MAX_BYTES),
        "--service-directory",
        str(service_directory),
    ]
    if cursor is not None:
        # The parser requires --epoch with --after-sequence; it is the observed epoch.
        command += [
            "--after-sequence",
            str(cursor.after_sequence),
            "--epoch",
            str(cursor.epoch),
        ]
    return command


def paginate_observation(
    fetch: abc.Callable[[ObservationCursor | None, int], ProviderObservation],
    cursor: ObservationCursor | None,
) -> ProviderObservation:
    """Follow result-limited pages until the deadline ends the observation.

    Mirrors the client's own follower (provider_session_observation.rs `accept`): the
    cursor advances to the highest sequence within one epoch. An epoch change, resync,
    disconnect or page overflow fails instead of returning a partial history as complete.
    """
    events: list[ObservedHubEvent] = []
    for page in range(OBSERVE_MAX_PAGES):
        observed = fetch(cursor, page)
        if cursor is not None and observed.epoch != cursor.epoch:
            raise ValueError("observation epoch changed: retained history was reset")
        events.extend(observed.events)
        if observed.endReason == "deadlineReached":
            return ProviderObservation(
                target=observed.target,
                events=events,
                epoch=observed.epoch,
                endReason="deadlineReached",
            )
        if observed.endReason != "resultLimitReached" or not observed.events:
            raise ValueError(
                f"observation ended with {observed.endReason}; resync is required"
            )
        cursor = observation_cursor(observed)
    raise ValueError("observation did not complete within its page bound")


def observe_session(
    target: QualifiedTarget,
    evidence: Path,
    label: str,
    cursor: ObservationCursor | None,
) -> ProviderObservation:
    """A complete bounded public observation of the exact declared Session."""
    plan = target.plan
    manifest = HostManifest.model_validate_json(
        (target.service_directory / "service.json").read_bytes()
    )

    def fetch(page_cursor: ObservationCursor | None, page: int) -> ProviderObservation:
        completed = subprocess.run(
            observe_command(
                target.collaboration_cli,
                plan.endpoint_id,
                plan.session_id,
                target.service_directory,
                page_cursor,
            ),
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        save_raw(evidence / f"{label}-observation-{page}.json", completed.stdout)
        save_raw(evidence / f"{label}-observation-{page}.stderr", completed.stderr)
        if completed.returncode != 0:
            raise ValueError(
                "public provider observation failed; raw response retained"
            )
        observed = ProviderObservation.model_validate_json(completed.stdout)
        if (
            observed.target.sessionId != plan.session_id
            or observed.target.endpoint.endpointId != plan.endpoint_id
            or observed.target.endpoint.serviceId != manifest.serviceId
        ):
            raise ValueError("public observation returned a different Session")
        return observed

    return paginate_observation(fetch, cursor)


def operation_rows(
    target: QualifiedTarget, evidence: Path, label: str
) -> list[OperationRow]:
    """Router-owned durable operations for the Session, read in a read-only connection."""
    database = target.service_directory / "provider-operations.sqlite"
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True, timeout=5)
    try:
        rows: list[tuple[str, str, str, int, int | None]] = connection.execute(
            OPERATIONS_FOR_SESSION, (target.plan.session_id,)
        ).fetchall()
    finally:
        connection.close()
    operations = [
        OperationRow(
            operation_id=operation_id,
            operation_kind=operation_kind,
            stage=stage,
            admitted_at_ms=admitted_at_ms,
            terminal_at_ms=terminal_at_ms,
        )
        for operation_id, operation_kind, stage, admitted_at_ms, terminal_at_ms in rows
    ]
    save_raw(
        evidence / f"{label}-operations.json",
        json.dumps([row.model_dump() for row in operations], indent=2),
    )
    return operations


def native_plugin_rows(
    target: QualifiedTarget, evidence: Path, label: str
) -> list[NativePluginRow] | None:
    """The bound Claude executable's own installed-plugin export, allow-listed fields only.

    Returns None for a provider with no qualified native plugin export (Cursor).
    """
    if target.plan.endpoint_id != "claude-local":
        return None
    completed = subprocess.run(
        [str(target.native_executable), "plugin", "list", "--json"],
        cwd=target.fixture.workspace,
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    if completed.returncode != 0:
        raise ValueError("native plugin export failed; its output was not retained")
    rows = [
        NativePluginRow.model_validate(
            {
                key: value
                for key, value in row.items()
                if key in {"id", "enabled", "installPath", "version", "scope"}
            }
        )
        for row in _native_rows.validate_json(completed.stdout)
    ]
    save_raw(
        evidence / f"{label}-native-plugins.json",
        json.dumps([row.model_dump() for row in rows], indent=2),
    )
    return rows


def frontmatter_description(skill_file: Path) -> str:
    """The single-line `description` from a SKILL.md frontmatter block."""
    lines = skill_file.read_text().splitlines()
    if not lines or lines[0].strip() != "---":
        raise ValueError(f"native skill {skill_file} has no frontmatter")
    for line in lines[1:]:
        if line.strip() == "---":
            break
        if match := re.fullmatch(r"description:\s*(.*)", line):
            value = match.group(1).strip()
            if value in {"", "|", ">", "|-", ">-"}:
                raise ValueError(
                    f"native skill {skill_file} description is not one line"
                )
            return value.strip("'\"")
    raise ValueError(f"native skill {skill_file} declares no description")


def declared_names(
    install: Path, manifest: JsonObject, key: str, file: str
) -> list[str] | None:
    """Names a plugin declares under `key`, from its manifest or its own file.

    No declaration anywhere is a known-empty list. An unreadable or non-object
    declaration raises, and the caller records it as unobserved.
    """
    declared = manifest.get(key)
    source: JsonObject
    if isinstance(declared, dict):
        source = declared
    elif isinstance(declared, str):
        source = read_observed_json((install / declared).read_bytes())
    elif (install / file).exists():
        source = read_observed_json((install / file).read_bytes())
    else:
        return []
    # Declaration files may wrap the mapping in their own `key` object.
    nested = source.get(key)
    if isinstance(nested, dict):
        source = nested
    return sorted(source)


def native_marketplaces(
    target: QualifiedTarget, evidence: Path, label: str
) -> list[NativeMarketplace] | None:
    """The bound Claude executable's own marketplace export with each manifest file.

    `plugin marketplace list --json` rows carry `installLocation`; the manifest is that
    location's `.claude-plugin/marketplace.json` when the file exists, else unobserved.
    Returns None for a provider with no qualified marketplace export (Cursor).
    """
    if target.plan.endpoint_id != "claude-local":
        return None
    completed = subprocess.run(
        [str(target.native_executable), "plugin", "marketplace", "list", "--json"],
        cwd=target.fixture.workspace,
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    if completed.returncode != 0:
        raise ValueError(
            "native marketplace export failed; its output was not retained"
        )
    marketplaces: list[NativeMarketplace] = []
    for row in _native_marketplace_rows.validate_json(completed.stdout):
        manifest = Path(row.installLocation) / ".claude-plugin" / "marketplace.json"
        marketplaces.append(
            NativeMarketplace(
                name=row.name, manifest_path=manifest if manifest.is_file() else None
            )
        )
    save_raw(
        evidence / f"{label}-native-marketplaces.json",
        json.dumps(
            [market.model_dump(mode="json") for market in marketplaces], indent=2
        ),
    )
    return marketplaces


def plugin_skill_files(row: NativePluginRow) -> list[Path]:
    """The installed plugin's own `skills/<name>/SKILL.md` files."""
    return sorted((Path(row.installPath) / "skills").glob("*/SKILL.md"))


def native_plugin_facts(
    row: NativePluginRow, evidence: Path, label: str
) -> NativePluginFacts:
    """Per-item facts from one installed plugin's own files, retaining names only."""
    install = Path(row.installPath)
    manifest_file = install / ".claude-plugin" / "plugin.json"
    manifest = (
        read_observed_json(manifest_file.read_bytes()) if manifest_file.exists() else {}
    )
    skills = {
        skill_file.parent.name: frontmatter_description(skill_file)
        for skill_file in plugin_skill_files(row)
    }
    try:
        hooks = declared_names(install, manifest, "hooks", "hooks/hooks.json")
    except (ValueError, OSError):
        hooks = None
    try:
        servers = declared_names(install, manifest, "mcpServers", ".mcp.json")
    except (ValueError, OSError):
        servers = None
    facts = NativePluginFacts(
        plugin_id=row.id,
        skill_descriptions=skills,
        hook_events=hooks,
        mcp_servers=servers,
    )
    save_raw(
        evidence / f"{label}-native-plugin-facts.json", facts.model_dump_json(indent=2)
    )
    return facts


def read_session_recorder(log: Path) -> list[JsonObject]:
    """Records from the installed client's own CODEX_TUI_RECORD_SESSION log."""
    if not log.exists():
        raise ValueError("installed client wrote no session recorder log")
    return [read_observed_json(line) for line in log.read_text().splitlines() if line]


def codex_config_digest() -> str:
    """Digest of the global Codex config, compared around each drive (R5)."""
    config = Path.home() / ".codex" / "config.toml"
    if not config.exists():
        return "absent"
    return hashlib.sha256(config.read_bytes()).hexdigest()
