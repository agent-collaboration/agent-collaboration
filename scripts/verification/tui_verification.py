"""Bind original compiled-PTY evidence to source/build and expose incomplete coverage.

Run with --help from the repository root. This module reads original PNG bytes;
it never renders, crops or reconstructs an image. The compact route is partial.
"""

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import subprocess
import typing as t
import urllib.parse
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from scripts.verification.process_ownership import (
    lookup_process_identity,
    same_identity,
)

QuotaFixtureScenario = t.Literal[
    "normal",
    "inspection-pending",
    "commit-pending",
    "unknown-result",
    "save-failed",
    "disabled-yes",
    "revalidation-pending",
    "refused-result",
    "refresh-pending",
    "save-pending",
]
QUOTA_COMPONENT_TEST = "presentation::quota::quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture"
CAPTURE_TEST = "compiled_compact_current_storyboard_exports_both_real_tuis"
QUOTA_CAPTURE_TEST = "compiled_compact_current_quota_storyboard_exports_real_tui"
SESSIONS_CAPTURE_TEST = "compiled_compact_current_sessions_storyboard_exports_real_tui"
SOURCE_PATHS = ("crates", "Cargo.toml", "Cargo.lock", ".cargo")
# Declared static terminal grids; phone-width grids are not device observations.
VIEWPORTS = ((120, 36), (76, 30), (48, 24), (48, 12), (40, 24), (32, 24))
SESSION_CHECKPOINT_STATES = (
    "all",
    "codex-details",
    "fork",
    "fork-choice",
    "new",
    "new-machine",
    "new-no-match",
    "new-returned",
    "machine-sources",
    "client-no-match",
    "chooser",
    "help",
    "claude",
    "cancelled",
    "details",
    "folder-choice",
    "model-provider-choice",
    "source-choice",
    "runtime-choice",
    "sort-choice",
)
SESSION_DETAILS_POSES: dict[tuple[int, int], tuple[tuple[str, int], ...]] = {
    (32, 24): (("details-title", 0), ("details", 4), ("details-path", 6)),
    (40, 24): (("details-title", 0), ("details", 3), ("details-path", 5)),
    (48, 12): (
        ("details-title", 1),
        ("details-identity", 5),
        ("details-state", 7),
        ("details-path", 8),
    ),
    (48, 24): (("details-title", 0), ("details", 3)),
    (76, 30): (("details", 2),),
    (120, 36): (("details", 0),),
}
SESSION_DETAILS_FIELDS = (
    "details context claude gamma [000.0011]",
    "details title claude gamma session · project-a",
    "details path /repo/project-a",
    "details session id 00000000-0000-4000-8000-000000000011",
    "details endpoint claude gamma",
    "details state idle",
)


def session_checkpoint_states(columns: int, rows: int) -> tuple[str, ...]:
    poses = SESSION_DETAILS_POSES[(columns, rows)]
    return tuple(
        expanded
        for state in SESSION_CHECKPOINT_STATES
        for expanded in ([name for name, _ in poses] if state == "details" else [state])
    )


def session_details_required_fields(checkpoint: str) -> set[str]:
    suffix = checkpoint.removeprefix("compact-sessions-")
    indices = {
        "details-title": (0, 1),
        "details-path": (0, 2),
        "details-identity": (0, 3),
        "details-state": (0, 4, 5),
        "details": (0, 3, 4, 5),
    }.get(suffix, ())
    return {SESSION_DETAILS_FIELDS[index] for index in indices}


QUOTA_CHECKPOINT_STATES = (
    "all",
    "provider-no-match",
    "chooser",
    "help",
    "disabled-choice",
    "hidden-choice",
    "claude",
    "cancelled",
    "details",
)
LIMITS = (
    "Synthetic records and substituted external effects; no live discovery, provider launch, "
    "personal balances, account reset/save or Ghostty observation. OCR checks scene markers; "
    "visual inspection and offered-product-test sensitivity remain required."
)


class EvidenceModel(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)


class CommandArguments(argparse.Namespace):
    source: Path | None = None
    gaps: bool = False
    cua_plan: Path | None = None
    surface: t.Literal["sessions", "quota"] | None = None
    columns: int = 0
    rows: int = 0
    baseline: Path | None = None
    record: Path | None = None


def file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class ArtifactBinding(EvidenceModel):
    path: Path
    sha256: str = Field(pattern=r"^[a-f0-9]{64}$")

    @classmethod
    def capture(cls, path: Path) -> t.Self:
        return cls(path=path.resolve(strict=True), sha256=file_digest(path))


def verify_artifact(binding: ArtifactBinding) -> None:
    if binding.path.resolve(strict=True) != binding.path:
        raise ValueError(f"artifact location changed: {binding.path}")
    if file_digest(binding.path) != binding.sha256:
        raise ValueError(f"artifact changed: {binding.path}")


class SourceBinding(EvidenceModel):
    head: str = Field(pattern=r"^[a-f0-9]{40}$")
    files: dict[str, str] = Field(min_length=1)

    def digest(self) -> str:
        return hashlib.sha256(
            json.dumps(self.model_dump(), sort_keys=True).encode()
        ).hexdigest()

    def verify_files(self, repository: Path) -> None:
        for relative, expected in self.files.items():
            source = repository / relative
            if (
                Path(relative).is_absolute()
                or ".." in Path(relative).parts
                or not source.is_file()
                or not source.resolve().is_relative_to(repository.resolve())
                or file_digest(source) != expected
            ):
                raise ValueError(f"source changed or unsafe: {relative}")


def git_output(repository: Path, arguments: list[str]) -> bytes:
    return subprocess.check_output(["git", "-C", str(repository), *arguments])


def source_binding(repository: Path) -> SourceBinding:
    names = (
        git_output(
            repository,
            [
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
                "--",
                *SOURCE_PATHS,
            ],
        )
        .decode()
        .split("\0")
    )
    files = {
        name: file_digest(repository / name)
        for name in sorted(set(names) - {""})
        if (repository / name).is_file()
    }
    return SourceBinding(
        head=git_output(repository, ["rev-parse", "HEAD"]).decode().strip(), files=files
    )


class ChoiceFocusMeasurement(EvidenceModel):
    label: t.Literal["shown", "hidden"]
    labelYellowPixels: int = Field(strict=True, ge=0)
    glyphYellowPixels: int = Field(strict=True, ge=0)
    glyphColumns: int = Field(strict=True, ge=0)
    glyphRows: int = Field(strict=True, ge=0)
    labelHeight: int = Field(strict=True, gt=0)

    def proves_focus(self) -> bool:
        return (
            self.labelYellowPixels >= 10
            and self.glyphYellowPixels >= 4
            and self.glyphColumns >= 2
            and self.labelHeight >= 4
            and self.glyphRows >= max(3, self.labelHeight // 3)
        )


class ImageObservation(EvidenceModel):
    checkpoint: str
    recognizedLines: list[str] = Field(min_length=1)
    recognizedFieldPairs: list[str]
    choiceFocusMeasurements: list[ChoiceFocusMeasurement]
    missingMarkers: list[str] = Field(max_length=0)
    unexpectedMarkers: list[str] = Field(max_length=0)

    def require_selected_choice(self, expected: t.Literal["shown", "hidden"]) -> None:
        text = " ".join(self.recognizedLines).lower()
        other = "shown" if expected == "hidden" else "hidden"
        if re.search(r"[›❯>]\s*" + other + r"\b", text) is None:
            if re.search(r"[›❯>]\s*" + expected + r"\b", text) is not None:
                return
            focused = [
                item for item in self.choiceFocusMeasurements if item.proves_focus()
            ]
            if (
                len(focused) == 1
                and focused[0].label == expected
                and sum(item.label == expected for item in self.choiceFocusMeasurements)
                == 1
            ):
                return
        raise ValueError("selected choice lacks original glyph/color evidence")


class ImageRejection(EvidenceModel):
    checkpoint: str
    recognizedLines: list[str]
    recognizedFieldPairs: list[str]
    choiceFocusMeasurements: list[ChoiceFocusMeasurement]
    missingMarkers: list[str]
    unexpectedMarkers: list[str]

    @model_validator(mode="after")
    def rejects_a_scene(self) -> t.Self:
        if not self.missingMarkers and not self.unexpectedMarkers:
            raise ValueError("negative control accepted the wrong scene")
        return self


CuaSceneName = t.Annotated[
    str,
    Field(min_length=1, max_length=96, pattern=r"^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$"),
]


class CuaArtifactExport(EvidenceModel):
    base_url: str
    directory: Path
    scenes: list[CuaSceneName] = Field(min_length=1, max_length=128)
    maximum_bytes: t.Literal[8388608]
    mime_type: t.Literal["image/jpeg"]
    source_sha256: str = Field(pattern=r"^[a-f0-9]{64}$")

    @field_validator("base_url")
    @classmethod
    def private_export_url(cls, value: str) -> str:
        parsed = urllib.parse.urlsplit(value)
        parts = parsed.path.split("/")
        if (
            parsed.scheme != "http"
            or parsed.hostname != "127.0.0.1"
            or parsed.port is None
            or parsed.username is not None
            or parsed.password is not None
            or parsed.query
            or parsed.fragment
            or len(parts) != 4
            or parts[:2] != ["", "export"]
            or parts[-1] != ""
            or len(parts[2]) != 64
            or any(c not in "0123456789abcdef" for c in parts[2])
        ):
            raise ValueError("export form must be the exact private loopback nonce URL")
        return value

    @field_validator("scenes")
    @classmethod
    def unique_scenes(cls, value: list[str]) -> list[str]:
        if len(set(value)) != len(value):
            raise ValueError("duplicate declared original-artifact scene")
        return value


class CuaReady(EvidenceModel):
    kind: t.Literal["listener-ready-not-ui"]
    surface: t.Literal["sessions", "quota"]
    url: str
    ttyd_pid: int = Field(strict=True, gt=0)
    binary_sha256: str
    ttyd_sha256: str
    child_pid_file: Path
    viewport_file: Path
    stop_file: Path
    lifetime_seconds: t.Literal[600]
    artifact_export: CuaArtifactExport
    quota_fixture_scenario: QuotaFixtureScenario | None = None

    @field_validator("url")
    @classmethod
    def owned_loopback_url(cls, value: str) -> str:
        parsed = urllib.parse.urlsplit(value)
        if (
            parsed.scheme != "http"
            or parsed.hostname != "127.0.0.1"
            or parsed.port is None
            or parsed.username is not None
            or parsed.password is not None
            or parsed.path != "/"
            or parsed.query
            or parsed.fragment
        ):
            raise ValueError("CUA fixture URL must be an exact loopback listener")
        return value


class CuaLifecycleReceipt(EvidenceModel):
    kind: t.Literal["live-cua-fixture-lifecycle-only"]
    surface: t.Literal["sessions", "quota"]
    binary_sha256: str
    child_exit: t.Literal[0]
    provider_requests: t.Literal[0]
    state_and_secrets_unchanged: t.Literal[True]
    owned_processes_stopped: t.Literal[True]
    artifact_sink_stopped: t.Literal[True]
    artifact_count: int = Field(strict=True, ge=0)
    last_viewport: str
    ui_observation: t.Literal["separate-original-CUA-evidence-required"]
    quota_fixture_scenario: QuotaFixtureScenario | None = None


class CuaSavedArtifact(EvidenceModel):
    kind: t.Literal["original-cua-jpeg-bytes-only"]
    scene: CuaSceneName
    filename: str
    bytes: int = Field(strict=True, gt=0, le=8388608)
    sha256: str = Field(pattern=r"^[a-f0-9]{64}$")
    encoding_round_trip: t.Literal[True]


class TerminalViewport(EvidenceModel):
    columns: int = Field(strict=True, gt=0)
    rows: int = Field(strict=True, gt=0)


class CuaProcessIdentity(EvidenceModel):
    pid: int
    uid: int
    group: int
    birth: str
    executable: str


class CuaPtyReadback(EvidenceModel):
    kind: t.Literal["pty-dimension-readback"] = "pty-dimension-readback"
    surface: t.Literal["sessions", "quota"]
    identity: CuaProcessIdentity
    viewport: TerminalViewport
    source_sha256: str
    binary_sha256: str
    observed_at: dt.datetime

    def require_same_pty(self, before: t.Self) -> None:
        if (
            self.surface != before.surface
            or self.source_sha256 != before.source_sha256
            or self.binary_sha256 != before.binary_sha256
            or not same_identity(
                self.identity.model_dump(), before.identity.model_dump()
            )
        ):
            raise ValueError("CUA resize replaced the declared PTY or target")


class ReceiptImage(EvidenceModel):
    checkpoint: str = Field(pattern=r"^compact-(sessions|quota)-[a-z]+(?:-[a-z]+)*$")
    png_sha256: str = Field(pattern=r"^[a-f0-9]{64}$")


class FilterReceipt(EvidenceModel):
    surface: t.Literal["sessions", "quota"]
    columns: int
    rows: int
    binary_sha256: str
    vhs_sha256: str
    ttyd_sha256: str
    verifier_sha256: str
    verifier_source_sha256: str
    verifier_checkpoint_source_sha256: str = Field(pattern=r"^[a-f0-9]{64}$")
    driver_source_sha256: str
    tape_driver_source_sha256: str
    fixed_child_sha256: str
    tape_sha256: str
    state_and_secrets_unchanged: t.Literal[True]
    provider_requests: t.Literal[0]
    child_exit: t.Literal[0]
    owned_processes_stopped: t.Literal[True]
    fixture: str
    images: list[ReceiptImage] = Field(min_length=6)

    @field_validator("images")
    @classmethod
    def unique_checkpoints(cls, images: list[ReceiptImage]) -> list[ReceiptImage]:
        if len({entry.checkpoint for entry in images}) != len(images):
            raise ValueError("duplicate checkpoint in capture receipt")
        return images


class CapturePlan(EvidenceModel):
    repository: Path
    output: Path
    temporary_directory: Path
    source: SourceBinding
    capture_driver: ArtifactBinding
    harness_binary: ArtifactBinding
    vhs: ArtifactBinding
    ttyd: ArtifactBinding
    image_verifier: ArtifactBinding
    dynamic_library_path: str = Field(min_length=1)
    build_declaration: str = Field(min_length=1)
    cua_artifact_scenes: list[CuaSceneName] = Field(
        default_factory=list, max_length=128
    )

    @field_validator("cua_artifact_scenes")
    @classmethod
    def unique_artifact_scenes(cls, value: list[str]) -> list[str]:
        if len(set(value)) != len(value):
            raise ValueError("duplicate declared original-artifact scene")
        return value

    @field_validator("repository", "output", "temporary_directory")
    @classmethod
    def absolute_location(cls, value: Path) -> Path:
        if not value.is_absolute():
            raise ValueError("capture locations must be absolute")
        return value

    def verify_target(self) -> None:
        if source_binding(self.repository) != self.source:
            raise ValueError("declared product source changed; stop and re-pin")
        for artifact in (
            self.capture_driver,
            self.harness_binary,
            self.vhs,
            self.ttyd,
            self.image_verifier,
        ):
            verify_artifact(artifact)
        if not self.temporary_directory.is_dir():
            raise ValueError("declared existing private temporary directory required")


class OriginalImage(EvidenceModel):
    path: Path
    sha256: str
    screen: str
    dialog: str
    columns: int
    rows: int
    journey: str
    source_head: str
    binary_sha256: str
    capture_driver_sha256: str
    source_sha256: str
    result: t.Literal["scene-oracle-passed"] = "scene-oracle-passed"
    visual_inspection: t.Literal["pending"] = "pending"
    limits: str = LIMITS


def read_cua_state(
    plan: CapturePlan,
    surface: t.Literal["sessions", "quota"],
    expected: TerminalViewport,
) -> CuaPtyReadback:
    """Read this fixture's actual same-process identity and kernel viewport."""
    plan.verify_target()
    folder = plan.output / f"cua-{surface}"
    ready = CuaReady.model_validate_json((folder / "cua-ready.json").read_bytes())
    if (
        ready.surface != surface
        or ready.binary_sha256 != plan.harness_binary.sha256
        or ready.ttyd_sha256 != plan.ttyd.sha256
        or ready.child_pid_file.resolve() != (folder / "quota-child.pid").resolve()
        or ready.viewport_file.resolve() != (folder / "quota-child.viewport").resolve()
        or ready.stop_file.resolve() != (folder / "cua-stop").resolve()
        or ready.artifact_export.directory.resolve()
        != (folder / "original-images").resolve()
        or set(ready.artifact_export.scenes) != set(plan.cua_artifact_scenes)
        or ready.artifact_export.source_sha256
        != file_digest(
            plan.repository
            / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/cua_artifact_sink.rs"
        )
    ):
        raise ValueError("CUA ready record does not match this declared fixture")
    ttyd = CuaProcessIdentity.model_validate(lookup_process_identity(ready.ttyd_pid))
    child_pid = int(ready.child_pid_file.read_text().strip())
    child = CuaProcessIdentity.model_validate(lookup_process_identity(child_pid))
    if (
        child.uid != os.getuid()
        or ttyd.uid != os.getuid()
        or Path(child.executable).resolve() != plan.harness_binary.path
        or Path(ttyd.executable).resolve() != plan.ttyd.path
    ):
        raise ValueError(
            "CUA process identity does not match declared executables/owner"
        )
    values = ready.viewport_file.read_text().split()
    if len(values) != 2:
        raise ValueError("CUA viewport readback incomplete")
    viewport = TerminalViewport(columns=int(values[0]), rows=int(values[1]))
    if viewport != expected:
        raise ValueError(
            f"actual PTY viewport {viewport} differs from expected {expected}"
        )
    return CuaPtyReadback(
        surface=surface,
        identity=child,
        viewport=viewport,
        source_sha256=plan.source.digest(),
        binary_sha256=plan.harness_binary.sha256,
        observed_at=dt.datetime.now(dt.UTC),
    )


def coverage_gaps() -> list[str]:
    return [
        "sessions/new",
        "sessions/machine-choice",
        "sessions/fork",
        "sessions/Details-Left-Right-Tab-focus-Enter-exact-effect-public-entry",
        "sessions/source-loading-and-failure",
        "sessions/choice-no-match-unavailable",
        "sessions/full-ID-paste-and-all-filter-fields",
        "agent-sessions/real-compiled-public-entry-Codex-ClaudeCode-Cursor",
        "agent-sessions/client-actions-hosted-Claude-interactive-Claude-hosted-Cursor",
        "agent-sessions/provider-runtime-proof-grant-and-adopted-action-contract",
        "R16/actual-callable-Human-Load-and-Session-requester-preservation",
        "R16/source-Stored-summary-saved-policy-directory-other-Approver",
        "R16/inventory-Stored-retention-explicit-partial-and-read-failure-classification",
        "R16/real-local-and-remote-managed-Cursor-create-load-resume",
        "R16/typed-origin-and-native-job-handle-versus-session-UUID",
        "R16/managed-one-shot-unknown-with-Router-operation-readback",
        "R16/native-lost-receipt-unknown-no-fake-Router-ID-or-retry",
        "R16/catalogue-real-bounded-native-job-reader-and-mixed-vendor-shapes",
        "R16/catalogue-new-Host-service-API-native-Stored-Loaded-Active-and-pagination",
        "R16/catalogue-independent-peer-job-coverage-through-rendered-notice",
        "R16/catalogue-captured-native-UUID-directory-source-and-terminal-job-resume",
        "R16/catalogue-before-first-prompt-public-TUI-local-and-Sunbook",
        "R15/public-agent-sessions-causal-background-process-proof",
        "R15/actual-native-print-background-guard-before-session-dispatch",
        "R15/native-Claude-background-registration-and-owned-cleanup",
        "R15/TUI-launch-owner-reuse-awaits-admitted-U16-implementation-and-target",
        "quota/options-resets",
        "quota/options-credits",
        "quota/credit-policy",
        "quota/credit-refresh",
        "quota/weekly-floor",
        "quota/reset-inventory",
        "quota/reset-review",
        "quota/reset-confirmation",
        "quota/reset-result",
        "quota/paging",
        "48x12 usable header/actions and scrolled body (static capture wired, unobserved)",
        "40x24 and 32x24 physical cell bounds and original visibility (wired, unobserved)",
        "actual iPhone terminal operation (separate device target, unobserved)",
        "C7 named header and current Help census associations",
        "C7 active field/query/no-match and unavailable Select",
        "C7 captured decision identity/risk and single No/Yes choice group",
        "C7 short-body action adjacency and long-body final content",
        "resize continuity",
        "long-name-CJK-and-status-variants",
        "offered-product-test sensitivity",
        "visual inspection",
        "U14 Lead document checks",
    ]


def checkpoint_dialog(state: str) -> str:
    if state.startswith("new"):
        return "new"
    if state.startswith("fork"):
        return "fork"
    if state == "machine-sources":
        return "machines"
    if state == "chooser" or state.endswith(("choice", "no-match")):
        return "filters"
    if state.startswith("details") or state.endswith("-details"):
        return "details"
    return "contextual-help" if state == "help" else "browse"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument(
        "--source", type=Path, help="Read-only source declaration for a released build"
    )
    _ = parser.add_argument(
        "--gaps",
        action="store_true",
        help="Print compact-route coverage gaps; launch nothing",
    )
    _ = parser.add_argument(
        "--cua-plan", type=Path, help="Read the declared live CUA fixture; no UI action"
    )
    _ = parser.add_argument("--surface", choices=("sessions", "quota"))
    _ = parser.add_argument("--columns", type=int, default=0)
    _ = parser.add_argument("--rows", type=int, default=0)
    _ = parser.add_argument(
        "--baseline",
        type=Path,
        help="Require same PTY identity as this earlier readback",
    )
    _ = parser.add_argument(
        "--record", type=Path, help="Fresh readback file inside declared output"
    )
    arguments = parser.parse_args(namespace=CommandArguments())
    if arguments.cua_plan is not None:
        if arguments.surface is None or arguments.record is None:
            parser.error("CUA readback needs --surface, --columns, --rows and --record")
        plan = CapturePlan.model_validate_json(arguments.cua_plan.read_bytes())
        if not arguments.record.resolve().is_relative_to(plan.output.resolve()):
            parser.error("CUA readback record must be inside declared output")
        state = read_cua_state(
            plan,
            arguments.surface,
            TerminalViewport(columns=arguments.columns, rows=arguments.rows),
        )
        if arguments.baseline is not None:
            state.require_same_pty(
                CuaPtyReadback.model_validate_json(arguments.baseline.read_bytes())
            )
        with arguments.record.open("x") as record:
            _ = record.write(state.model_dump_json(indent=2))
        print(state.model_dump_json(indent=2))
    elif arguments.source is not None:
        print(source_binding(arguments.source).model_dump_json(indent=2))
    elif arguments.gaps:
        print(
            json.dumps(
                {"status": "draft-unobserved", "gaps": coverage_gaps()}, indent=2
            )
        )
    else:
        parser.error("choose --source or --gaps")


if __name__ == "__main__":
    main()
