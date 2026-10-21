"""Apparatus contracts: stale pixels and incomplete coverage must not earn proof."""

import datetime as dt
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from textwrap import dedent

from pydantic import JsonValue, ValidationError

from scripts.verification.tui_capture import (
    CaptureSelection,
    capture_arguments,
    check_driver,
)
from scripts.verification.tui_capture_lifecycle import (
    StaticCaptureBudget,
    discover_capture_test,
    wait_static_capture,
)
from scripts.verification.tui_public_sessions import (
    PublicHostIdentity,
    PublicSessionsTarget,
    public_sessions_launch_configuration,
)
from scripts.verification.tui_verification import (
    ArtifactBinding,
    CapturePlan,
    CuaProcessIdentity,
    CuaPtyReadback,
    CuaReady,
    FilterReceipt,
    ImageObservation,
    ImageRejection,
    SourceBinding,
    TerminalViewport,
    checkpoint_dialog,
    coverage_gaps,
    source_binding,
    verify_artifact,
)


class TuiEvidenceTests(unittest.TestCase):
    def test_cua_fixture_discovery_rejects_more_than_one_libtest_filter(self) -> None:
        """Exercise discovery argv with a real process; this is no UI oracle proof."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            executable = root / "controlled-libtest-protocol"
            _ = executable.write_text(
                dedent("""\
                #!/usr/bin/python3
                import json,sys
                arguments = sys.argv[1:]
                if arguments[-1] == '--list':
                    filters = [value for value in arguments if not value.startswith('--')]
                    if len(filters) != 1:
                        sys.exit(2)
                    print(filters[0] + ': test\\n1 test, 0 benchmarks')
                elif arguments[0] == 'compact-quota-all':
                    print(json.dumps({'checkpoint': arguments[0], 'recognizedLines': [],
                        'recognizedFieldPairs': [], 'choiceFocusMeasurements': [],
                        'missingMarkers': ['Quota status'], 'unexpectedMarkers': []}))
                    sys.exit(1)
                else:
                    sys.exit(2)
                """)
            )
            executable.chmod(0o700)
            binding = ArtifactBinding.capture(executable)
            repository = Path.cwd().resolve()
            plan = CapturePlan(
                repository=repository,
                output=root / "no-ui-output",
                temporary_directory=root,
                source=source_binding(repository),
                capture_driver=binding,
                harness_binary=binding,
                vhs=binding,
                ttyd=binding,
                image_verifier=binding,
                dynamic_library_path=str(root),
                build_declaration="Controlled libtest argument protocol; no product UI",
            )
            check_driver(plan, CaptureSelection.CUA_QUOTA, "normal")
            self.assertFalse(plan.output.exists())

    def test_sessions_only_selects_exact_entry_and_narrow_first_grids(self) -> None:
        selection = CaptureSelection.STATIC_SESSIONS
        self.assertEqual(
            capture_arguments(Path("/declared/capture"), selection)[1:],
            [
                "compiled_compact_current_sessions_storyboard_exports_real_tui",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ],
        )
        self.assertEqual(selection.static_surfaces, ("sessions",))
        self.assertEqual(selection.blank_scene, ("compact-sessions-all", "Sessions"))
        self.assertEqual(
            selection.static_viewports,
            ((32, 24), (40, 24), (48, 12), (48, 24), (76, 30), (120, 36)),
        )

    def test_public_sessions_launch_uses_exact_binary_cwd_and_normal_environment(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            approved_cwd = root / "approved source"
            approved_cwd.mkdir()
            debug_root = root / "new debug Host"
            debug_root.mkdir()
            record = root / "invocation.json"
            executable = root / "agent-sessions"
            _ = executable.write_text(
                dedent(f"""\
                #!/usr/bin/python3
                import json,os,sys
                with open({str(record)!r},'w') as output:
                    json.dump({{'argv':sys.argv[1:],'cwd':os.getcwd(),
                               'home':os.environ.get('HOME'),
                               'codex_home_present':'CODEX_HOME' in os.environ,
                               'codex_home':os.environ.get('CODEX_HOME'),
                               'root':os.environ.get('CODEX_ROUTER_DEBUG_ROUTER_ROOT'),
                               'sentinel':os.environ.get('PUBLIC_TEST_SENTINEL')}},output)
                """)
            )
            executable.chmod(0o700)
            host = PublicHostIdentity.model_validate(
                {
                    "service_id": "00000000-0000-4000-8000-000000000001",
                    "service_epoch": "00000000-0000-4000-8000-000000000002",
                }
            )
            target = PublicSessionsTarget(
                binary=ArtifactBinding.capture(executable),
                approved_cwd=approved_cwd,
                debug_router_root=debug_root,
                expected_host=host,
            )
            for codex_home in (None, str(root / "normal codex")):
                inherited = {
                    "HOME": str(root / "normal home"),
                    "PUBLIC_TEST_SENTINEL": "preserved",
                }
                if codex_home is not None:
                    inherited["CODEX_HOME"] = codex_home
                original = dict(inherited)
                arguments, environment = public_sessions_launch_configuration(
                    target, host, inherited
                )
                self.assertEqual(arguments, [str(executable)])
                self.assertEqual(inherited, original)
                result = subprocess.run(
                    arguments,
                    cwd=target.approved_cwd,
                    env=environment,
                    stdin=subprocess.DEVNULL,
                    capture_output=True,
                    timeout=2,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                expected: dict[str, JsonValue] = {
                    "argv": [],
                    "cwd": str(approved_cwd),
                    "home": inherited["HOME"],
                    "codex_home_present": codex_home is not None,
                    "codex_home": codex_home,
                    "root": str(debug_root),
                    "sentinel": "preserved",
                }
                self.assertEqual(record.read_text(), json.dumps(expected))

            for field in ("service_id", "service_epoch"):
                wrong = host.model_dump(mode="json")
                wrong[field] = "00000000-0000-4000-8000-000000000003"
                with (
                    self.subTest(field=field),
                    self.assertRaisesRegex(ValueError, "Host service/epoch mismatch"),
                ):
                    _ = public_sessions_launch_configuration(
                        target, PublicHostIdentity.model_validate(wrong), {}
                    )
            with self.assertRaises(ValidationError):
                _ = PublicSessionsTarget.model_validate(
                    {**target.model_dump(), "arguments": ["--new"]}
                )
            _, environment = public_sessions_launch_configuration(target, host, {})
            self.assertNotIn("HOME", environment)
            self.assertNotIn("CODEX_HOME", environment)
            _ = executable.write_text(executable.read_text() + "# changed bytes\n")
            with self.assertRaisesRegex(ValueError, "artifact changed"):
                _ = public_sessions_launch_configuration(target, host, {})

    def test_capture_discovery_bounds_and_reaps_its_exact_child(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = "import sys,time; print('partial list', flush=True); print('partial error', file=sys.stderr, flush=True); time.sleep(2)"
            with self.assertRaisesRegex(
                ValueError, "capture discovery failed"
            ) as failure:
                _ = discover_capture_test(
                    [sys.executable, "-c", fixture], root, timeout=0.15
                )
            message = str(failure.exception)
            self.assertIn('"timeout": true', message)
            self.assertIn('"exit": -9', message)
            self.assertIn("partial list", message)
            self.assertIn("partial error", message)

    def test_capture_discovery_preserves_success_and_nonzero_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertEqual(
                discover_capture_test(
                    [sys.executable, "-c", "print('exact: test')"], root
                ),
                "exact: test\n",
            )
            with self.assertRaisesRegex(
                ValueError, "capture discovery failed"
            ) as failure:
                _ = discover_capture_test(
                    [
                        sys.executable,
                        "-c",
                        "import sys; print('bad list'); print('error',file=sys.stderr); sys.exit(7)",
                    ],
                    root,
                )
            self.assertIn('"exit": 7', str(failure.exception))
            self.assertIn("bad list", str(failure.exception))
            self.assertIn("error", str(failure.exception))

    def test_static_deadline_stays_failure_after_cooperative_child_exit(self) -> None:
        # A real controlled process honors the same private stop protocol. Its
        # self-limit also makes the old unbounded wrapper fail without hanging.
        fixture = """
import pathlib, sys, time
root = pathlib.Path(sys.argv[1])
print('driver started', flush=True)
print('driver stderr', file=sys.stderr, flush=True)
deadline = time.monotonic() + 2
while time.monotonic() < deadline:
    marker = root / 'capture-stop'
    if marker.exists() and marker.read_bytes() == b'stop\\n':
        print('owned cleanup settled', flush=True)
        sys.exit(0)
    time.sleep(0.005)
sys.exit(0)
"""
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with (
                (output / "driver.stdout").open("wb") as stdout,
                (output / "driver.stderr").open("wb") as stderr,
            ):
                child = subprocess.Popen(
                    [sys.executable, "-c", fixture, directory],
                    stdin=subprocess.DEVNULL,
                    stdout=stdout,
                    stderr=stderr,
                )
                try:
                    with self.assertRaisesRegex(
                        TimeoutError, "static capture deadline"
                    ):
                        _ = wait_static_capture(
                            child,
                            output,
                            StaticCaptureBudget(work_seconds=0.15, cleanup_seconds=1),
                        )
                finally:
                    _ = child.wait(timeout=3)
            self.assertEqual(child.returncode, 0)
            self.assertEqual((output / "capture-stop").read_bytes(), b"stop\n")
            self.assertIn(
                "owned cleanup settled", (output / "driver.stdout").read_text()
            )
            self.assertEqual((output / "driver.stderr").read_text(), "driver stderr\n")
            self.assertIn('"exit": 0', (output / "driver-exit.json").read_text())

    def test_static_deadline_reports_unsettled_child_without_signalling_it(
        self,
    ) -> None:
        fixture = """
import pathlib, sys, time
root = pathlib.Path(sys.argv[1])
deadline = time.monotonic() + 2
while not (root / 'fixture-release').exists() and time.monotonic() < deadline:
    time.sleep(0.005)
sys.exit(7)
"""
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            child = subprocess.Popen(
                [sys.executable, "-c", fixture, directory],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            try:
                with self.assertRaisesRegex(TimeoutError, "cleanup unsettled"):
                    _ = wait_static_capture(
                        child,
                        output,
                        StaticCaptureBudget(work_seconds=0.05, cleanup_seconds=0.05),
                    )
                self.assertIsNone(child.poll())
                record = (output / "driver-exit.json").read_text()
                self.assertIn('"exit": null', record)
                self.assertIn('"driver_settled": false', record)
                self.assertEqual((output / "capture-stop").read_bytes(), b"stop\n")
            finally:
                _ = (output / "fixture-release").write_text("release\n")
                _ = child.wait(timeout=3)
            self.assertEqual(child.returncode, 7)

    def test_static_wait_preserves_normal_success_and_failure_without_stop(
        self,
    ) -> None:
        for exit_code in (0, 7):
            with (
                self.subTest(exit_code=exit_code),
                tempfile.TemporaryDirectory() as directory,
            ):
                output = Path(directory)
                child = subprocess.Popen(
                    [sys.executable, "-c", f"raise SystemExit({exit_code})"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                result = wait_static_capture(
                    child,
                    output,
                    StaticCaptureBudget(work_seconds=2, cleanup_seconds=1),
                )
                self.assertEqual(result, exit_code)
                self.assertFalse((output / "capture-stop").exists())
                record = (output / "driver-exit.json").read_text()
                self.assertIn(f'"exit": {exit_code}', record)
                self.assertIn('"deadline_reached": false', record)
                self.assertIn('"driver_settled": true', record)

    def test_static_stop_refuses_to_overwrite_a_foreign_marker(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            marker = output / "capture-stop"
            _ = marker.write_text("different protocol\n")
            child = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(0.2)"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            with self.assertRaisesRegex(
                TimeoutError, "invalid private capture stop marker"
            ):
                _ = wait_static_capture(
                    child,
                    output,
                    StaticCaptureBudget(work_seconds=0.05, cleanup_seconds=1),
                )
            self.assertEqual(child.returncode, 0)
            self.assertEqual(marker.read_text(), "different protocol\n")
            self.assertIn(
                "invalid private capture stop marker",
                (output / "driver-exit.json").read_text(),
            )

    def test_static_budget_accounts_for_every_grid_image_and_cleanup(self) -> None:
        self.assertEqual(CaptureSelection.STATIC_QUOTA.static_budget_seconds, 3210)
        self.assertEqual(CaptureSelection.STATIC_BOTH.static_budget_seconds, 8130)
        self.assertEqual(CaptureSelection.STATIC_SESSIONS.static_budget_seconds, 5010)
        for selection in (CaptureSelection.CUA_QUOTA, CaptureSelection.CUA_SESSIONS):
            with self.assertRaisesRegex(ValueError, "no static image"):
                _ = selection.static_budget_seconds

    def test_gallery_retains_new_fork_and_choice_dialog_identity(self) -> None:
        for checkpoint, expected in (
            ("new-machine", "new"),
            ("fork-choice", "fork"),
            ("codex-details", "details"),
            ("model-provider-choice", "filters"),
            ("provider-no-match", "filters"),
            ("disabled-choice", "filters"),
            ("hidden-choice", "filters"),
            ("machine-sources", "machines"),
        ):
            with self.subTest(checkpoint=checkpoint):
                self.assertEqual(checkpoint_dialog(checkpoint), expected)

    def test_resize_readback_allows_new_dimensions_but_rejects_replaced_pty(
        self,
    ) -> None:
        identity = CuaProcessIdentity(
            pid=123,
            uid=501,
            group=123,
            birth="first start",
            executable="/fixture/harness",
        )
        before = CuaPtyReadback(
            surface="sessions",
            identity=identity,
            viewport=TerminalViewport(columns=120, rows=36),
            source_sha256="a" * 64,
            binary_sha256="b" * 64,
            observed_at=dt.datetime(2026, 10, 18, tzinfo=dt.UTC),
        )
        after = before.model_copy(
            update={"viewport": TerminalViewport(columns=48, rows=12)}
        )
        after.require_same_pty(before)
        for replacement in (
            identity.model_copy(update={"pid": 124}),
            identity.model_copy(update={"birth": "reused PID new start"}),
        ):
            with (
                self.subTest(replacement=replacement),
                self.assertRaisesRegex(ValueError, "replaced the declared PTY"),
            ):
                after.model_copy(update={"identity": replacement}).require_same_pty(
                    before
                )

    def test_live_cua_ready_url_rejects_foreign_or_credential_destinations(
        self,
    ) -> None:
        fields = {
            "kind": "listener-ready-not-ui",
            "surface": "sessions",
            "ttyd_pid": 123,
            "binary_sha256": "a" * 64,
            "ttyd_sha256": "b" * 64,
            "child_pid_file": "/fixture/quota-child.pid",
            "viewport_file": "/fixture/quota-child.viewport",
            "stop_file": "/fixture/cua-stop",
            "lifetime_seconds": 600,
            "artifact_export": {
                "base_url": "http://127.0.0.1:2346/export/" + "a" * 64 + "/",
                "directory": "/fixture/original-images",
                "scenes": ["sessions-details-120x36"],
                "maximum_bytes": 8388608,
                "mime_type": "image/jpeg",
                "source_sha256": "c" * 64,
            },
        }
        _ = CuaReady.model_validate({**fields, "url": "http://127.0.0.1:2345/"})
        for destination in (
            "https://example.com/",
            "http://user:secret@127.0.0.1:2345/",
            "http://127.0.0.1:2345/other",
        ):
            with (
                self.subTest(destination=destination),
                self.assertRaises(ValidationError),
            ):
                _ = CuaReady.model_validate({**fields, "url": destination})
        export = fields["artifact_export"]
        assert isinstance(export, dict)
        for changes in (
            {"base_url": "https://example.com/export/" + "a" * 64 + "/"},
            {"scenes": ["../outside"]},
            {"scenes": ["same", "same"]},
            {"mime_type": "image/png"},
        ):
            with self.subTest(changes=changes), self.assertRaises(ValidationError):
                _ = CuaReady.model_validate(
                    {
                        **fields,
                        "artifact_export": {**export, **changes},
                        "url": "http://127.0.0.1:2345/",
                    }
                )

    def test_mutation_provider_call_or_unsettled_cleanup_receipt_cannot_pass(
        self,
    ) -> None:
        receipt: dict[str, JsonValue] = {
            "surface": "sessions",
            "columns": 120,
            "rows": 36,
            "binary_sha256": "a" * 64,
            "vhs_sha256": "b" * 64,
            "ttyd_sha256": "c" * 64,
            "verifier_sha256": "d" * 64,
            "verifier_source_sha256": "e" * 64,
            "verifier_checkpoint_source_sha256": "c" * 64,
            "driver_source_sha256": "f" * 64,
            "tape_driver_source_sha256": "0" * 64,
            "fixed_child_sha256": "1" * 64,
            "tape_sha256": "2" * 64,
            "state_and_secrets_unchanged": True,
            "provider_requests": 0,
            "child_exit": 0,
            "owned_processes_stopped": True,
            "fixture": "synthetic records; actual compiled renderer",
            "images": [
                {"checkpoint": f"compact-sessions-{state}", "png_sha256": "3" * 64}
                for state in (
                    "all",
                    "chooser",
                    "help",
                    "claude",
                    "cancelled",
                    "details",
                )
            ],
        }
        _ = FilterReceipt.model_validate(receipt)
        failures: tuple[tuple[str, JsonValue], ...] = (
            ("provider_requests", 1),
            ("child_exit", 1),
            ("owned_processes_stopped", False),
            ("state_and_secrets_unchanged", False),
        )
        for field, value in failures:
            with self.subTest(field=field), self.assertRaises(ValidationError):
                _ = FilterReceipt.model_validate({**receipt, field: value})
        images = receipt["images"]
        assert isinstance(images, list)
        with self.assertRaisesRegex(ValidationError, "duplicate checkpoint"):
            _ = FilterReceipt.model_validate(
                {**receipt, "images": [*images, images[0]]}
            )

    def test_negative_control_that_accepted_wrong_image_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValidationError, "accepted the wrong scene"):
            _ = ImageRejection(
                checkpoint="compact-sessions-claude",
                recognizedLines=["wrong screen"],
                recognizedFieldPairs=[],
                choiceFocusMeasurements=[],
                missingMarkers=[],
                unexpectedMarkers=[],
            )
        _ = ImageRejection(
            checkpoint="compact-sessions-claude",
            recognizedLines=["wrong screen"],
            recognizedFieldPairs=[],
            choiceFocusMeasurements=[],
            missingMarkers=[],
            unexpectedMarkers=["Codex Alpha"],
        )

    def test_original_choice_focus_requires_label_and_same_row_glyph(self) -> None:
        original: dict[str, object] = {
            "label": "hidden",
            "labelYellowPixels": 131,
            "glyphYellowPixels": 25,
            "glyphColumns": 5,
            "glyphRows": 12,
            "labelHeight": 20,
        }

        def observation(measurements: list[dict[str, object]]) -> ImageObservation:
            return ImageObservation.model_validate(
                {
                    "checkpoint": "compact-quota-hidden-choice",
                    "recognizedLines": [
                        "Account filters",
                        "Shown",
                        "Hidden",
                        "Cancel Apply",
                    ],
                    "recognizedFieldPairs": [],
                    "choiceFocusMeasurements": measurements,
                    "missingMarkers": [],
                    "unexpectedMarkers": [],
                }
            )

        observation([original]).require_selected_choice("hidden")
        for changed in (
            {"labelYellowPixels": 0},
            {"glyphYellowPixels": 0},
            {"glyphColumns": 0},
            {"glyphRows": 0},
            {"label": "shown"},
        ):
            with (
                self.subTest(changed=changed),
                self.assertRaisesRegex(ValueError, "glyph/color"),
            ):
                observation([{**original, **changed}]).require_selected_choice("hidden")
        for measurements in (
            [],
            [original, original],
            [original, {**original, "label": "shown"}],
        ):
            with (
                self.subTest(measurements=measurements),
                self.assertRaisesRegex(ValueError, "glyph/color"),
            ):
                observation(measurements).require_selected_choice("hidden")
        with self.assertRaises(ValidationError):
            _ = observation([{**original, "label": "OpenAI"}])
        with self.assertRaises(ValidationError):
            _ = observation([{**original, "glyphYellowPixels": -1}])
        raw = observation([original]).model_dump()
        del raw["choiceFocusMeasurements"]
        with self.assertRaises(ValidationError):
            _ = ImageObservation.model_validate(raw)

    def test_changed_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "binary"
            _ = artifact.write_bytes(b"declared build")
            binding = ArtifactBinding.capture(artifact)
            _ = artifact.write_bytes(b"replacement build")
            with self.assertRaisesRegex(ValueError, "artifact changed"):
                verify_artifact(binding)

    def test_scene_oracle_cannot_hide_missing_or_unexpected_markers(self) -> None:
        for missing, unexpected in [(["Apply"], []), ([], ["Quota Help"])]:
            with (
                self.subTest(missing=missing, unexpected=unexpected),
                self.assertRaises(ValidationError),
            ):
                _ = ImageObservation(
                    checkpoint="compact-sessions-chooser",
                    recognizedLines=["Session filters"],
                    recognizedFieldPairs=[],
                    choiceFocusMeasurements=[],
                    missingMarkers=missing,
                    unexpectedMarkers=unexpected,
                )

    def test_compact_route_never_claims_all_menu_or_resize_coverage(self) -> None:
        gaps = coverage_gaps()
        self.assertIn("sessions/new", gaps)
        self.assertIn("quota/reset-result", gaps)
        self.assertTrue(any("48x12" in gap for gap in gaps))
        self.assertTrue(any("40x24" in gap and "32x24" in gap for gap in gaps))
        self.assertTrue(any("actual iPhone" in gap for gap in gaps))
        self.assertIn("resize continuity", gaps)

    def test_source_binding_detects_changed_untracked_input(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            _ = (repository / "input.rs").write_text("first")
            binding = SourceBinding(
                head="a" * 40,
                files={
                    "input.rs": ArtifactBinding.capture(repository / "input.rs").sha256
                },
            )
            binding.verify_files(repository)
            _ = (repository / "input.rs").write_text("second")
            with self.assertRaisesRegex(ValueError, "source changed"):
                binding.verify_files(repository)

    def test_capture_target_rejects_omitted_or_changed_compiled_controls_input(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            repository = root / "source"
            _ = subprocess.run(
                [
                    "git",
                    "clone",
                    "--quiet",
                    "--shared",
                    "--no-checkout",
                    str(Path(__file__).resolve().parents[2]),
                    str(repository),
                ],
                capture_output=True,
                check=True,
                timeout=10,
            )
            support = repository / "crates/capture"
            support.mkdir(parents=True)
            for name in ("image", "checkpoint", "controls"):
                _ = (support / f"verify_screen_{name}.swift").write_text(name)
            binary = root / "protocol-only"
            _ = binary.write_bytes(b"source boundary only; never executed")
            artifact = ArtifactBinding.capture(binary)
            binding = source_binding(repository)
            plan = CapturePlan(
                repository=repository,
                output=root / "output",
                temporary_directory=root,
                source=binding,
                capture_driver=artifact,
                harness_binary=artifact,
                vhs=artifact,
                ttyd=artifact,
                image_verifier=artifact,
                dynamic_library_path=str(root),
                build_declaration="source boundary only",
            )
            plan.verify_target()
            controls = "crates/capture/verify_screen_controls.swift"
            omitted = SourceBinding(
                head=binding.head,
                files={
                    name: digest
                    for name, digest in binding.files.items()
                    if name != controls
                },
            )
            with self.assertRaisesRegex(ValueError, "source changed"):
                plan.model_copy(update={"source": omitted}).verify_target()
            _ = (repository / controls).write_text("changed control fixture")
            with self.assertRaisesRegex(ValueError, "source changed"):
                plan.verify_target()


if __name__ == "__main__":
    _ = unittest.main()
