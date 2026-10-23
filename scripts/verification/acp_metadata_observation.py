"""Read back one real ACP metadata journey after an installed Codex PTY drive.

Run: python -m scripts.verification.acp_metadata_observation PLAN.json
The caller owns the isolated Host and installed-client drive. This script sends
only thread/read, thread/resume and bounded observation to the exact declared
Session. It retains raw responses and rejects metadata-as-prose, lost metadata,
an absent ordinary-reply control, a wrong build, or incomplete observation.
It never creates a Session, sends model input, logs in or stops a Host.
"""

import argparse
import asyncio
import hashlib
import json
import re
import subprocess
import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field
from websockets.asyncio.client import unix_connect

from scripts.verification.process_ownership import lookup_process_identity
from scripts.verification.typed_observations import (
    HostManifest,
    SessionView,
    observed_object,
    read_observed_json,
)


class MetadataObservationPlan(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid")
    repository: Path
    source_head: str
    build_manifest: Path
    host_pid: int = Field(gt=0)
    host_binary: Path
    host_sha256: str
    router_binary: Path
    router_sha256: str
    codex_cli: Path
    codex_sha256: str
    collaboration_cli: Path
    service_directory: Path
    endpoint_id: t.Literal["claude-local", "cursor-local"]
    session_id: str
    terminal_capture: Path
    resume_capture: Path
    expected_reply: str = Field(min_length=1)
    evidence: Path
    require_usage: bool = False


class BuildArtifact(BaseModel):
    path: Path
    sha256: str


class NormalBuildManifest(BaseModel):
    source_commit: str
    source_tree: str
    dirty: bool
    test_guard_features: list[str]
    binaries: dict[str, BuildArtifact]


class ProviderItemKind(BaseModel):
    kind: str


class ProviderItem(BaseModel):
    itemId: str
    kind: ProviderItemKind
    text: str | None = None


class ObservedEvent(BaseModel):
    kind: str
    item: ProviderItem | None = None
    turnId: str | None = None


class ObservedHubEvent(BaseModel):
    sequence: int
    event: ObservedEvent


class ProviderObservation(BaseModel):
    target: SessionView
    events: list[ObservedHubEvent]
    epoch: int
    endReason: str


class CodexItem(BaseModel):
    id: str
    type: str
    text: str | None = None


class CodexTurn(BaseModel):
    id: str
    items: list[CodexItem]


class CodexThread(BaseModel):
    id: str
    turns: list[CodexTurn]


class MetadataObservationResult(BaseModel):
    thread_id: str
    metadata_item_ids: list[str]
    metadata_kinds: list[str]
    command_catalogues: int
    usage_items: int
    assistant_item_ids: list[str]
    historical_views: int
    terminal_reply_observed: bool
    terminal_views: int
    usage_limit: str | None


def file_sha256(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def terminal_text(raw: bytes) -> str:
    text = raw.decode("utf-8", errors="replace").replace("\r\n", "\n")
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)
    return re.sub(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)", "", text)


def is_command_catalogue(item: ProviderItem) -> bool:
    if item.kind.kind != "notice" or item.text is None:
        return False
    try:
        payload = read_observed_json(item.text)
    except ValueError:
        return False
    return isinstance(payload.get("availableCommands"), list)


def require_terminal_prose(
    capture: str, expected_reply: str, metadata: list[ProviderItem]
) -> None:
    # The synthetic prompt spells the marker as separate words. Require its
    # actual Codex assistant row, not a title or an arbitrary substring.
    if (
        re.search(
            r"(?m)^\s*•[ \t]+" + re.escape(expected_reply) + r"(?=[ \t]*(?:$|›))",
            capture,
        )
        is None
    ):
        raise ValueError(
            "ordinary assistant reply absent from original installed-client PTY"
        )
    compact_capture = re.sub(r"\s+", "", capture)
    for item in metadata:
        if item.text is None:
            continue
        try:
            payload = read_observed_json(item.text)
        except ValueError:
            if re.search(r"(?m)^\s*•\s+" + re.escape(item.text) + r"\s*$", capture):
                raise ValueError("provider metadata appeared as terminal prose")
            continue
        # Whitespace normalization catches keys split across terminal wraps.
        if any(
            re.sub(r"\s+", "", json.dumps(key) + ":") in compact_capture
            for key in payload
        ):
            raise ValueError("raw provider metadata appeared in installed-client PTY")


def check_metadata_observation(
    observation: ProviderObservation,
    views: list[CodexThread],
    terminal: str,
    resumed_terminal: str,
    expected_reply: str,
    require_usage: bool,
) -> MetadataObservationResult:
    if observation.endReason != "deadlineReached":
        raise ValueError(
            "observation incomplete: page, reset or disconnect needs reconciliation"
        )
    retained: dict[str, ProviderItem] = {}
    for entry in observation.events:
        event = entry.event
        if event.kind in {"itemStarted", "itemUpdated"} and event.item is not None:
            retained[event.item.itemId] = event.item
    metadata = [
        item
        for item in retained.values()
        if item.kind.kind in {"usage", "modeChange", "configChange", "sessionInfo"}
        or is_command_catalogue(item)
    ]
    commands = sum(is_command_catalogue(item) for item in metadata)
    usage = sum(item.kind.kind == "usage" for item in metadata)
    if commands == 0 or (require_usage and usage == 0):
        raise ValueError("required provider metadata was not retained")
    metadata_ids = {item.itemId for item in metadata}
    assistants = [
        item
        for item in retained.values()
        if item.kind.kind == "agentMessage" and item.text == expected_reply
    ]
    if not assistants:
        raise ValueError(
            "ordinary assistant reply absent from original provider events"
        )
    if len(views) != 2 or any(
        view.id != observation.target.sessionId for view in views
    ):
        raise ValueError("both read and resume views of the same thread are required")
    assistant_ids = {item.itemId for item in assistants}
    for view in views:
        rendered = [item for turn in view.turns for item in turn.items]
        if metadata_ids.intersection(item.id for item in rendered):
            raise ValueError("provider metadata leaked into a Codex transcript item")
        if not any(
            item.type == "agentMessage"
            and item.id in assistant_ids
            and item.text == expected_reply
            for item in rendered
        ):
            raise ValueError("ordinary assistant reply missing from Codex history")
    for capture in (terminal, resumed_terminal):
        require_terminal_prose(capture, expected_reply, metadata)
    return MetadataObservationResult(
        thread_id=views[0].id,
        metadata_item_ids=sorted(metadata_ids),
        metadata_kinds=sorted({item.kind.kind for item in metadata}),
        command_catalogues=commands,
        usage_items=usage,
        assistant_item_ids=sorted(assistant_ids),
        historical_views=len(views),
        terminal_reply_observed=True,
        terminal_views=2,
        usage_limit=None
        if usage
        else "Provider emitted no usage; context/cost runtime proof remains unobserved",
    )


def save_raw(path: Path, body: str) -> None:
    with path.open("x") as output:
        _ = output.write(body)
    path.chmod(0o600)


async def read_codex_views(plan: MetadataObservationPlan) -> list[CodexThread]:
    socket = plan.service_directory / "router-sessions" / f"{plan.endpoint_id}.sock"
    views: list[CodexThread] = []
    async with unix_connect(
        str(socket), uri="ws://localhost/rpc", max_size=16_777_216
    ) as client:
        for request_id, method, params in [
            (
                1,
                "initialize",
                {"clientInfo": {"name": "metadata-verifier", "version": "1"}},
            ),
            (2, "thread/read", {"threadId": plan.session_id, "includeTurns": True}),
            (3, "thread/resume", {"threadId": plan.session_id, "excludeTurns": False}),
        ]:
            await client.send(
                json.dumps({"id": request_id, "method": method, "params": params})
            )
            while True:
                raw = await asyncio.wait_for(client.recv(), timeout=15)
                response = read_observed_json(
                    raw.decode() if isinstance(raw, bytes) else raw
                )
                if response.get("id") == request_id:
                    break
            save_raw(
                plan.evidence / f"app-server-{request_id}.json",
                raw.decode() if isinstance(raw, bytes) else raw,
            )
            if "error" in response:
                raise ValueError(f"{method} failed; raw response retained")
            if request_id != 1:
                view = CodexThread.model_validate(
                    observed_object(response["result"])["thread"]
                )
                if view.id != plan.session_id:
                    raise ValueError("app-server returned the wrong thread")
                views.append(view)
    return views


def run(plan: MetadataObservationPlan) -> MetadataObservationResult:
    if plan.service_directory.resolve().is_relative_to(Path.home() / ".codex-router"):
        raise ValueError("production service is outside this journey")
    head = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=plan.repository, text=True
    ).strip()
    if head != plan.source_head:
        raise ValueError("source revision mismatch")
    manifest = NormalBuildManifest.model_validate_json(plan.build_manifest.read_bytes())
    tree = subprocess.check_output(
        ["git", "rev-parse", f"{head}^{{tree}}"], cwd=plan.repository, text=True
    ).strip()
    dirty = subprocess.check_output(
        ["git", "status", "--porcelain"], cwd=plan.repository, text=True
    ).strip()
    if (
        manifest.source_commit != head
        or manifest.source_tree != tree
        or manifest.dirty
        or manifest.test_guard_features
        or dirty
    ):
        raise ValueError("committed normal-build source qualification mismatch")
    for name, path, expected in [
        ("automation-debug-host", plan.host_binary, plan.host_sha256),
        ("codex-router", plan.router_binary, plan.router_sha256),
    ]:
        artifact = manifest.binaries.get(name)
        if (
            artifact is None
            or artifact.path.resolve() != path.resolve()
            or artifact.sha256 != expected
        ):
            raise ValueError("binary is not bound to the declared build manifest")
    for binary, expected in [
        (plan.host_binary, plan.host_sha256),
        (plan.router_binary, plan.router_sha256),
        (plan.codex_cli, plan.codex_sha256),
    ]:
        if file_sha256(binary) != expected:
            raise ValueError("declared binary hash mismatch")
    identity = lookup_process_identity(plan.host_pid)
    executable = identity.get("executable") if identity is not None else None
    if (
        not isinstance(executable, str)
        or Path(executable).resolve() != plan.host_binary.resolve()
    ):
        raise ValueError("owning Host identity mismatch")
    plan.evidence.mkdir(mode=0o700)
    save_raw(plan.evidence / "binding.json", plan.model_dump_json(indent=2))
    save_raw(plan.evidence / "build-manifest.json", plan.build_manifest.read_text())
    completed = subprocess.run(
        [
            str(plan.collaboration_cli),
            "events",
            "observe",
            "--endpoint",
            plan.endpoint_id,
            "--session",
            plan.session_id,
            "--attach",
            "--timeout-seconds",
            "2",
            "--max-events",
            "4096",
            "--max-bytes",
            "1048576",
            "--service-directory",
            str(plan.service_directory),
        ],
        capture_output=True,
        text=True,
        timeout=20,
        check=False,
    )
    save_raw(plan.evidence / "provider-observation.json", completed.stdout)
    save_raw(plan.evidence / "provider-observation.stderr", completed.stderr)
    if completed.returncode != 0:
        raise ValueError("public provider observation failed; raw response retained")
    observation = ProviderObservation.model_validate_json(completed.stdout)
    manifest = HostManifest.model_validate_json(
        (plan.service_directory / "service.json").read_bytes()
    )
    if (
        observation.target.sessionId != plan.session_id
        or observation.target.endpoint.endpointId != plan.endpoint_id
        or observation.target.endpoint.serviceId != manifest.serviceId
    ):
        raise ValueError("public observation returned a different Session")
    views = asyncio.run(read_codex_views(plan))
    result = check_metadata_observation(
        observation,
        views,
        terminal_text(plan.terminal_capture.read_bytes()),
        terminal_text(plan.resume_capture.read_bytes()),
        plan.expected_reply,
        plan.require_usage,
    )
    save_raw(plan.evidence / "result.json", result.model_dump_json(indent=2))
    return result


class MetadataArguments(argparse.Namespace):
    plan: Path = Path()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("plan", type=Path)
    arguments = parser.parse_args(namespace=MetadataArguments())
    plan = MetadataObservationPlan.model_validate_json(
        Path(arguments.plan).read_bytes()
    )
    print(run(plan).model_dump_json(indent=2))


if __name__ == "__main__":
    main()
