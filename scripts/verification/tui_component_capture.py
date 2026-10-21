"""Observe the declared existing normal Quota component fixture via VHS/ttyd.

Pending and actual resize journeys use the existing live CUA lease. This module
selects no vendor, starts no Host and supplies no arbitrary executable arguments.
"""

import argparse
import json
import os
import subprocess
import typing as t
from pathlib import Path

from pydantic import Field, field_validator

from scripts.verification.tui_capture import require_digest, require_one_test
from scripts.verification.tui_capture_lifecycle import (
    StaticCaptureBudget,
    discover_capture_test,
    wait_static_capture,
)
from scripts.verification.tui_verification import (
    VIEWPORTS,
    ArtifactBinding,
    CapturePlan,
    EvidenceModel,
    ImageObservation,
    ImageRejection,
    TerminalViewport,
    verify_artifact,
)

COMPONENT_CAPTURE_TEST = "compiled_normal_quota_component_dialogs_export_real_tui"
COMPONENT_FIXTURE_TEST = "presentation::quota::quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture"
COMPONENT_STATES = (
    "browse",
    "browse-credit-window",
    "browse-pace-heading",
    "browse-pace-value",
    "browse-page-returned",
    "next-account",
    "restored-account",
    "details-top",
    "details-bottom",
    "help-top",
    "help-bottom",
    "floor-original",
    "floor-draft",
    "floor-cancelled",
    "floor-saved",
    "options-identity",
    "reset-inventory",
    "reset-last-page",
    "credits-identity",
    "credits-balance",
    "policy-original",
    "policy-choice",
    "policy-no-match",
    "policy-choice-returned",
    "policy-draft",
    "policy-cancelled",
    "policy-saved",
    "refresh-result",
    "credits-refreshed-balance",
    "credits-refreshed-freshness",
    "credits-refreshed-observation",
    "credits-policy-boundary",
    "reset-review",
    "confirm-no",
    "confirm-no-returned",
    "confirm-yes",
    "confirm-help-top",
    "confirm-help-identity",
    "confirm-help-returned",
    "reset-result",
    "reset-result-bottom",
    "returned",
)


class ComponentPlan(CapturePlan):
    component_binary: ArtifactBinding
    grids: list[TerminalViewport] = Field(min_length=1, max_length=len(VIEWPORTS))

    @field_validator("grids")
    @classmethod
    def declared_grids(cls, value: list[TerminalViewport]) -> list[TerminalViewport]:
        grids = [(grid.columns, grid.rows) for grid in value]
        if len(set(grids)) != len(grids) or any(
            grid not in VIEWPORTS for grid in grids
        ):
            raise ValueError("component grids must be unique supported viewports")
        return value

    @property
    def work_seconds(self) -> int:
        # Existing240s VHS,30s per positive/12 wrong images,40s cleanup pergrid.
        return len(self.grids) * (240 + 30 * (len(COMPONENT_STATES) + 12) + 40)

    def require_target(self) -> None:
        self.verify_target()
        verify_artifact(self.component_binary)


class ComponentImage(EvidenceModel):
    checkpoint: str = Field(pattern=r"^component-quota-[a-z]+(?:-[a-z]+)*$")
    png_sha256: str = Field(pattern=r"^[a-f0-9]{64}$")


class ComponentReceipt(EvidenceModel):
    kind: t.Literal["compiled-quota-component-normal"]
    columns: int
    rows: int
    binary_sha256: str
    verifier_sha256: str
    checkpoint_source_sha256: str
    recognition_source_sha256: str
    driver_source_sha256: str
    tape_source_sha256: str
    fixed_child_sha256: str
    tape_sha256: str
    child_exit: t.Literal[0]
    external_requests: t.Literal[0]
    persisted_writes: t.Literal[0]
    owned_children_stopped: t.Literal[True]
    images: list[ComponentImage]


def component_dialog(state: str) -> str:
    if state.startswith("browse") or state in (
        "next-account",
        "restored-account",
        "returned",
    ):
        return "browse"
    if state.startswith("details-"):
        return "Account details"
    if state.startswith("help-") or state in (
        "confirm-help-top",
        "confirm-help-identity",
    ):
        return "Quota Help"
    if state.startswith("floor-"):
        return "Weekly floor"
    if state.startswith("policy-"):
        return "Credit usage"
    if state.startswith("reset-result"):
        return "Reset result"
    if state in ("confirm-no", "confirm-yes", "confirm-help-returned"):
        return "Confirm reset"
    if state.startswith("credits-") or state == "refresh-result":
        return "Options · Credits"
    return "Options · Resets"


def collect_component_images(plan: ComponentPlan) -> list[dict[str, object]]:
    expected_scenes = [f"component-quota-{state}" for state in COMPONENT_STATES]
    expected_folders = {
        f"quota-component-normal-{g.columns}x{g.rows}" for g in plan.grids
    }
    folders = {
        path.parent.name for path in plan.output.glob("*/component-receipt.json")
    }
    if folders != expected_folders:
        raise ValueError("missing or extra component receipt grid")
    records: list[dict[str, object]] = []
    for grid in plan.grids:
        folder = plan.output / f"quota-component-normal-{grid.columns}x{grid.rows}"
        receipt = ComponentReceipt.model_validate_json(
            (folder / "component-receipt.json").read_bytes()
        )
        if (receipt.columns, receipt.rows) != (grid.columns, grid.rows):
            raise ValueError("component receipt viewport mismatch")
        if (
            receipt.binary_sha256 != plan.component_binary.sha256
            or receipt.verifier_sha256 != plan.image_verifier.sha256
        ):
            raise ValueError("component receipt executable changed")
        support = (
            plan.repository
            / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test"
        )
        for name, digest in (
            ("verify_screen_checkpoint.swift", receipt.checkpoint_source_sha256),
            ("verify_screen_image.swift", receipt.recognition_source_sha256),
            ("quota_component_capture.rs", receipt.driver_source_sha256),
            ("quota_component_tape.rs", receipt.tape_source_sha256),
        ):
            require_digest(support / name, digest)
        require_digest(folder / "fixed_quota_child.py", receipt.fixed_child_sha256)
        require_digest(folder / "quota.tape", receipt.tape_sha256)
        if [image.checkpoint for image in receipt.images] != expected_scenes:
            raise ValueError("component scene missing, replaced, repeated or reordered")
        if {image.stem for image in folder.glob("*.png")} != set(expected_scenes):
            raise ValueError("component original image census mismatch")
        for scene, wrong in (
            ("component-quota-policy-saved", "blank-startup"),
            ("component-quota-policy-saved", "component-quota-policy-cancelled"),
            ("component-quota-floor-saved", "component-quota-floor-cancelled"),
            ("component-quota-floor-saved", "component-quota-floor-draft"),
            (
                "component-quota-credits-refreshed-balance",
                "component-quota-credits-balance",
            ),
            ("component-quota-returned", "component-quota-reset-result"),
            ("component-quota-help-bottom", "component-quota-help-top"),
            ("component-quota-details-bottom", "component-quota-details-top"),
            ("component-quota-reset-result-bottom", "component-quota-confirm-yes"),
            ("component-quota-browse-credit-window", "component-quota-next-account"),
            ("component-quota-browse-pace-heading", "component-quota-next-account"),
            ("component-quota-browse-pace-value", "component-quota-reset-result"),
        ):
            rejection = ImageRejection.model_validate_json(
                (folder / f"reject-{scene}-{wrong}.image-observation.json").read_bytes()
            )
            if rejection.checkpoint != scene:
                raise ValueError("component negative checkpoint mismatch")
        for entry in receipt.images:
            image = folder / f"{entry.checkpoint}.png"
            require_digest(image, entry.png_sha256)
            observation = ImageObservation.model_validate_json(
                (folder / f"{entry.checkpoint}.image-observation.json").read_bytes()
            )
            if observation.checkpoint != entry.checkpoint:
                raise ValueError("component image checkpoint mismatch")
            policy = (
                "allow"
                if entry.checkpoint.endswith(("policy-draft", "policy-saved"))
                else "disallow"
            )
            if entry.checkpoint in {
                f"component-quota-policy-{state}"
                for state in (
                    "original",
                    "draft",
                    "cancelled",
                    "saved",
                    "choice-returned",
                )
            } and observation.recognizedFieldPairs != [f"use credits {policy}"]:
                raise ValueError("component lacks associated policy value")
            if (
                entry.checkpoint == "component-quota-floor-saved"
                and observation.recognizedFieldPairs != ["current 1%"]
            ):
                raise ValueError("component lacks associated saved floor value")
            content = image.read_bytes()
            if len(content) <= 1000 or not content.startswith(b"\x89PNG\r\n\x1a\n"):
                raise ValueError("component original PNG missing")
            records.append(
                {
                    "path": str(image),
                    "sha256": entry.png_sha256,
                    "screen": "Quota status",
                    "dialog": component_dialog(
                        entry.checkpoint.removeprefix("component-quota-")
                    ),
                    "journey": entry.checkpoint,
                    "viewport": grid.model_dump(),
                    "source_head": plan.source.head,
                    "component_sha256": plan.component_binary.sha256,
                    "driver_sha256": plan.capture_driver.sha256,
                    "checker_sha256": plan.image_verifier.sha256,
                    "source_sha256": plan.source.digest(),
                    "result": "Scene/receipt checks passed; personal visual and independent cleanup review pending",
                    "visual_inspection": "pending",
                    "limits": (
                        "Synthetic external ports/model; real compiled UI/input/PTY. "
                        + "Not provider/storage/public-entry acceptance. "
                        + (
                            "Exact full name and tag value share the measured Account field; "
                            + "OCR does not prove the appearance of its one leading/trailing decoration. "
                            + "Personal original-image inspection must verify literal tag brackets; raw OCR is unchanged."
                            if entry.checkpoint
                            == "component-quota-confirm-help-identity"
                            else ""
                        )
                    ),
                }
            )
    return records


def run_component_capture(plan: ComponentPlan) -> None:
    plan.require_target()
    for binary, selector in (
        (plan.capture_driver.path, COMPONENT_CAPTURE_TEST),
        (plan.component_binary.path, COMPONENT_FIXTURE_TEST),
    ):
        require_one_test(
            discover_capture_test(
                [str(binary), selector, "--exact", "--list"], plan.repository
            ),
            selector,
        )
    blank = (
        plan.repository
        / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/fixtures/blank-startup.png"
    )
    qualification = subprocess.run(
        [str(plan.image_verifier.path), "component-quota-policy-saved", str(blank)],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if qualification.returncode != 1:
        raise ValueError("component blank oracle unavailable or accepted")
    rejection = ImageRejection.model_validate_json(qualification.stdout)
    if rejection.checkpoint != "component-quota-policy-saved":
        raise ValueError("component blank oracle wrong checkpoint")
    plan.output.mkdir(mode=0o700, exist_ok=False)
    _ = (plan.output / "declared-target.json").write_text(
        plan.model_dump_json(indent=2)
    )
    environment = dict(os.environ)
    _ = environment.pop("QUOTA_STORYBOARD_CUA_SURFACE", None)
    environment.update(
        {
            "QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY": str(
                plan.component_binary.path
            ),
            "QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO": "normal",
            "QUOTA_STORYBOARD_COMPONENT_VIEWPORTS": ",".join(
                f"{g.columns}x{g.rows}" for g in plan.grids
            ),
            "QUOTA_STORYBOARD_OUTPUT": str(plan.output),
            "QUOTA_STORYBOARD_VHS": str(plan.vhs.path),
            "QUOTA_STORYBOARD_TTYD": str(plan.ttyd.path),
            "QUOTA_STORYBOARD_IMAGE_VERIFIER": str(plan.image_verifier.path),
            "QUOTA_STORYBOARD_DYLD_LIBRARY_PATH": plan.dynamic_library_path,
            "TMPDIR": str(plan.temporary_directory),
        }
    )
    command = [
        str(plan.capture_driver.path),
        COMPONENT_CAPTURE_TEST,
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]
    _ = (plan.output / "command.json").write_text(json.dumps(command))
    with (
        (plan.output / "driver.stdout").open("w") as stdout,
        (plan.output / "driver.stderr").open("w") as stderr,
    ):
        child = subprocess.Popen(
            command,
            cwd=plan.repository,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=stdout,
            stderr=stderr,
        )
        result = wait_static_capture(
            child,
            plan.output,
            StaticCaptureBudget(work_seconds=plan.work_seconds, cleanup_seconds=90),
        )
    if result != 0:
        raise ValueError(
            "component driver failed; retain originals and settle owned resources"
        )
    plan.require_target()
    records = collect_component_images(plan)
    _ = (plan.output / "component-gallery-manifest.json").write_text(
        json.dumps(
            {
                "kind": "partial-compiled-component-proof",
                "complete": False,
                "images": records,
                "pending": [
                    "personal original image inspection",
                    "pending scenarios/resize",
                    "public/provider observations",
                ],
            },
            indent=2,
        )
    )


class ComponentArguments(argparse.Namespace):
    plan: Path = Path()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("plan", type=Path)
    arguments = parser.parse_args(namespace=ComponentArguments())
    run_component_capture(
        ComponentPlan.model_validate_json(arguments.plan.read_bytes())
    )


if __name__ == "__main__":
    main()
