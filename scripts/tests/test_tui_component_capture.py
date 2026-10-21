"""Component receipt protocol only; fabricated protocol bytes never prove a UI."""

import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from pydantic import ValidationError

from scripts.verification.tui_component_capture import (
    COMPONENT_STATES,
    ComponentPlan,
    ComponentReceipt,
    collect_component_images,
    run_component_capture,
)
from scripts.verification.tui_verification import (
    VIEWPORTS,
    ArtifactBinding,
    ImageObservation,
    SourceBinding,
    TerminalViewport,
)


class ComponentCaptureTests(unittest.TestCase):
    def test_collection_rechecks_real_source_and_artifact_after_driver(self) -> None:
        # Only external Git enumeration/driver execution are controlled. Real
        # file hashes, admission, receipt collection and publication stay real.
        for mutation in ("none", "source", "artifact"):
            with (
                self.subTest(mutation=mutation),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                plan, _ = self.prepare_receipts(root)
                saved = root / "saved-receipts"
                _ = plan.output.rename(saved)
                source = root / "crates/declared-source.rs"
                _ = source.write_text("before driver\n")
                plan = plan.model_copy(
                    update={
                        "source": SourceBinding(
                            head="a" * 40,
                            files={
                                "crates/declared-source.rs": ArtifactBinding.capture(
                                    source
                                ).sha256
                            },
                        )
                    }
                )

                def complete_driver(
                    *_args: object,
                    prepared: tuple[Path, ComponentPlan, Path, str] = (
                        saved,
                        plan,
                        source,
                        mutation,
                    ),
                ) -> int:
                    saved, plan, source, mutation = prepared
                    _ = shutil.copytree(saved, plan.output, dirs_exist_ok=True)
                    if mutation == "source":
                        _ = source.write_text("changed during capture\n")
                    elif mutation == "artifact":
                        _ = plan.component_binary.path.write_bytes(
                            b"changed during capture"
                        )
                    return 0

                rejection = json.dumps(
                    {
                        "checkpoint": "component-quota-policy-saved",
                        "recognizedLines": [],
                        "recognizedFieldPairs": [],
                        "choiceFocusMeasurements": [],
                        "missingMarkers": ["marker"],
                        "unexpectedMarkers": [],
                    }
                )

                def controlled_git_output(_root: Path, arguments: list[str]) -> bytes:
                    return (
                        b"crates/declared-source.rs\0"
                        if arguments[0] == "ls-files"
                        else b"a" * 40
                    )

                def controlled_discovery(command: list[str], _root: Path) -> str:
                    return command[1] + ": test\n"

                with (
                    patch(
                        "scripts.verification.tui_verification.git_output",
                        side_effect=controlled_git_output,
                    ),
                    patch(
                        "scripts.verification.tui_component_capture.discover_capture_test",
                        side_effect=controlled_discovery,
                    ),
                    patch(
                        "scripts.verification.tui_component_capture.subprocess.run",
                        return_value=subprocess.CompletedProcess([], 1, rejection, ""),
                    ),
                    patch(
                        "scripts.verification.tui_component_capture.subprocess.Popen"
                    ),
                    patch(
                        "scripts.verification.tui_component_capture.wait_static_capture",
                        side_effect=complete_driver,
                    ),
                ):
                    if mutation == "none":
                        run_component_capture(plan)
                        self.assertTrue(
                            (plan.output / "component-gallery-manifest.json").exists()
                        )
                    else:
                        with self.assertRaisesRegex(
                            ValueError, "source changed|artifact changed"
                        ):
                            run_component_capture(plan)
                        self.assertFalse(
                            (plan.output / "component-gallery-manifest.json").exists()
                        )

    def prepare_receipts(
        self, root: Path, grid: tuple[int, int] = (120, 36)
    ) -> tuple[ComponentPlan, Path]:
        columns, rows = grid
        binary = root / "protocol-only-binary"
        _ = binary.write_bytes(b"protocol only")
        binding = ArtifactBinding.capture(binary)
        plan = ComponentPlan(
            repository=root,
            output=root / "output",
            temporary_directory=root,
            source=SourceBinding(head="a" * 40, files={binary.name: binding.sha256}),
            capture_driver=binding,
            harness_binary=binding,
            component_binary=binding,
            vhs=binding,
            ttyd=binding,
            image_verifier=binding,
            dynamic_library_path=str(root),
            build_declaration="protocol only, no product",
            grids=[TerminalViewport(columns=columns, rows=rows)],
        )
        folder = plan.output / f"quota-component-normal-{columns}x{rows}"
        folder.mkdir(parents=True)
        support = (
            root
            / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test"
        )
        support.mkdir(parents=True)
        for name in (
            "verify_screen_checkpoint.swift",
            "verify_screen_image.swift",
            "quota_component_capture.rs",
            "quota_component_tape.rs",
        ):
            _ = (support / name).write_bytes(binary.read_bytes())
        for name in ("fixed_quota_child.py", "quota.tape"):
            _ = (folder / name).write_bytes(binary.read_bytes())
        images: list[dict[str, str]] = []
        for state in COMPONENT_STATES:
            checkpoint = f"component-quota-{state}"
            image = folder / f"{checkpoint}.png"
            _ = image.write_bytes(b"\x89PNG\r\n\x1a\n" + b"protocol-only" * 100)
            images.append(
                {
                    "checkpoint": checkpoint,
                    "png_sha256": ArtifactBinding.capture(image).sha256,
                }
            )
            policy = (
                "allow" if state in ("policy-draft", "policy-saved") else "disallow"
            )
            _ = (folder / f"{checkpoint}.image-observation.json").write_text(
                json.dumps(
                    {
                        "checkpoint": checkpoint,
                        "recognizedLines": ["protocol only"],
                        "recognizedFieldPairs": [f"use credits {policy}"]
                        if state
                        in (
                            "policy-original",
                            "policy-draft",
                            "policy-cancelled",
                            "policy-saved",
                            "policy-choice-returned",
                        )
                        else ["current 1%"]
                        if state == "floor-saved"
                        else [],
                        "choiceFocusMeasurements": [],
                        "missingMarkers": [],
                        "unexpectedMarkers": [],
                    }
                )
            )
        for scene, wrong in (
            ("policy-saved", "blank-startup"),
            ("policy-saved", "component-quota-policy-cancelled"),
            ("floor-saved", "component-quota-floor-cancelled"),
            ("floor-saved", "component-quota-floor-draft"),
            ("credits-refreshed-balance", "component-quota-credits-balance"),
            ("returned", "component-quota-reset-result"),
            ("help-bottom", "component-quota-help-top"),
            ("details-bottom", "component-quota-details-top"),
            ("reset-result-bottom", "component-quota-confirm-yes"),
            ("browse-credit-window", "component-quota-next-account"),
            ("browse-pace-heading", "component-quota-next-account"),
            ("browse-pace-value", "component-quota-reset-result"),
        ):
            checkpoint = f"component-quota-{scene}"
            _ = (
                folder / f"reject-{checkpoint}-{wrong}.image-observation.json"
            ).write_text(
                json.dumps(
                    {
                        "checkpoint": checkpoint,
                        "recognizedLines": [],
                        "recognizedFieldPairs": [],
                        "choiceFocusMeasurements": [],
                        "missingMarkers": ["saved value"],
                        "unexpectedMarkers": [],
                    }
                )
            )
        receipt = {
            key: binding.sha256
            for key in (
                "binary_sha256",
                "verifier_sha256",
                "checkpoint_source_sha256",
                "recognition_source_sha256",
                "driver_source_sha256",
                "tape_source_sha256",
                "fixed_child_sha256",
                "tape_sha256",
            )
        }
        _ = (folder / "component-receipt.json").write_text(
            json.dumps(
                {
                    **receipt,
                    "kind": "compiled-quota-component-normal",
                    "columns": columns,
                    "rows": rows,
                    "child_exit": 0,
                    "external_requests": 0,
                    "persisted_writes": 0,
                    "owned_children_stopped": True,
                    "images": images,
                }
            )
        )
        return plan, folder

    def test_complete_protocol_and_finite_budget(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            plan, _ = self.prepare_receipts(Path(directory).resolve())
            images = collect_component_images(plan)
            self.assertEqual(len(images), 42)
            for scene, dialog in (
                ("confirm-no", "Confirm reset"),
                ("confirm-no-returned", "Options · Resets"),
                ("reset-result-bottom", "Reset result"),
                ("policy-saved", "Credit usage"),
            ):
                image = next(
                    image
                    for image in images
                    if image["journey"] == f"component-quota-{scene}"
                )
                self.assertEqual(image["dialog"], dialog)
            help_identity = next(
                image
                for image in images
                if image["journey"] == "component-quota-confirm-help-identity"
            )
            limits = help_identity["limits"]
            assert isinstance(limits, str)
            self.assertIn("Personal original-image inspection", limits)
            self.assertIn("literal tag brackets", limits)
            self.assertIn("raw OCR is unchanged", limits)
            self.assertEqual(plan.work_seconds, 1900)
            self.assertEqual(plan.work_seconds + 90, 1990)
            all_grids = ComponentPlan.model_validate(
                {
                    **plan.model_dump(),
                    "grids": [
                        {"columns": columns, "rows": rows}
                        for columns, rows in VIEWPORTS
                    ],
                }
            )
            self.assertEqual(all_grids.work_seconds + 90, 11490)
            for grids in (
                [],
                [(48, 13)],
                [(40, 12)],
                [(32, 12)],
                [(48, 12), (48, 12)],
                [*VIEWPORTS, (32, 24)],
            ):
                with self.subTest(grids=grids), self.assertRaises(ValidationError):
                    _ = ComponentPlan.model_validate(
                        {
                            **plan.model_dump(),
                            "grids": [
                                {"columns": columns, "rows": rows}
                                for columns, rows in grids
                            ],
                        }
                    )

    def test_phone_receipt_cannot_substitute_a_clamped_48_column_viewport(self) -> None:
        for columns in (40, 32):
            with (
                self.subTest(columns=columns),
                tempfile.TemporaryDirectory() as directory,
            ):
                plan, folder = self.prepare_receipts(
                    Path(directory).resolve(), (columns, 24)
                )
                originals = collect_component_images(plan)
                self.assertEqual(len(originals), 42)
                self.assertTrue(
                    all(
                        image["viewport"] == {"columns": columns, "rows": 24}
                        for image in originals
                    )
                )
                receipt_path = folder / "component-receipt.json"
                receipt = ComponentReceipt.model_validate_json(
                    receipt_path.read_bytes()
                ).model_dump(mode="json")
                receipt["columns"] = 48
                _ = receipt_path.write_text(json.dumps(receipt))
                with self.assertRaisesRegex(ValueError, "viewport mismatch"):
                    _ = collect_component_images(plan)

    def test_wrong_saved_values_effects_sources_or_scene_cannot_earn_proof(
        self,
    ) -> None:
        for failure in (
            "draft-is-not-saved",
            "floor-draft-is-not-saved",
            "missing-floor-draft-negative",
            "missing-stale-refresh-negative",
            "missing-negative",
            "missing-returned-negative",
            "missing-help-bottom-negative",
            "missing-details-bottom-negative",
            "missing-result-bottom-negative",
            "missing-compact-credit-negative",
            "missing-short-window-negative",
            "missing-lower-pace-negative",
            "wrong-binary",
            "changed-source",
            "reordered",
            "missing-source",
            "provider-effect",
            "extra-image",
            "first-frame-diagnostic",
        ):
            with (
                self.subTest(failure=failure),
                tempfile.TemporaryDirectory() as directory,
            ):
                plan, folder = self.prepare_receipts(Path(directory).resolve())
                receipt_path = folder / "component-receipt.json"
                receipt = ComponentReceipt.model_validate_json(
                    receipt_path.read_text()
                ).model_dump(mode="json")
                if failure == "draft-is-not-saved":
                    observation = (
                        folder / "component-quota-policy-saved.image-observation.json"
                    )
                    value = ImageObservation.model_validate_json(
                        observation.read_text()
                    ).model_dump(mode="json")
                    value["recognizedFieldPairs"] = ["use credits disallow"]
                    _ = observation.write_text(json.dumps(value))
                elif failure == "floor-draft-is-not-saved":
                    observation = (
                        folder / "component-quota-floor-saved.image-observation.json"
                    )
                    value = ImageObservation.model_validate_json(
                        observation.read_text()
                    ).model_dump(mode="json")
                    value["recognizedFieldPairs"] = []
                    _ = observation.write_text(json.dumps(value))
                elif failure == "missing-floor-draft-negative":
                    (
                        folder
                        / "reject-component-quota-floor-saved-component-quota-floor-draft.image-observation.json"
                    ).unlink()
                elif failure == "missing-stale-refresh-negative":
                    (
                        folder
                        / "reject-component-quota-credits-refreshed-balance-component-quota-credits-balance.image-observation.json"
                    ).unlink()
                elif failure == "missing-negative":
                    (
                        folder
                        / "reject-component-quota-policy-saved-component-quota-policy-cancelled.image-observation.json"
                    ).unlink()
                elif failure == "missing-returned-negative":
                    (
                        folder
                        / "reject-component-quota-returned-component-quota-reset-result.image-observation.json"
                    ).unlink()
                elif failure == "missing-compact-credit-negative":
                    (
                        folder
                        / "reject-component-quota-browse-credit-window-component-quota-next-account.image-observation.json"
                    ).unlink()
                elif failure == "missing-lower-pace-negative":
                    (
                        folder
                        / "reject-component-quota-browse-pace-value-component-quota-reset-result.image-observation.json"
                    ).unlink()
                elif failure == "missing-short-window-negative":
                    (
                        folder
                        / "reject-component-quota-browse-pace-heading-component-quota-next-account.image-observation.json"
                    ).unlink()
                elif failure == "missing-result-bottom-negative":
                    (
                        folder
                        / "reject-component-quota-reset-result-bottom-component-quota-confirm-yes.image-observation.json"
                    ).unlink()
                elif failure in (
                    "missing-help-bottom-negative",
                    "missing-details-bottom-negative",
                ):
                    dialog = (
                        "help"
                        if failure == "missing-help-bottom-negative"
                        else "details"
                    )
                    (
                        folder
                        / f"reject-component-quota-{dialog}-bottom-component-quota-{dialog}-top.image-observation.json"
                    ).unlink()
                elif failure == "wrong-binary":
                    receipt["binary_sha256"] = "0" * 64
                elif failure == "changed-source":
                    _ = (
                        plan.repository
                        / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/verify_screen_checkpoint.swift"
                    ).write_text("changed markers")
                elif failure == "reordered":
                    receipt["images"][18], receipt["images"][20] = (
                        receipt["images"][20],
                        receipt["images"][18],
                    )
                elif failure == "missing-source":
                    del receipt["checkpoint_source_sha256"]
                elif failure == "provider-effect":
                    receipt["external_requests"] = 1
                elif failure == "extra-image":
                    _ = (folder / "extra.png").write_text("stale")
                elif failure == "first-frame-diagnostic":
                    receipt["kind"] = "compiled-quota-first-frame-diagnostic"
                _ = receipt_path.write_text(json.dumps(receipt))
                with self.assertRaises((ValueError, FileNotFoundError)):
                    _ = collect_component_images(plan)


if __name__ == "__main__":
    _ = unittest.main()
