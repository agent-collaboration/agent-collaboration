"""The wrapper must select one real capture, never compile or broaden the suite."""

import errno
import fcntl
import json
import os
import pty
import runpy
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import typing as t
import unittest
from enum import Enum
from pathlib import Path

from scripts.verification.tui_capture import (
    CaptureSelection,
    capture_arguments,
    collect_cua_artifacts,
    collect_images,
    require_one_test,
    validate_image_qualification,
)
from scripts.verification.tui_verification import (
    QUOTA_CHECKPOINT_STATES,
    SESSION_CHECKPOINT_STATES,
    VIEWPORTS,
    ArtifactBinding,
    CapturePlan,
    FilterReceipt,
    ImageObservation,
    SourceBinding,
)


class ComponentAdapterOutcome(Enum):
    VALID = "valid"
    MISSING_RECEIPT = "missing-receipt"
    NONZERO_CHILD = "nonzero-child"
    FORGED_CHILD_MARKER = "forged-child-marker"


class TuiCaptureSelectionTests(unittest.TestCase):
    def build_measured_details_receipts(self, root: Path) -> CapturePlan:
        """Literal adopted pose sets exercise the real receipt collector only."""
        plan = self.build_static_receipts(root, CaptureSelection.STATIC_SESSIONS)
        pose_states = {
            (32, 24): ("details-title", "details", "details-path"),
            (40, 24): ("details-title", "details", "details-path"),
            (48, 12): (
                "details-title", "details-identity", "details-state", "details-path"
            ),
            (48, 24): ("details-title", "details"),
            (76, 30): ("details",),
            (120, 36): ("details",),
        }
        fields = [
            "details context claude gamma [000.0011]",
            "details title claude gamma session · project-a",
            "details path /repo/project-a",
            "details session id 00000000-0000-4000-8000-000000000011",
            "details endpoint claude gamma",
            "details state idle",
        ]
        for (columns, rows), states in pose_states.items():
            folder = plan.output / f"sessions-compact-{columns}x{rows}"
            path = folder / "filter-receipt.json"
            receipt = FilterReceipt.model_validate_json(path.read_bytes())
            updated = receipt.model_dump(mode="json")
            images: list[dict[str, str]] = []
            for entry in receipt.images:
                if entry.checkpoint != "compact-sessions-details":
                    images.append(entry.model_dump())
                    continue
                for state in states:
                    name = f"compact-sessions-{state}"
                    image = folder / f"{name}.png"
                    _ = image.write_bytes(b"\x89PNG\r\n\x1a\n" + b"protocol-only" * 100)
                    images.append(
                        {"checkpoint": name, "png_sha256": ArtifactBinding.capture(image).sha256}
                    )
                    observation = {
                        "checkpoint": name, "recognizedLines": ["protocol-only"],
                        "recognizedFieldPairs": fields, "choiceFocusMeasurements": [],
                        "missingMarkers": [], "unexpectedMarkers": [],
                    }
                    _ = (folder / f"{name}.image-observation.json").write_text(
                        json.dumps(observation)
                    )
                    observation["recognizedFieldPairs"] = []
                    observation["missingMarkers"] = ["captured session context"]
                    _ = (folder / f"reject-{name}-compact-sessions-codex-details.image-observation.json").write_text(json.dumps(observation))
            if "details" not in states:
                (folder / "compact-sessions-details.png").unlink()
            updated["images"] = images
            _ = path.write_text(json.dumps(updated))
        short = plan.output / "sessions-compact-48x12"
        rejection = {
            "checkpoint": "compact-sessions-details-identity",
            "recognizedLines": ["protocol-only"], "recognizedFieldPairs": [],
            "choiceFocusMeasurements": [], "missingMarkers": ["Session ID"],
            "unexpectedMarkers": [],
        }
        _ = (short / "reject-compact-sessions-details-identity-compact-sessions-details-title.image-observation.json").write_text(json.dumps(rejection))
        return plan

    def test_sessions_accepts_measured_ordered_details_pose_sets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            plan = self.build_measured_details_receipts(Path(directory))
            images = collect_images(plan, CaptureSelection.STATIC_SESSIONS)
            self.assertEqual(len(images), 128)
            self.assertEqual(
                sum(image.journey.startswith("compact-sessions-details") for image in images),
                14,
            )

    def test_sessions_details_rejects_incomplete_foreign_or_cross_pose_evidence(self) -> None:
        for failure in ("missing", "extra", "reordered", "foreign-build", "wrong-id", "missing-title", "cross-pose"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                plan = self.build_measured_details_receipts(Path(directory))
                folder = plan.output / "sessions-compact-48x12"
                path = folder / "filter-receipt.json"
                receipt = FilterReceipt.model_validate_json(path.read_bytes()).model_dump(mode="json")
                images = FilterReceipt.model_validate(receipt).images
                if failure in ("missing", "extra", "reordered"):
                    entries = [entry.model_dump() for entry in images]
                    index = next(i for i, entry in enumerate(images) if entry.checkpoint == "compact-sessions-details-identity")
                    if failure == "missing":
                        _ = entries.pop(index)
                    elif failure == "extra":
                        entries.insert(index, entries[index])
                    else:
                        entries[index], entries[index + 1] = entries[index + 1], entries[index]
                    receipt["images"] = entries
                elif failure == "foreign-build":
                    receipt["binary_sha256"] = "f" * 64
                else:
                    state = "details-title" if failure == "missing-title" else "details-identity"
                    observation_path = folder / f"compact-sessions-{state}.image-observation.json"
                    observation = ImageObservation.model_validate_json(observation_path.read_bytes()).model_dump(mode="json")
                    if failure == "cross-pose":
                        observation["checkpoint"] = "compact-sessions-details-title"
                    elif failure == "wrong-id":
                        observation["recognizedFieldPairs"] = ["details context claude gamma [000.0011]", "details session id 00000000-0000-4000-8000-000000000012"]
                    else:
                        observation["recognizedFieldPairs"] = ["details context claude gamma [000.0011]"]
                    _ = observation_path.write_text(json.dumps(observation))
                _ = path.write_text(json.dumps(receipt))
                with self.assertRaises((ValueError, FileNotFoundError)):
                    _ = collect_images(plan, CaptureSelection.STATIC_SESSIONS)

    def test_sessions_chooser_rejects_missing_or_wrong_client_association(self) -> None:
        for pairs in (
            [],
            ["client cursor"],
            ["draft codex"],
            ["client codex", "client cursor"],
        ):
            with self.subTest(pairs=pairs), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                plan = self.build_static_receipts(root, CaptureSelection.STATIC_BOTH)
                columns, rows = VIEWPORTS[0]
                observation_path = (
                    plan.output
                    / f"sessions-compact-{columns}x{rows}"
                    / "compact-sessions-chooser.image-observation.json"
                )
                observation: dict[str, object] = ImageObservation.model_validate_json(
                    observation_path.read_text()
                ).model_dump(mode="json")
                observation["recognizedFieldPairs"] = pairs
                _ = observation_path.write_text(json.dumps(observation))
                with self.assertRaisesRegex(ValueError, "associated.*client"):
                    _ = collect_images(plan, CaptureSelection.STATIC_BOTH)

    def build_static_receipts(
        self, root: Path, selection: CaptureSelection
    ) -> CapturePlan:
        """Receipt protocol bytes only; no image oracle or product is substituted as proof."""
        target = root / "apparatus-target"
        _ = target.write_bytes(b"receipt protocol fixture")
        binding = ArtifactBinding.capture(target)
        plan = CapturePlan(
            repository=root,
            output=root / "run",
            temporary_directory=root,
            source=SourceBinding(head="a" * 40, files={target.name: binding.sha256}),
            capture_driver=binding,
            harness_binary=binding,
            vhs=binding,
            ttyd=binding,
            image_verifier=binding,
            dynamic_library_path=str(root),
            build_declaration="receipt collector protocol fixture, no product or real image",
        )
        support = (
            root
            / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test"
        )
        support.mkdir(parents=True)
        for name in (
            "provider_filter_capture.rs",
            "provider_filter_tape.rs",
            "verify_screen_image.swift",
            "verify_screen_checkpoint.swift",
        ):
            _ = (support / name).write_bytes(target.read_bytes())
        for surface in selection.static_surfaces:
            states = (
                QUOTA_CHECKPOINT_STATES
                if surface == "quota"
                else SESSION_CHECKPOINT_STATES
            )
            for columns, rows in VIEWPORTS:
                folder = plan.output / f"{surface}-compact-{columns}x{rows}"
                folder.mkdir(parents=True)
                for name in ("quota.tape", "fixed_quota_child.py"):
                    _ = (folder / name).write_bytes(target.read_bytes())
                images: list[dict[str, str]] = []
                for state in states:
                    checkpoint = f"compact-{surface}-{state}"
                    image = folder / f"{checkpoint}.png"
                    _ = image.write_bytes(b"\x89PNG\r\n\x1a\n" + b"protocol-only" * 100)
                    images.append(
                        {
                            "checkpoint": checkpoint,
                            "png_sha256": ArtifactBinding.capture(image).sha256,
                        }
                    )
                    _ = (folder / f"{checkpoint}.image-observation.json").write_text(
                        json.dumps(
                            {
                                "checkpoint": checkpoint,
                                "recognizedLines": ["› Shown"]
                                if checkpoint == "compact-quota-disabled-choice"
                                else ["› Hidden"]
                                if checkpoint == "compact-quota-hidden-choice"
                                else ["protocol-only"],
                                "choiceFocusMeasurements": [],
                                "recognizedFieldPairs": [
                                    "account provider openai",
                                    "disabled accounts shown",
                                ]
                                if checkpoint == "compact-quota-chooser"
                                else ["client codex"]
                                if checkpoint == "compact-sessions-chooser"
                                else [],
                                "missingMarkers": [],
                                "unexpectedMarkers": [],
                            }
                        )
                    )
                for expected, wrong in (
                    (f"compact-{surface}-claude", "blank-startup"),
                    (f"compact-{surface}-claude", f"compact-{surface}-all"),
                    (f"compact-{surface}-cancelled", f"compact-{surface}-chooser"),
                ) + (
                    (("compact-quota-hidden-choice", "compact-quota-disabled-choice"),)
                    if surface == "quota"
                    else ()
                ):
                    _ = (
                        folder / f"reject-{expected}-{wrong}.image-observation.json"
                    ).write_text(
                        json.dumps(
                            {
                                "checkpoint": expected,
                                "recognizedLines": [],
                                "choiceFocusMeasurements": [],
                                "recognizedFieldPairs": [],
                                "missingMarkers": ["required marker"],
                                "unexpectedMarkers": [],
                            }
                        )
                    )
                receipt: dict[str, object] = {
                    key: binding.sha256
                    for key in (
                        "binary_sha256",
                        "vhs_sha256",
                        "ttyd_sha256",
                        "verifier_sha256",
                        "verifier_source_sha256",
                        "verifier_checkpoint_source_sha256",
                        "driver_source_sha256",
                        "tape_driver_source_sha256",
                        "fixed_child_sha256",
                        "tape_sha256",
                    )
                }
                receipt.update(
                    surface=surface,
                    columns=columns,
                    rows=rows,
                    state_and_secrets_unchanged=True,
                    provider_requests=0,
                    child_exit=0,
                    owned_processes_stopped=True,
                    fixture="receipt protocol only",
                    images=images,
                )
                _ = (folder / "filter-receipt.json").write_text(json.dumps(receipt))
        return plan

    def test_static_selection_collects_every_declared_scene_and_grid(self) -> None:
        for selection, count, surfaces in (
            (CaptureSelection.STATIC_BOTH, 174, {"sessions", "quota"}),
            (CaptureSelection.STATIC_QUOTA, 54, {"quota"}),
            (CaptureSelection.STATIC_SESSIONS, 120, {"sessions"}),
        ):
            with (
                self.subTest(selection=selection),
                tempfile.TemporaryDirectory() as directory,
            ):
                plan = self.build_static_receipts(Path(directory), selection)
                images = collect_images(plan, selection)
                self.assertEqual(len(images), count)
                self.assertEqual({image.screen for image in images}, surfaces)
                self.assertEqual(
                    {(image.columns, image.rows) for image in images}, set(VIEWPORTS)
                )

    def test_sessions_only_rejects_missing_extra_wrong_and_unchecked_receipts(
        self,
    ) -> None:
        selection = CaptureSelection.STATIC_SESSIONS
        for failure in (
            "missing-grid",
            "extra-quota",
            "wrong-surface",
            "missing-negative",
        ):
            with (
                self.subTest(failure=failure),
                tempfile.TemporaryDirectory() as directory,
            ):
                plan = self.build_static_receipts(Path(directory), selection)
                folder = plan.output / "sessions-compact-32x24"
                receipt_path = folder / "filter-receipt.json"
                if failure == "missing-grid":
                    receipt_path.unlink()
                elif failure == "extra-quota":
                    extra = plan.output / "quota-compact-32x24"
                    extra.mkdir()
                    _ = (extra / "filter-receipt.json").write_bytes(
                        receipt_path.read_bytes()
                    )
                elif failure == "wrong-surface":
                    receipt: dict[str, object] = FilterReceipt.model_validate_json(
                        receipt_path.read_bytes()
                    ).model_dump(mode="json")
                    receipt["surface"] = "quota"
                    _ = receipt_path.write_text(json.dumps(receipt))
                else:
                    (
                        folder
                        / "reject-compact-sessions-claude-blank-startup.image-observation.json"
                    ).unlink()
                with self.assertRaises((ValueError, FileNotFoundError)):
                    _ = collect_images(plan, selection)

    def test_quota_collection_rejects_missing_wrong_extra_scene_target_and_negative(
        self,
    ) -> None:
        for failure in (
            "missing-grid",
            "missing-image",
            "extra-image",
            "extra-receipt",
            "missing-scene",
            "missing-choice-scene",
            "reordered-choice-scenes",
            "missing-choice-negative",
            "missing-choice-focus",
            "wrong-choice-focus",
            "wrong-field-association",
            "missing-field-association",
            "wrong-scene",
            "extra-scene",
            "wrong-target",
            "changed-checkpoint-source",
            "missing-checkpoint-source",
            "provider-effect",
            "accepted-negative",
        ):
            with (
                self.subTest(failure=failure),
                tempfile.TemporaryDirectory() as directory,
            ):
                plan = self.build_static_receipts(
                    Path(directory), CaptureSelection.STATIC_QUOTA
                )
                folder = plan.output / "quota-compact-48x12"
                path = folder / "filter-receipt.json"
                decoded = FilterReceipt.model_validate_json(path.read_bytes())
                receipt: dict[str, object] = decoded.model_dump(mode="json")
                images = [
                    {"checkpoint": image.checkpoint, "png_sha256": image.png_sha256}
                    for image in decoded.images
                ]
                receipt["images"] = images
                if failure == "missing-grid":
                    path.unlink()
                elif failure == "changed-checkpoint-source":
                    checkpoint_source = (
                        plan.repository
                        / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/verify_screen_checkpoint.swift"
                    )
                    _ = checkpoint_source.write_bytes(b"different scene expectations")
                elif failure == "missing-checkpoint-source":
                    del receipt["verifier_checkpoint_source_sha256"]
                    _ = path.write_text(json.dumps(receipt))
                elif failure == "missing-image":
                    (folder / "compact-quota-details.png").unlink()
                elif failure == "extra-image":
                    _ = (folder / "unexpected.png").write_bytes(b"not a UI image")
                elif failure == "extra-receipt":
                    extra_folder = plan.output / "sessions-compact-120x36"
                    extra_folder.mkdir()
                    _ = (extra_folder / "filter-receipt.json").write_bytes(
                        path.read_bytes()
                    )
                elif failure == "missing-choice-negative":
                    (
                        folder
                        / "reject-compact-quota-hidden-choice-compact-quota-disabled-choice.image-observation.json"
                    ).unlink(missing_ok=True)
                elif failure in ("missing-choice-focus", "wrong-choice-focus"):
                    observation_path = (
                        folder / "compact-quota-hidden-choice.image-observation.json"
                    )
                    observation = ImageObservation.model_validate_json(
                        observation_path.read_text()
                    ).model_dump(mode="json")
                    observation["recognizedLines"] = [
                        "Account filters",
                        "Shown",
                        "Hidden",
                        "Cancel Apply",
                    ]
                    observation["choiceFocusMeasurements"] = (
                        []
                        if failure == "missing-choice-focus"
                        else [
                            {
                                "label": "shown",
                                "labelYellowPixels": 131,
                                "glyphYellowPixels": 25,
                                "glyphColumns": 5,
                                "glyphRows": 12,
                                "labelHeight": 20,
                            }
                        ]
                    )
                    _ = observation_path.write_text(json.dumps(observation))
                elif failure in (
                    "wrong-field-association",
                    "missing-field-association",
                ):
                    observation_path = (
                        folder / "compact-quota-chooser.image-observation.json"
                    )
                    observation: dict[str, object] = (
                        ImageObservation.model_validate_json(
                            observation_path.read_text()
                        ).model_dump(mode="json")
                    )
                    if failure == "missing-field-association":
                        del observation["recognizedFieldPairs"]
                    else:
                        observation["recognizedFieldPairs"] = [
                            "account provider claude",
                            "disabled accounts shown",
                        ]
                    _ = observation_path.write_text(json.dumps(observation))
                elif failure == "accepted-negative":
                    negative = (
                        folder
                        / "reject-compact-quota-claude-blank-startup.image-observation.json"
                    )
                    _ = negative.write_text(
                        json.dumps(
                            {
                                "checkpoint": "compact-quota-claude",
                                "recognizedLines": [],
                                "choiceFocusMeasurements": [],
                                "recognizedFieldPairs": [],
                                "missingMarkers": [],
                                "unexpectedMarkers": [],
                            }
                        )
                    )
                else:
                    if failure == "missing-scene":
                        _ = images.pop()
                    elif failure == "missing-choice-scene":
                        images[:] = [
                            image
                            for image in images
                            if image["checkpoint"] != "compact-quota-hidden-choice"
                        ]
                    elif failure == "reordered-choice-scenes":
                        images[4], images[5] = images[5], images[4]
                    elif failure == "wrong-scene":
                        images[-1]["checkpoint"] = "compact-quota-unexpected"
                    elif failure == "extra-scene":
                        images.append(
                            {
                                "checkpoint": "compact-quota-extra",
                                "png_sha256": "b" * 64,
                            }
                        )
                    elif failure == "wrong-target":
                        receipt["binary_sha256"] = "b" * 64
                    elif failure == "provider-effect":
                        receipt["provider_requests"] = 1
                    _ = path.write_text(json.dumps(receipt))
                with self.assertRaises((ValueError, FileNotFoundError)):
                    _ = collect_images(plan, CaptureSelection.STATIC_QUOTA)

    def test_static_surface_and_cua_modes_are_mutually_exclusive(self) -> None:
        for flags in (
            ("--quota-only", "--cua-surface", "quota"),
            ("--sessions-only", "--cua-surface", "sessions"),
            ("--sessions-only", "--quota-only"),
        ):
            result = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    "-m",
                    "scripts.verification.tui_capture",
                    "unused-plan.json",
                    *flags,
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 2)
            self.assertIn("not allowed with argument", result.stderr)
        for selection in (CaptureSelection.CUA_QUOTA, CaptureSelection.CUA_SESSIONS):
            with self.assertRaisesRegex(ValueError, "no static image"):
                _ = selection.static_surfaces

    def test_quota_blank_oracle_cannot_use_sessions_or_accept_blank(self) -> None:
        result = json.dumps(
            {
                "checkpoint": "compact-quota-all",
                "recognizedLines": [],
                "choiceFocusMeasurements": [],
                "recognizedFieldPairs": [],
                "missingMarkers": ["Quota status"],
                "unexpectedMarkers": [],
            }
        )
        validate_image_qualification(1, result, CaptureSelection.STATIC_QUOTA)
        for wrong in (
            result.replace("compact-quota-all", "compact-sessions-all"),
            result.replace("Quota status", "Sessions"),
        ):
            with self.assertRaisesRegex(ValueError, "exact declared scene"):
                validate_image_qualification(1, wrong, CaptureSelection.STATIC_QUOTA)
        for exit_code in (0, 2, 133):
            with self.assertRaisesRegex(ValueError, "unavailable or blank accepted"):
                validate_image_qualification(
                    exit_code, result, CaptureSelection.STATIC_QUOTA
                )

    def test_quota_only_selects_its_exact_declared_entry_without_sessions(self) -> None:
        arguments = capture_arguments(
            Path("/declared/capture"), CaptureSelection.STATIC_QUOTA
        )
        self.assertEqual(
            arguments,
            [
                "/declared/capture",
                "compiled_compact_current_quota_storyboard_exports_real_tui",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ],
        )

    def test_original_cua_collector_rejects_modified_bytes_and_wrong_count(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "controlled-binary"
            _ = binary.write_bytes(b"apparatus only")
            binding = ArtifactBinding.capture(binary)
            plan = CapturePlan(
                repository=root,
                output=root / "run",
                temporary_directory=root,
                source=SourceBinding(
                    head="a" * 40, files={"controlled-binary": binding.sha256}
                ),
                capture_driver=binding,
                harness_binary=binding,
                vhs=binding,
                ttyd=binding,
                image_verifier=binding,
                dynamic_library_path=str(root),
                build_declaration="controlled artifact metadata; no product run",
                cua_artifact_scenes=["quota-details-120x36"],
            )
            folder = plan.output / "cua-quota" / "original-images"
            folder.mkdir(parents=True)
            image = folder / "quota-details-120x36.jpg"
            _ = image.write_bytes(b"\xff\xd8raw protocol bytes\xff\xd9")
            image_binding = ArtifactBinding.capture(image)
            _ = (folder / "quota-details-120x36.receipt.json").write_text(
                json.dumps(
                    {
                        "kind": "original-cua-jpeg-bytes-only",
                        "scene": "quota-details-120x36",
                        "filename": image.name,
                        "bytes": image.stat().st_size,
                        "sha256": image_binding.sha256,
                        "encoding_round_trip": True,
                    }
                )
            )
            artifacts = collect_cua_artifacts(plan, "quota", 1)
            self.assertEqual(
                artifacts[0]["result"], "original-bytes-retained-ui-judgment-separate"
            )
            # A self-consistent partial receipt must not satisfy a four-scene plan.
            scenes = ["wide-draft", "narrow-draft", "restored-draft"]
            plan.cua_artifact_scenes.extend(scenes)
            with self.assertRaisesRegex(ValueError, "count or declared"):
                _ = collect_cua_artifacts(plan, "quota", 1)
            original_receipt = t.cast(
                dict[str, object],
                json.loads((folder / "quota-details-120x36.receipt.json").read_text()),
            )
            for scene in scenes:
                _ = (folder / f"{scene}.jpg").write_bytes(image.read_bytes())
                _ = (folder / f"{scene}.receipt.json").write_text(
                    json.dumps(
                        original_receipt | {"scene": scene, "filename": f"{scene}.jpg"}
                    )
                )
            self.assertEqual(len(collect_cua_artifacts(plan, "quota", 4)), 4)
            with self.assertRaisesRegex(ValueError, "count or declared"):
                _ = collect_cua_artifacts(plan, "quota", 2)
            extra = folder / "undeclared.jpg"
            _ = extra.write_bytes(image.read_bytes())
            with self.assertRaisesRegex(ValueError, "count or declared"):
                _ = collect_cua_artifacts(plan, "quota", 4)
            extra.unlink()
            extra_receipt = folder / "undeclared.receipt.json"
            _ = extra_receipt.write_text(json.dumps(original_receipt))
            with self.assertRaisesRegex(ValueError, "count or declared"):
                _ = collect_cua_artifacts(plan, "quota", 4)
            extra_receipt.unlink()
            last_receipt = folder / f"{scenes[-1]}.receipt.json"
            saved_receipt = last_receipt.read_bytes()
            last_receipt.unlink()
            with self.assertRaisesRegex(ValueError, "count or declared"):
                _ = collect_cua_artifacts(plan, "quota", 3)
            _ = last_receipt.write_bytes(saved_receipt)
            missing_image = folder / f"{scenes[-1]}.jpg"
            saved_image = missing_image.read_bytes()
            missing_image.unlink()
            with self.assertRaises(FileNotFoundError):
                _ = collect_cua_artifacts(plan, "quota", 4)
            _ = missing_image.write_bytes(saved_image)
            _ = image.write_bytes(b"\xff\xd8modified\xff\xd9")
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                _ = collect_cua_artifacts(plan, "quota", 4)

    def run_component_adapter(
        self, root: Path, outcome: ComponentAdapterOutcome
    ) -> subprocess.CompletedProcess[str]:
        output = root / "output"
        home = root / "home"
        output.mkdir(mode=0o700)
        home.mkdir(mode=0o700)
        child = root / "controlled-child"
        _ = child.write_text(
            "\n".join(
                [
                    f"#!{sys.executable}",
                    "import json, os, pathlib, sys",
                    "home = pathlib.Path(os.environ['HOME'])",
                    "(home / 'invocation.json').write_text(json.dumps({'arguments':sys.argv[1:], 'scenario':os.environ.get('QUOTA_COMPONENT_FIXTURE')}))",
                    "for line in range(40): print(f'controlled scrollback line {line}')",
                    f"if {outcome not in (ComponentAdapterOutcome.MISSING_RECEIPT, ComponentAdapterOutcome.FORGED_CHILD_MARKER)!r}:",
                    "    pathlib.Path(os.environ['QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE']).write_text('QUOTA_COMPONENT_FIXTURE_FINISHED scenario=Normal released=false external_requests=0 persisted_writes=0\\n')",
                    f"if {outcome is ComponentAdapterOutcome.FORGED_CHILD_MARKER!r}: print('QUOTA_STORYBOARD_CHILD_EXIT=0')",
                    f"sys.exit({7 if outcome is ComponentAdapterOutcome.NONZERO_CHILD else 0})",
                    "",
                ]
            )
        )
        child.chmod(0o700)
        configuration = root / "configuration.json"
        _ = configuration.write_text(
            json.dumps(
                {
                    "binary": str(child),
                    "surface": "quota-component",
                    "output": str(output),
                    "temporary_directory": str(home),
                    "component_fixture_scenario": "normal",
                    "component_fixture_receipt": str(output / "fixture-finished"),
                }
            )
        )
        adapter = (
            Path(__file__).resolve().parents[2]
            / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/fixed_quota_child.py"
        )
        master, slave = pty.openpty()
        try:
            _ = fcntl.ioctl(
                slave, termios.TIOCSWINSZ, struct.pack("HHHH", 12, 48, 0, 0)
            )
            chunks: list[bytes] = []
            # Drain during execution: a real macOS PTY can fill before forty
            # lines finish, and this test needs actual terminal scrollback.
            with tempfile.TemporaryFile() as errors:
                child = subprocess.Popen(
                    [sys.executable, str(adapter)],
                    stdin=slave,
                    stdout=slave,
                    stderr=errors,
                    env={"QUOTA_STORYBOARD_FIXTURE_COMMAND": str(configuration)},
                    start_new_session=True,
                )
                deadline = time.monotonic() + 5
                try:
                    while child.poll() is None:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0:
                            raise TimeoutError("controlled PTY adapter did not exit")
                        readable, _, _ = select.select(
                            [master], [], [], min(0.05, remaining)
                        )
                        if readable:
                            chunks.append(os.read(master, 4096))
                finally:
                    if child.poll() is None:
                        # Only this test-created process group is signalled.
                        os.killpg(child.pid, signal.SIGTERM)
                        try:
                            _ = child.wait(timeout=1)
                        except subprocess.TimeoutExpired:
                            os.killpg(child.pid, signal.SIGKILL)
                            _ = child.wait(timeout=1)
                _ = errors.seek(0)
                stderr = errors.read().decode()
            os.close(slave)
            slave = -1
            # The child is reaped; closing the last slave gives real PTY EOF.
            try:
                while chunk := os.read(master, 4096):
                    chunks.append(chunk)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
            self.assertIsNotNone(child.returncode)
            with self.assertRaises(ProcessLookupError):
                os.killpg(child.pid, 0)
            assert child.returncode is not None
            return subprocess.CompletedProcess(
                child.args, child.returncode, b"".join(chunks).decode(), stderr
            )
        finally:
            os.close(master)
            if slave != -1:
                os.close(slave)

    def test_component_adapter_selects_exact_fixture_and_requires_its_receipt(
        self,
    ) -> None:
        for outcome in ComponentAdapterOutcome:
            with (
                self.subTest(outcome=outcome),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                result = self.run_component_adapter(root, outcome)
                if outcome is not ComponentAdapterOutcome.VALID:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertTrue((root / "home/invocation.json").is_file())
                    self.assertNotEqual(
                        (root / "output/quota-child.exit").read_text(), "0"
                    )
                    self.assertEqual(
                        (root / "output/component-process.exit").read_text(),
                        "7"
                        if outcome is ComponentAdapterOutcome.NONZERO_CHILD
                        else "0",
                    )
                    if outcome is ComponentAdapterOutcome.FORGED_CHILD_MARKER:
                        self.assertIn("QUOTA_STORYBOARD_CHILD_EXIT=0", result.stdout)
                        self.assertEqual(
                            (root / "output/quota-child.exit").read_text(), "2"
                        )
                    continue
                self.assertEqual(result.returncode, 0, result.stderr)
                invocation = t.cast(
                    dict[str, object],
                    json.loads((root / "home/invocation.json").read_text()),
                )
                self.assertEqual(invocation["scenario"], "normal")
                self.assertEqual(
                    invocation["arguments"],
                    [
                        "presentation::quota::quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture",
                        "--exact",
                        "--ignored",
                        "--nocapture",
                        "--test-threads=1",
                    ],
                )

    def test_component_exit_marker_stays_on_the_final_cursor_line(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = self.run_component_adapter(root, ComponentAdapterOutcome.VALID)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertGreater(result.stdout.count("\n"), 12)
            self.assertEqual(
                result.stdout.split("\n")[-1], "QUOTA_STORYBOARD_CHILD_EXIT=0"
            )
            self.assertEqual(
                (root / "output/quota-child.viewport").read_text(), "48 12"
            )

    def test_component_pending_receipt_requires_exact_release_and_fresh_paths(
        self,
    ) -> None:
        adapter = (
            Path(__file__).resolve().parents[2]
            / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/fixed_quota_child.py"
        )
        functions = runpy.run_path(str(adapter))
        builder = t.cast(
            t.Callable[[dict[str, object]], tuple[list[str], dict[str, str]]],
            functions["child_launch_configuration"],
        )
        verify = t.cast(
            t.Callable[[dict[str, object]], None],
            functions["verify_component_completion"],
        )
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            receipt, release = output / "receipt", output / "release"
            configuration: dict[str, object] = {
                "surface": "quota-component",
                "binary": "/declared/test",
                "temporary_directory": str(output),
                "output": str(output),
                "component_fixture_scenario": "commit-pending",
                "component_fixture_receipt": str(receipt),
                "component_fixture_release": str(release),
            }
            _, environment = builder(configuration)
            self.assertEqual(
                environment["QUOTA_COMPONENT_FIXTURE_RELEASE_FILE"], str(release)
            )
            marker = "QUOTA_COMPONENT_FIXTURE_FINISHED scenario=CommitPending released=true external_requests=0 persisted_writes=0\n"
            _ = receipt.write_text(marker)
            with self.assertRaises((OSError, ValueError)):
                verify(configuration)
            _ = release.write_bytes(b"wrong\n")
            with self.assertRaisesRegex(ValueError, "exact owned release"):
                verify(configuration)
            _ = release.write_bytes(b"settle\n")
            verify(configuration)
            _ = receipt.write_text(marker.replace("CommitPending", "Normal"))
            with self.assertRaisesRegex(ValueError, "receipt mismatch"):
                verify(configuration)
            with self.assertRaisesRegex(ValueError, "must be absent"):
                _ = builder(configuration)

    def test_adapter_keeps_existing_surfaces_and_rejects_unknown_scenario(self) -> None:
        adapter = (
            Path(__file__).resolve().parents[2]
            / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/fixed_quota_child.py"
        )
        builder = t.cast(
            t.Callable[[dict[str, object]], tuple[list[str], dict[str, str]]],
            runpy.run_path(str(adapter))["child_launch_configuration"],
        )
        configuration: dict[str, object] = {
            "surface": "quota",
            "binary": "/declared/test",
            "root": "/owned/quoted ' root",
            "capability": "owned-capability",
            "provider": "127.0.0.1:1234",
            "temporary_directory": "/owned/home",
            "output": "/owned/output",
        }
        arguments, environment = builder(configuration)
        self.assertEqual(
            arguments,
            [
                "/declared/test",
                "--router-root",
                "/owned/quoted ' root",
                "--fixture-capability",
                "owned-capability",
                "--provider-listener",
                "127.0.0.1:1234",
            ],
        )
        self.assertEqual(set(environment), {"HOME", "TERM", "LANG", "TMPDIR"})
        arguments, _ = builder({**configuration, "surface": "sessions"})
        self.assertEqual(
            arguments,
            [
                "/declared/test",
                "--sessions-picker",
                "--provider-filter-storyboard",
                "--fixture-capability",
                "owned-capability",
            ],
        )
        with self.assertRaisesRegex(ValueError, "unknown component fixture scenario"):
            _ = builder(
                {
                    **configuration,
                    "surface": "quota-component",
                    "component_fixture_scenario": "arbitrary",
                }
            )

    def test_image_qualification_requires_real_negative_not_harness_failure(
        self,
    ) -> None:
        negative = json.dumps(
            {
                "checkpoint": "compact-sessions-all",
                "recognizedLines": [],
                "recognizedFieldPairs": [],
                "choiceFocusMeasurements": [],
                "missingMarkers": ["Sessions"],
                "unexpectedMarkers": [],
            }
        )
        validate_image_qualification(1, negative)
        for exit_code in (0, 2, 133):
            with (
                self.subTest(exit_code=exit_code),
                self.assertRaisesRegex(
                    ValueError, "image oracle unavailable or blank accepted"
                ),
            ):
                validate_image_qualification(exit_code, negative)
        with self.assertRaisesRegex(ValueError, "exact declared scene"):
            validate_image_qualification(
                1, negative.replace("compact-sessions-all", "compact-quota-all")
            )

    def test_capture_selection_is_exact_ignored_and_serial(self) -> None:
        arguments = capture_arguments(Path("/declared/capture"))
        self.assertEqual(arguments[0], "/declared/capture")
        self.assertEqual(
            arguments[1], "compiled_compact_current_storyboard_exports_both_real_tuis"
        )
        self.assertIn("--exact", arguments)
        self.assertIn("--ignored", arguments)
        self.assertIn("--test-threads=1", arguments)
        self.assertNotIn("cargo", arguments)

    def test_empty_or_broadened_discovery_is_not_a_pass(self) -> None:
        require_one_test(
            "compiled_compact_current_storyboard_exports_both_real_tuis: test\n1 test, 0 benchmarks"
        )
        for discovery in [
            "0 tests, 0 benchmarks",
            "unrelated: test\n1 test, 0 benchmarks",
        ]:
            with (
                self.subTest(discovery=discovery),
                self.assertRaisesRegex(ValueError, "exact capture test"),
            ):
                require_one_test(discovery)


if __name__ == "__main__":
    _ = unittest.main()
