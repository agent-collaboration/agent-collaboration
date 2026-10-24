"""Shared real-file setups and evidence builders for the native-capabilities oracle tests."""

import json
import tempfile
import typing as t
from pathlib import Path

from pydantic import JsonValue

from scripts.verification.acp_metadata_observation import ProviderObservation
from scripts.verification.native_capabilities_models import (
    FixtureRecord,
    NativeSkillAuthority,
    OperationRow,
    RpcReply,
)
from scripts.verification.typed_observations import JsonObject

ARGUMENTS = "literal-$value café\nsecond-line"
CANARY = "NATIVE_SKILL_0123456789ABCDEF"
SECOND_CANARY = "NATIVE_SKILL_FEDCBA9876543210"
SKILL = "router-native-arguments"


def write_skill(root: Path, name: str, extra: str = "") -> Path:
    path = root / name / "SKILL.md"
    path.parent.mkdir(parents=True)
    _ = path.write_text(
        f"---\nname: {name}\ndescription: {name} skill\n{extra}---\nBody\n"
    )
    return path


# Cursor's CustomCommandsLoader skips `user-invocable: false` skills from its active list.
CURSOR_DISABLED = (("user-invocable", "false"),)


class NativeSetup:
    """Real files for one fixture workspace, an empty workspace and native home roots."""

    def __init__(self) -> None:
        self.directory: tempfile.TemporaryDirectory[str] = tempfile.TemporaryDirectory()
        root = Path(self.directory.name).resolve()
        self.workspace: Path = root / "workspace"
        self.empty: Path = root / "empty"
        self.home: Path = root / "home"
        self.empty.mkdir()
        skills = self.workspace / ".claude/skills"
        self.skill_file: Path = write_skill(skills, SKILL)
        self.second_file: Path = write_skill(skills, "router-native-second")
        self.user_skill: Path = write_skill(self.home / ".claude/skills", "user-native")
        # Cursor's own loader reads ~/.codex/skills; a provider that loads it may report it.
        self.shared_skill: Path = write_skill(
            self.home / ".codex/skills", "shared-native"
        )
        self.codex_only: Path = write_skill(self.home / ".codex/skills", "codex-only")
        self.fixture: FixtureRecord = FixtureRecord(
            workspace=self.workspace,
            empty_workspace=self.empty,
            visible_file=self.workspace / "README.verification.txt",
            skill_name=SKILL,
            skill_file=self.skill_file,
            canary=CANARY,
            second_skill_name="router-native-second",
            second_skill_file=self.second_file,
            second_canary=SECOND_CANARY,
        )

    def __enter__(self) -> t.Self:
        return self

    def __exit__(self, *_: object) -> None:
        self.directory.cleanup()

    def authority(
        self,
        *extra: str,
        shared_root: bool = True,
        disabled_markers: tuple[tuple[str, str], ...] = (),
    ) -> NativeSkillAuthority:
        home_roots = [self.home / ".claude/skills"]
        if shared_root:
            home_roots.append(self.home / ".codex/skills")
        return NativeSkillAuthority(
            advertised_names=frozenset(
                {SKILL, "router-native-second", "user-native", *extra}
            ),
            home_roots=tuple(home_roots),
            workspace_dirs=(".claude/skills",),
            disabled_markers=disabled_markers,
        )


def skill_row(
    name: str, path: Path, scope: str = "repo", enabled: bool = True
) -> JsonObject:
    return {
        "name": name,
        "description": "d",
        "path": str(path),
        "scope": scope,
        "enabled": enabled,
    }


def skills_reply(
    cwd: Path, skills: list[JsonValue], errors: list[JsonValue] | None = None
) -> RpcReply:
    entry: JsonObject = {"cwd": str(cwd), "skills": skills, "errors": errors or []}
    return RpcReply.model_validate({"id": 2, "result": {"data": [entry]}})


def error_reply(message: str = "unsupported") -> RpcReply:
    return RpcReply.model_validate(
        {"id": 2, "error": {"code": -32000, "message": message}}
    )


def observation(
    events: list[JsonObject], end: str = "deadlineReached"
) -> ProviderObservation:
    target: JsonObject = {
        "endpoint": {"serviceId": "service", "endpointId": "claude-local"},
        "sessionId": "thread",
    }
    sequenced: list[JsonValue] = [
        {"sequence": index, "event": event}
        for index, event in enumerate(events, start=1)
    ]
    return ProviderObservation.model_validate(
        {"target": target, "events": sequenced, "epoch": 0, "endReason": end}
    )


def item(item_id: str, kind: str, text: str) -> JsonObject:
    return {
        "kind": "itemStarted",
        "item": {"itemId": item_id, "kind": {"kind": kind}, "text": text},
    }


def catalogue(item_id: str, names: list[str]) -> JsonObject:
    commands = [{"name": name, "description": ""} for name in names]
    return item(item_id, "notice", json.dumps({"availableCommands": commands}))


def turn_started(turn_id: str = "turn-1") -> JsonObject:
    return {"kind": "turnStarted", "turnId": turn_id}


def operation(
    operation_id: str,
    kind: str = "conversationPrompt",
    admitted: int = 1,
    terminal: int | None = 2,
) -> OperationRow:
    return OperationRow(
        operation_id=operation_id,
        operation_kind=kind,
        stage="terminal",
        admitted_at_ms=admitted,
        terminal_at_ms=terminal,
    )
