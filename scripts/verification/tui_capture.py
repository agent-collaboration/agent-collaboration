"""Run the existing compiled compact PTY journey on a released, declared target.

Usage: python -m scripts.verification.tui_capture PLAN.json [--quota-only | --sessions-only] [--check]
No cargo build, install or pixel reconstruction occurs. Supply CapturePlan JSON
from tui_verification.py, including producer build declaration and binary hashes.
The route is partial: all-menu, constrained usability and resize proof remain gaps.
"""

import argparse
import datetime as dt
import json
import os
import subprocess
import typing as t
from enum import StrEnum
from pathlib import Path

from scripts.verification.tui_capture_lifecycle import (
    StaticCaptureBudget,
    discover_capture_test,
    static_capture_budget_seconds,
    wait_static_capture,
)
from scripts.verification.tui_verification import (
    CAPTURE_TEST,
    QUOTA_CAPTURE_TEST,
    QUOTA_CHECKPOINT_STATES,
    QUOTA_COMPONENT_TEST,
    SESSION_CHECKPOINT_STATES,
    SESSIONS_CAPTURE_TEST,
    VIEWPORTS,
    CapturePlan,
    CuaLifecycleReceipt,
    CuaSavedArtifact,
    FilterReceipt,
    ImageObservation,
    ImageRejection,
    OriginalImage,
    QuotaFixtureScenario,
    checkpoint_dialog,
    coverage_gaps,
    file_digest,
)


class CommandArguments(argparse.Namespace):
    plan: Path = Path()
    check: bool = False
    cua_surface: t.Literal["sessions", "quota"] | None = None
    cua_quota_fixture: QuotaFixtureScenario | None = None
    quota_only: bool = False
    sessions_only: bool = False


class CaptureSelection(StrEnum):
    STATIC_BOTH = "static-both"
    STATIC_QUOTA = "static-quota"
    STATIC_SESSIONS = "static-sessions"
    CUA_SESSIONS = "cua-sessions"
    CUA_QUOTA = "cua-quota"

    @property
    def test_name(self) -> str:
        if self is self.STATIC_QUOTA:
            return QUOTA_CAPTURE_TEST
        if self is self.STATIC_SESSIONS:
            return SESSIONS_CAPTURE_TEST
        return CAPTURE_TEST

    @property
    def static_surfaces(self) -> tuple[t.Literal["sessions", "quota"], ...]:
        if self.cua_surface is not None:
            raise ValueError("CUA selection has no static image collection")
        if self is self.STATIC_QUOTA:
            return ("quota",)
        if self is self.STATIC_SESSIONS:
            return ("sessions",)
        return ("sessions", "quota")

    @property
    def static_viewports(self) -> tuple[tuple[int, int], ...]:
        _ = self.static_surfaces
        return tuple(reversed(VIEWPORTS)) if self is self.STATIC_SESSIONS else VIEWPORTS

    @property
    def blank_scene(self) -> tuple[str, str]:
        if self in (self.STATIC_QUOTA, self.CUA_QUOTA):
            return "compact-quota-all", "Quota status"
        return "compact-sessions-all", "Sessions"

    @property
    def static_budget_seconds(self) -> int:
        return static_capture_budget_seconds(self.static_surfaces)

    @property
    def cua_surface(self) -> t.Literal["sessions", "quota"] | None:
        match self:
            case CaptureSelection.CUA_SESSIONS:
                return "sessions"
            case CaptureSelection.CUA_QUOTA:
                return "quota"
            case (
                CaptureSelection.STATIC_BOTH
                | CaptureSelection.STATIC_QUOTA
                | CaptureSelection.STATIC_SESSIONS
            ):
                return None


def capture_arguments(
    driver: Path, selection: CaptureSelection = CaptureSelection.STATIC_BOTH
) -> list[str]:
    return [
        str(driver),
        selection.test_name,
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]


def require_one_test(discovery: str, test_name: str = CAPTURE_TEST) -> None:
    selected = [line for line in discovery.splitlines() if line.endswith(": test")]
    if selected != [f"{test_name}: test"]:
        raise ValueError("exact capture test missing or broadened")


def check_driver(
    plan: CapturePlan,
    selection: CaptureSelection = CaptureSelection.STATIC_BOTH,
    quota_fixture: QuotaFixtureScenario | None = None,
) -> None:
    if quota_fixture is not None and selection is not CaptureSelection.CUA_QUOTA:
        raise ValueError("component fixture requires the quota CUA lease")
    plan.verify_target()
    discovery = discover_capture_test(
        [str(plan.capture_driver.path), selection.test_name, "--exact", "--list"],
        plan.repository,
    )
    require_one_test(discovery, selection.test_name)
    if quota_fixture is not None:
        fixture_discovery = discover_capture_test(
            [
                str(plan.harness_binary.path),
                QUOTA_COMPONENT_TEST,
                "--exact",
                "--ignored",
                "--list",
            ],
            plan.repository,
        )
        require_one_test(fixture_discovery, QUOTA_COMPONENT_TEST)
    qualify_image_oracle(plan, selection)


def validate_image_qualification(
    exit_code: int,
    output: str,
    selection: CaptureSelection = CaptureSelection.STATIC_BOTH,
) -> None:
    if exit_code != 1:
        raise ValueError(
            "image oracle unavailable or blank accepted; use an authorized working execution context"
        )
    rejection = ImageRejection.model_validate_json(output)
    checkpoint, marker = selection.blank_scene
    if rejection.checkpoint != checkpoint or marker not in rejection.missingMarkers:
        raise ValueError("blank image did not fail its exact declared scene")


def qualify_image_oracle(plan: CapturePlan, selection: CaptureSelection) -> None:
    blank = (
        plan.repository
        / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/fixtures/blank-startup.png"
    )
    result = subprocess.run(
        [str(plan.image_verifier.path), selection.blank_scene[0], str(blank)],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    try:
        validate_image_qualification(result.returncode, result.stdout, selection)
    except ValueError as error:
        raise ValueError(f"{error}; oracle stderr: {result.stderr.strip()}") from error


def run_capture(
    plan: CapturePlan,
    selection: CaptureSelection = CaptureSelection.STATIC_BOTH,
    quota_fixture: QuotaFixtureScenario | None = None,
) -> None:
    cua_surface = selection.cua_surface
    if quota_fixture is not None and cua_surface != "quota":
        raise ValueError("component fixture requires the quota CUA lease")
    if cua_surface is not None and not plan.cua_artifact_scenes:
        raise ValueError(
            "CUA lease requires exact predeclared original-artifact scenes"
        )
    check_driver(plan, selection, quota_fixture)
    # Exclusive creation prevents old receipts or pixels from satisfying this run.
    plan.output.mkdir(mode=0o700, parents=False, exist_ok=False)
    _ = (plan.output / "declared-target.json").write_text(
        plan.model_dump_json(indent=2)
    )
    _ = (plan.output / "cua-request.json").write_text(
        json.dumps({"surface": cua_surface, "quota_fixture_scenario": quota_fixture})
    )
    environment = dict(os.environ)
    _ = environment.pop("QUOTA_STORYBOARD_CUA_SURFACE", None)
    _ = environment.pop("QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY", None)
    _ = environment.pop("QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO", None)
    _ = environment.pop("QUOTA_STORYBOARD_CUA_ARTIFACT_SCENES", None)
    if cua_surface is not None:
        environment["QUOTA_STORYBOARD_CUA_SURFACE"] = cua_surface
        environment["QUOTA_STORYBOARD_CUA_ARTIFACT_SCENES"] = ",".join(
            plan.cua_artifact_scenes
        )
    if quota_fixture is not None:
        environment["QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY"] = str(
            plan.harness_binary.path
        )
        environment["QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO"] = quota_fixture
    environment.update(
        {
            "QUOTA_STORYBOARD_OUTPUT": str(plan.output),
            "QUOTA_STORYBOARD_VHS": str(plan.vhs.path),
            "QUOTA_STORYBOARD_TTYD": str(plan.ttyd.path),
            "QUOTA_STORYBOARD_IMAGE_VERIFIER": str(plan.image_verifier.path),
            "QUOTA_STORYBOARD_DYLD_LIBRARY_PATH": plan.dynamic_library_path,
            "TMPDIR": str(plan.temporary_directory),
        }
    )
    command = capture_arguments(plan.capture_driver.path, selection)
    _ = (plan.output / "command.json").write_text(json.dumps(command))
    with (
        (plan.output / "driver.stdout").open("w") as stdout,
        (plan.output / "driver.stderr").open("w") as stderr,
    ):
        if cua_surface is None:
            child = subprocess.Popen(
                command,
                cwd=plan.repository,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
            )
            exit_code = wait_static_capture(
                child,
                plan.output,
                StaticCaptureBudget(
                    work_seconds=selection.static_budget_seconds - 90,
                    cleanup_seconds=90,
                ),
            )
        else:
            result = subprocess.run(
                command,
                cwd=plan.repository,
                env=environment,
                stdout=stdout,
                stderr=stderr,
                check=False,
            )
            exit_code = result.returncode
            _ = (plan.output / "driver-exit.json").write_text(
                json.dumps({"exit": exit_code})
            )
    plan.verify_target()
    transcript = (plan.output / "driver.stdout").read_text()
    if (
        exit_code != 0
        or "test result: ok. 1 passed; 0 failed; 0 ignored;" not in transcript
    ):
        raise ValueError(
            "capture did not run exactly one passing non-ignored test; retain logs"
        )
    if cua_surface is not None:
        receipt = CuaLifecycleReceipt.model_validate_json(
            (plan.output / f"cua-{cua_surface}" / "cua-receipt.json").read_bytes()
        )
        if (
            receipt.surface != cua_surface
            or receipt.binary_sha256 != plan.harness_binary.sha256
            or receipt.quota_fixture_scenario != quota_fixture
        ):
            raise ValueError("CUA lifecycle receipt target mismatch")
        _ = (plan.output / "cua-lifecycle-summary.json").write_text(
            json.dumps(
                {
                    "target": plan.model_dump(mode="json"),
                    "receipt": receipt.model_dump(mode="json"),
                    "original_artifacts": collect_cua_artifacts(
                        plan, cua_surface, receipt.artifact_count
                    ),
                    "complete": False,
                    "limits": "Lifecycle only; same-PTY readbacks and original CUA screenshots must be checked separately.",
                },
                indent=2,
            )
        )
        return
    images = collect_images(plan, selection)
    manifest = {
        "kind": "original-compiled-fixture-pty",
        "created_at": dt.datetime.now(dt.UTC).isoformat(),
        "target": plan.model_dump(mode="json"),
        "capture_selection": selection.value,
        "capture_test": selection.test_name,
        "captured_surfaces": selection.static_surfaces,
        "images": [image.model_dump(mode="json") for image in images],
        "gaps": coverage_gaps(),
        "complete": False,
    }
    _ = (plan.output / "gallery-manifest.json").write_text(
        json.dumps(manifest, indent=2)
    )


def collect_cua_artifacts(
    plan: CapturePlan, surface: str, expected_count: int
) -> list[dict[str, str | int | bool]]:
    """Retain byte identity only; viewport/action/UI judgment stays separate."""
    folder = plan.output / f"cua-{surface}" / "original-images"
    artifacts: list[dict[str, str | int | bool]] = []
    for scene in plan.cua_artifact_scenes:
        record = folder / f"{scene}.receipt.json"
        if not record.exists():
            continue
        receipt = CuaSavedArtifact.model_validate_json(record.read_bytes())
        if receipt.scene != scene or receipt.filename != f"{scene}.jpg":
            raise ValueError("original-artifact receipt scene/filename mismatch")
        original = folder / receipt.filename
        content = original.read_bytes()
        require_digest(original, receipt.sha256)
        if (
            len(content) != receipt.bytes
            or not content.startswith(b"\xff\xd8")
            or not content.endswith(b"\xff\xd9")
        ):
            raise ValueError("original-artifact bytes or JPEG format mismatch")
        artifacts.append(
            {
                "path": str(original.resolve()),
                "scene": scene,
                "bytes": receipt.bytes,
                "sha256": receipt.sha256,
                "encoding_round_trip": True,
                "mime_type": "image/jpeg",
                "result": "original-bytes-retained-ui-judgment-separate",
            }
        )
    if (
        expected_count != len(plan.cua_artifact_scenes)
        or len(artifacts) != expected_count
        or {path.name for path in folder.glob("*.jpg")}
        != {f"{scene}.jpg" for scene in plan.cua_artifact_scenes}
        or {path.name for path in folder.glob("*.receipt.json")}
        != {f"{scene}.receipt.json" for scene in plan.cua_artifact_scenes}
    ):
        raise ValueError("original-artifact count or declared file set mismatch")
    return artifacts


def require_digest(path: Path, expected: str) -> None:
    if file_digest(path) != expected:
        raise ValueError(f"evidence digest mismatch: {path}")


def collect_images(
    plan: CapturePlan, selection: CaptureSelection = CaptureSelection.STATIC_BOTH
) -> list[OriginalImage]:
    """Read every expected same-run receipt set; missing sets fail closed."""
    images: list[OriginalImage] = []
    support = (
        plan.repository
        / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test"
    )
    expected_receipts = {
        plan.output / f"{surface}-compact-{columns}x{rows}" / "filter-receipt.json"
        for surface in selection.static_surfaces
        for columns, rows in selection.static_viewports
    }
    if set(plan.output.glob("**/filter-receipt.json")) != expected_receipts:
        raise ValueError("capture receipt set mismatch: missing or extra surface/grid")
    for surface in selection.static_surfaces:
        for columns, rows in selection.static_viewports:
            folder = plan.output / f"{surface}-compact-{columns}x{rows}"
            receipt = FilterReceipt.model_validate_json(
                (folder / "filter-receipt.json").read_bytes()
            )
            if (receipt.surface, receipt.columns, receipt.rows) != (
                surface,
                columns,
                rows,
            ):
                raise ValueError("receipt surface/viewport mismatch")
            for actual, expected in (
                (receipt.binary_sha256, plan.harness_binary.sha256),
                (receipt.vhs_sha256, plan.vhs.sha256),
                (receipt.ttyd_sha256, plan.ttyd.sha256),
                (receipt.verifier_sha256, plan.image_verifier.sha256),
            ):
                if actual != expected:
                    raise ValueError("receipt build/tool identity mismatch")
            for filename, expected in (
                ("provider_filter_capture.rs", receipt.driver_source_sha256),
                ("provider_filter_tape.rs", receipt.tape_driver_source_sha256),
                (
                    "verify_screen_checkpoint.swift",
                    receipt.verifier_checkpoint_source_sha256,
                ),
                ("verify_screen_image.swift", receipt.verifier_source_sha256),
            ):
                require_digest(support / filename, expected)
            require_digest(folder / "quota.tape", receipt.tape_sha256)
            require_digest(folder / "fixed_quota_child.py", receipt.fixed_child_sha256)
            states = (
                SESSION_CHECKPOINT_STATES
                if surface == "sessions"
                else QUOTA_CHECKPOINT_STATES
            )
            expected_order = [f"compact-{surface}-{state}" for state in states]
            expected_names = set(expected_order)
            if [entry.checkpoint for entry in receipt.images] != expected_order:
                raise ValueError(
                    "checkpoint sequence mismatch: missing, wrong or reordered scene"
                )
            if {path.name for path in folder.glob("*.png")} != {
                f"{checkpoint}.png" for checkpoint in expected_names
            }:
                raise ValueError("original PNG set mismatch: missing or extra scene")
            for expected, wrong in (
                (f"compact-{surface}-claude", "blank-startup"),
                (f"compact-{surface}-claude", f"compact-{surface}-all"),
                (f"compact-{surface}-cancelled", f"compact-{surface}-chooser"),
            ) + (
                (("compact-quota-hidden-choice", "compact-quota-disabled-choice"),)
                if surface == "quota"
                else ()
            ):
                rejection = ImageRejection.model_validate_json(
                    (
                        folder / f"reject-{expected}-{wrong}.image-observation.json"
                    ).read_bytes()
                )
                if rejection.checkpoint != expected:
                    raise ValueError("wrong negative-control checkpoint")
            for entry in receipt.images:
                png = folder / f"{entry.checkpoint}.png"
                require_digest(png, entry.png_sha256)
                observation = ImageObservation.model_validate_json(
                    (folder / f"{entry.checkpoint}.image-observation.json").read_bytes()
                )
                if observation.checkpoint != entry.checkpoint:
                    raise ValueError("wrong image observation checkpoint")
                if entry.checkpoint == "compact-quota-disabled-choice":
                    observation.require_selected_choice("shown")
                elif entry.checkpoint == "compact-quota-hidden-choice":
                    observation.require_selected_choice("hidden")
                if (
                    entry.checkpoint == "compact-quota-chooser"
                    and observation.recognizedFieldPairs
                    != ["account provider openai", "disabled accounts shown"]
                ):
                    raise ValueError(
                        "chooser lacks exact associated uncommitted field values"
                    )
                if (
                    entry.checkpoint == "compact-sessions-chooser"
                    and observation.recognizedFieldPairs != ["client codex"]
                ):
                    raise ValueError(
                        "chooser lacks exact associated uncommitted client"
                    )
                content = png.read_bytes()
                if len(content) <= 1000 or not content.startswith(b"\x89PNG\r\n\x1a\n"):
                    raise ValueError("missing original PNG")
                state = entry.checkpoint.removeprefix(f"compact-{surface}-")
                images.append(
                    OriginalImage(
                        path=png.resolve(),
                        sha256=entry.png_sha256,
                        screen=surface,
                        dialog=checkpoint_dialog(state),
                        columns=columns,
                        rows=rows,
                        journey=entry.checkpoint,
                        source_head=plan.source.head,
                        binary_sha256=receipt.binary_sha256,
                        capture_driver_sha256=plan.capture_driver.sha256,
                        source_sha256=plan.source.digest(),
                    )
                )
    return images


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("plan", type=Path)
    _ = parser.add_argument(
        "--check",
        action="store_true",
        help="Verify bindings and exact driver test; do not launch TUI",
    )
    _ = parser.add_argument(
        "--cua-quota-fixture",
        choices=t.get_args(QuotaFixtureScenario),
        help="Use the declared CLI libtest component fixture in the quota CUA lease",
    )
    surface_selection = parser.add_mutually_exclusive_group()
    _ = surface_selection.add_argument(
        "--quota-only",
        action="store_true",
        help="Run only the current compact Quota entry; require all nine scenes at every grid",
    )
    _ = surface_selection.add_argument(
        "--sessions-only",
        action="store_true",
        help="Run only compact Sessions; require twenty scenes at all six grids, narrow first",
    )
    _ = surface_selection.add_argument(
        "--cua-surface",
        choices=("sessions", "quota"),
        help="Lease one live ttyd fixture for CUA (600s); no VHS run or UI-proof claim",
    )
    arguments = parser.parse_args(namespace=CommandArguments())
    selection = CaptureSelection.STATIC_BOTH
    if arguments.quota_only:
        selection = CaptureSelection.STATIC_QUOTA
    elif arguments.sessions_only:
        selection = CaptureSelection.STATIC_SESSIONS
    elif arguments.cua_surface == "quota":
        selection = CaptureSelection.CUA_QUOTA
    elif arguments.cua_surface == "sessions":
        selection = CaptureSelection.CUA_SESSIONS
    if arguments.cua_quota_fixture is not None and arguments.cua_surface != "quota":
        parser.error("--cua-quota-fixture requires --cua-surface quota")
    plan = CapturePlan.model_validate_json(arguments.plan.read_bytes())
    if arguments.check:
        check_driver(plan, selection, arguments.cua_quota_fixture)
        print(
            "target binding, exact capture test and blank-image rejection verified; no TUI observation"
        )
    else:
        run_capture(plan, selection, arguments.cua_quota_fixture)
        print(
            plan.output
            / (
                "cua-lifecycle-summary.json"
                if arguments.cua_surface is not None
                else "gallery-manifest.json"
            )
        )


if __name__ == "__main__":
    main()
