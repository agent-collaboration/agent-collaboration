"""Apparatus controls; these tests make no central product claim."""

import hashlib
import json
import os
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

from scripts.verification.central_runtime import (
    check_source,
    native_command,
    read_test_counts,
)
from scripts.verification.central_staging_models import Plan as StagingPlan
from scripts.verification.central_worker_scenarios import (
    DEFAULT_WORKERD_MODES,
    check_workerd_output,
    read_owner_retirement_state,
    retired_scenarios,
    worker_build_command,
    workerd_command,
    workerd_scenarios,
)


class TestStagingPreparation(unittest.TestCase):
    def test_prepare_records_unknown_without_executing_a_target(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = Path(folder).resolve()
            repository = fixture / "source"
            repository.mkdir()
            commands = fixture / "commands"
            commands.mkdir()
            # The real CLI must only parse/write a declaration. Any attempt to
            # inspect source, build or start a runtime reaches a failing sentinel.
            for name in ["git", "cargo", "node"]:
                executable = commands / name
                _ = executable.write_text(
                    '#!/bin/sh\nprintf "%s\\n" "$0 $*" >> "$STAGING_COMMAND_CAPTURE"\nexit 86\n'
                )
                executable.chmod(0o700)
            capture = fixture / "commands.txt"
            evidence = fixture / "evidence"
            declaration = fixture / "plan.json"
            _ = declaration.write_text(
                json.dumps(
                    {
                        "repository": str(repository),
                        "expected_head": "1" * 40,
                        "evidence": str(evidence),
                        "runtime_parent": "/private/tmp/g4-prepare-fixture",
                        "artifact_root": str(fixture / "absent-artifacts"),
                        "target": {
                            "kind": "localWorkerd",
                            "node": str(commands / "node"),
                            "miniflare": str(fixture / "absent-miniflare"),
                            "native_probe": {
                                "path": str(fixture / "absent-probe"),
                                "sha256": "2" * 64,
                            },
                            "bundle": str(fixture / "absent-bundle"),
                            "bundle_js_sha256": "3" * 64,
                            "bundle_wasm_sha256": "4" * 64,
                        },
                        "cases": ["access"],
                    }
                )
            )
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "scripts.verification.central_staging",
                    "--plan",
                    str(declaration),
                ],
                cwd=Path(__file__).resolve().parents[2],
                env=os.environ
                | {
                    "PATH": str(commands) + os.pathsep + os.environ["PATH"],
                    "STAGING_COMMAND_CAPTURE": str(capture),
                    "PYTHONDONTWRITEBYTECODE": "1",
                },
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertFalse(capture.exists(), "preparation executed a target command")
            self.assertIn("OB7 UNOBSERVED", (evidence / "preparation.txt").read_text())
            saved = StagingPlan.model_validate_json(
                (evidence / "declaration.json").read_text()
            )
            self.assertEqual(saved.expected_head, "1" * 40)
            self.assertEqual(saved.target.kind, "localWorkerd")
            self.assertEqual(saved.cases, ["access"])
            self.assertFalse((evidence / "journey").exists())
            self.assertFalse((evidence / "driver.json").exists())
            self.assertEqual(evidence.stat().st_mode & 0o077, 0)


class TestOwnerRetirementReadback(unittest.TestCase):
    @staticmethod
    def fixture_table(directory: Path, filename: str, rows: int) -> Path:
        database = directory / filename
        connection = sqlite3.connect(database)
        try:
            _ = connection.execute(
                "CREATE TABLE owner_retirement_rows(id TEXT PRIMARY KEY)"
            )
            for index in range(rows):
                _ = connection.execute(
                    "INSERT INTO owner_retirement_rows VALUES (?)",
                    (f"019f0000-0000-7000-8000-{index:012x}",),
                )
            connection.commit()
        finally:
            connection.close()
        return database

    def test_empty_physical_fixture_is_read_without_changing_its_file(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            database = self.fixture_table(Path(folder), "copy.sqlite", 0)
            before = hashlib.sha256(database.read_bytes()).hexdigest()
            observed = read_owner_retirement_state(Path(folder))
            self.assertEqual(observed.rows, 0)
            self.assertEqual(observed.database, database.resolve())
            self.assertEqual(observed.sha256, before)
            self.assertEqual(hashlib.sha256(database.read_bytes()).hexdigest(), before)

    def test_retained_project_rows_cannot_pass_the_readback(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            _ = self.fixture_table(Path(folder), "copy.sqlite", 1)
            with self.assertRaisesRegex(ValueError, "retained 1 physical project rows"):
                _ = read_owner_retirement_state(Path(folder))

    def test_missing_or_ambiguous_fixture_cannot_pass_the_readback(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaisesRegex(
                ValueError, "exactly one physical fixture table"
            ):
                _ = read_owner_retirement_state(Path(folder))
            _ = self.fixture_table(Path(folder), "first.sqlite", 0)
            _ = self.fixture_table(Path(folder), "second.sqlite", 0)
            with self.assertRaisesRegex(
                ValueError, "exactly one physical fixture table"
            ):
                _ = read_owner_retirement_state(Path(folder))


class TestObservationCounts(unittest.TestCase):
    def test_counts_each_target_once_and_keeps_ignored_separate(self) -> None:
        output = (
            "fixture quoted: test result: ok. 900 passed; 0 failed; 0 ignored;\n"
            "test result: ok. 2 passed; 0 failed; 1 ignored;\n"
            "test result: ok. 9 passed; 0 failed; 0 ignored;\n"
        )
        counts = read_test_counts(output, 2)
        self.assertEqual(
            (counts.passed, counts.failed, counts.ignored, counts.targets),
            (11, 0, 1, 2),
        )

    def test_zero_tests_is_failure_even_with_success_exit_text(self) -> None:
        with self.assertRaisesRegex(ValueError, "zero-test"):
            _ = read_test_counts("test result: ok. 0 passed; 0 failed; 0 ignored;", 1)

    def test_missing_target_is_failure(self) -> None:
        with self.assertRaisesRegex(ValueError, "expected 2 test targets"):
            _ = read_test_counts("test result: ok. 1 passed; 0 failed; 0 ignored;", 2)

    def test_failed_target_is_failure(self) -> None:
        with self.assertRaisesRegex(ValueError, "failed or zero-test"):
            _ = read_test_counts(
                "test result: FAILED. 1 passed; 1 failed; 0 ignored;", 1
            )

    def test_command_passes_flags_as_distinct_arguments(self) -> None:
        self.assertEqual(
            native_command(("local_access",)),
            [
                "cargo",
                "test",
                "--locked",
                "-p",
                "collaboration-central-conformance",
                "--test",
                "local_access",
                "--",
                "--nocapture",
            ],
        )

    def test_command_rejects_option_or_shell_input(self) -> None:
        for targets in ((), ("--workspace",), ("local_storage; touch unexpected",)):
            with self.subTest(targets=targets), self.assertRaises(ValueError):
                _ = native_command(targets)
        with self.assertRaises(ValueError):
            _ = native_command(("step_transactions",), "unrelated-package")
        with self.assertRaises(ValueError):
            _ = native_command(
                ("step_transactions",), features="conformance; echo unsafe"
            )

    def test_declared_tokio_slice_keeps_feature_and_test_flags_separate(self) -> None:
        command = native_command(
            ("step_transactions",), "collaboration-central-tokio", "conformance"
        )
        self.assertEqual(
            command[4:8],
            ["collaboration-central-tokio", "--features", "conformance", "--test"],
        )

    def test_exact_filter_preserves_filtered_count_and_rejects_empty_execution(
        self,
    ) -> None:
        counts = read_test_counts(
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out;",
            1,
        )
        self.assertEqual((counts.passed, counts.filtered_out), (1, 3))
        with self.assertRaisesRegex(ValueError, "zero-test"):
            _ = read_test_counts(
                "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out;",
                1,
            )

    def test_exact_name_is_a_distinct_argument_and_cannot_add_options(self) -> None:
        command = native_command(("local_access",), test_name="valid_profile")
        self.assertEqual(
            command[-5:],
            ["local_access", "valid_profile", "--", "--exact", "--nocapture"],
        )
        with self.assertRaises(ValueError):
            _ = native_command(("local_access",), test_name="--skip other")

    def test_retired_access_profile_does_not_substitute_legacy_scenarios(
        self,
    ) -> None:
        with self.assertRaisesRegex(ValueError, "Retired"):
            _ = workerd_scenarios(("access-profile",))

    def test_retired_worker_modes_require_explicit_current_selection(
        self,
    ) -> None:
        for mode in ("conformance", "dag-edge", "access-zero-age", "access-workflow"):
            with self.subTest(mode=mode), self.assertRaisesRegex(ValueError, "Retired"):
                _ = workerd_scenarios((mode,))

    def test_retired_native_target_requires_explicit_current_selection(self) -> None:
        with self.assertRaisesRegex(ValueError, "Retired"):
            _ = native_command(("local_storage",))


class TestCurrentWorkerCommands(unittest.TestCase):
    def test_closure_modes_use_normal_entry_and_require_complete_settlement(
        self,
    ) -> None:
        contracts = {
            "claim-archived": (
                "CLAIM_ARCHIVED_RESULT 1 passed; 0 failed; 0 ignored (shared Native/Worker scenario; actual Archived unretired imported seed, normal Claim exact board_archived refusal and owning Fleet reservation absent before/after; no D1b HTTP import or physical restart claim)",
                1,
            ),
            "archive-reports": (
                "ARCHIVE_REPORT_RESULT 1 passed; 0 failed; 0 ignored (same retained archive-report oracle as Native; actual normal Worker sessions, owning Fleet clock, exact first-report/replay/board/workspace/feed/ledger observations; no reservation-race or hosted claim)",
                1,
            ),
            "imported-claim": (
                "IMPORTED_CLAIM_RESULT 2 passed; 0 failed; 0 ignored (conformance Open import/unheld/seed on actual composed stores; normal owner retirement refusal with unchanged seed, normal exact Claim/replay/fresh AlreadyHeld, Fleet Directory and Copy fence; no D1b HTTP import, private publication-ack read, Retiring/Retired precedence or physical restart claim)",
                1,
            ),
            "claim-retiring": (
                "CLAIM_RETIRING_RESULT 1 passed; 0 failed; 0 ignored (real imported seed and normal owner CopyMismatch retirement; cached Register overridden, old Claim NotFound replay retained, fresh Claim exact transfer refusal, normal Directory Unheld; no private publication-ack or physical restart claim)",
                1,
            ),
            "claim-retired": (
                "CLAIM_RETIRED_RESULT 1 passed; 0 failed; 0 ignored (real imported seed and normal completed owner retirement; Retired outranks cached Register, cached Claim and fresh Claim; no D1b admin import or physical restart claim)",
                1,
            ),
            "forward-origin": (
                "FORWARD_SESSION_RESULT 1 passed; 0 failed; 0 ignored (selected retained Native oracle through actual normal Worker enrollment/Fleet-create/Register/session/Registry/holder; no capacity, deadline, physical restart or full row19 claim)",
                1,
            ),
            "forward-disconnected": (
                "FORWARD_SESSION_RESULT 1 passed; 0 failed; 0 ignored (selected retained Native oracle through actual normal Worker enrollment/Fleet-create/Register/session/Registry/holder; no capacity, deadline, physical restart or full row19 claim)",
                1,
            ),
            "forward-reconnect": (
                "FORWARD_SESSION_RESULT 1 passed; 0 failed; 0 ignored (selected retained Native oracle through actual normal Worker enrollment/Fleet-create/Register/session/Registry/holder; no capacity, deadline, physical restart or full row19 claim)",
                1,
            ),
            "forward-capacity": (
                "FORWARD_CAPACITY_RESULT 1 passed; 0 failed; 0 ignored (shared retained Native oracle, normal Worker enrollment/Fleet/Register; actual sender Registry-dispatch barrier, reserved sixteenth slot, exact seventeenth Overloaded and terminal capacity reuse; no process restart or full row19 claim)",
                1,
            ),
            "forward-deadline": (
                "FORWARD_DEADLINE_RESULT 1 passed; 0 failed; 0 ignored (shared retained Native oracle, normal Worker enrollment/Fleet/Register; actual owning sender clock/alarm, original Unanswered and late-reply isolation from fresh identity; no process restart or full row19 claim)",
                1,
            ),
            "forward-reopen": (
                "FORWARD_REOPEN_RESULT 1 passed; 0 failed; 0 ignored (two actual owned Worker process lifetimes; stopped persistence known-correlation presence/opaque absence, sixteen retained slots before original deadline and sixteen fresh deliveries after expiry; public capacity observation, no private SQL read, DO eviction/hibernation or real Router writer claim)",
                2,
            ),
        }
        for mode, (marker, minimum_reaps) in contracts.items():
            with self.subTest(mode=mode):
                command = workerd_command(
                    mode, "/node", Path("/mf"), Path("/state"), 8957
                )
                self.assertEqual(
                    command[-7:],
                    [
                        "session_scope_workerd",
                        "--",
                        "/node",
                        "/mf",
                        "/state",
                        "8957",
                        mode,
                    ],
                )
                reap = "WORKERD owned process stopped and reaped\n"
                observed = check_workerd_output(
                    mode, marker + "\n" + reap * minimum_reaps
                )
                self.assertEqual(observed.reaped_processes, minimum_reaps)
                self.assertEqual(observed.completion_marker_occurrences, 1)
                for output in (
                    marker + "\n" + reap * (minimum_reaps - 1),
                    marker.replace("0 failed", "1 failed")
                    + "\n"
                    + reap * minimum_reaps,
                    "wrong result\n" + reap * minimum_reaps,
                ):
                    with self.subTest(output=output), self.assertRaises(ValueError):
                        _ = check_workerd_output(mode, output)

    def test_normal_session_and_holder_modes_use_the_normal_entry(self) -> None:
        for mode in ("session-capabilities", "session-failures", "holder-lifecycle"):
            with self.subTest(mode=mode):
                command = workerd_command(
                    mode, "/node", Path("/mf"), Path("/state"), 8957
                )
                self.assertEqual(
                    command[-7:],
                    [
                        "session_scope_workerd",
                        "--",
                        "/node",
                        "/mf",
                        "/state",
                        "8957",
                        mode,
                    ],
                )
        with self.assertRaises(ValueError):
            _ = workerd_scenarios(("holder-schema-overlap",))

    def test_normal_session_and_holder_results_require_full_groups_and_reap(
        self,
    ) -> None:
        markers = {
            "session-capabilities": "SESSION_CAPABILITY_RESULT 2 passed; 0 failed; 0 ignored (normal enrollment and session, real workers-rs attachment roundtrip and original origin, actual same-Connection-object alarm; old SqlTx pointer identity retired)",
            "session-failures": "SESSION_FAILURE_RESULT 5 passed; 0 failed; 0 ignored (normal enrolled sessions, targeted real postcommit publication failure, same-socket Register refusal, callback/SQL rollback and real retry, exact public journal/ordered socket barrier)",
            "holder-lifecycle": "HOLDER_LIFECYCLE_RESULT 5 passed; 0 failed; 0 ignored (normal enrolled sessions and real Registry holder journal; readiness/expiry/replacement, exact frame/connection, close/alarm/late answer, held schema publication; fifth group combines existing hub primitive after normal admission with changed-SessionId wire close)",
        }
        reap = "WORKERD owned process stopped and reaped\n"
        for mode, marker in markers.items():
            with self.subTest(mode=mode):
                observation = check_workerd_output(mode, marker + "\n" + reap)
                self.assertEqual(observation.example, "session_scope_workerd")
                self.assertEqual(observation.reaped_processes, 1)
                self.assertEqual(observation.completion_marker_occurrences, 1)
                other = next(value for name, value in markers.items() if name != mode)
                for output in (
                    marker + "\n",
                    other + "\n" + reap,
                    marker.replace("0 failed", "1 failed") + "\n" + reap,
                    "HOLDER_SCHEMA_OVERLAP_RESULT 1 passed; 0 failed; 0 ignored\n"
                    + reap,
                ):
                    with self.subTest(output=output), self.assertRaises(ValueError):
                        _ = check_workerd_output(mode, output)

    def test_owner_admin_modes_use_the_normal_session_entry(self) -> None:
        for mode in ("owner-registry", "owner-retirement"):
            with self.subTest(mode=mode):
                command = workerd_command(
                    mode, "/node", Path("/mf"), Path("/state"), 8957
                )
                self.assertEqual(
                    command[-7:],
                    [
                        "session_scope_workerd",
                        "--",
                        "/node",
                        "/mf",
                        "/state",
                        "8957",
                        mode,
                    ],
                )

    def test_owner_admin_modes_require_their_complete_group_and_owned_reap(
        self,
    ) -> None:
        markers = {
            "owner-registry": "OWNER_REGISTRY_RESULT 3 passed; 0 failed; 0 ignored (normal Fleet creation, Page/Migrate, holder Release/Copy fence/target renewal, unchanged preview/tamper/takeover/replay and held-lease refusal; no successful retirement or physical restart claim)",
            "owner-retirement": "OWNER_RETIREMENT_RESULT 1 passed; 0 failed; 0 ignored (conformance import/unheld/seed setup on actual composed stores, normal HTTP retirement, before-row/read-after-retired, exact Fleet acknowledgement and stable receipt/replay/Migrate; D1b HTTP import, physical file row count and process restart unobserved)",
        }
        reap = "WORKERD owned process stopped and reaped\n"
        for mode, marker in markers.items():
            with self.subTest(mode=mode):
                observation = check_workerd_output(mode, marker + "\n" + reap)
                self.assertEqual(observation.example, "session_scope_workerd")
                self.assertEqual(observation.reaped_processes, 1)
                self.assertEqual(observation.completion_marker_occurrences, 1)
                other = next(value for name, value in markers.items() if name != mode)
                for output in (
                    marker + "\n",
                    other + "\n" + reap,
                    marker.replace("0 failed", "1 failed") + "\n" + reap,
                ):
                    with self.subTest(output=output), self.assertRaises(ValueError):
                        _ = check_workerd_output(mode, output)

    def test_copy_owner_uses_the_normal_session_entry(self) -> None:
        command = workerd_command(
            "copy-owner", "/node", Path("/mf"), Path("/state"), 8957
        )
        self.assertEqual(
            command[-7:],
            [
                "session_scope_workerd",
                "--",
                "/node",
                "/mf",
                "/state",
                "8957",
                "copy-owner",
            ],
        )

    def test_copy_owner_requires_the_complete_group_and_owned_reap(self) -> None:
        marker = "COPY_OWNER_RESULT 1 passed; 0 failed; 0 ignored (owner HTTP Health before initialization, target bytes, empty Page, real Repair, machine refusal and minimal retired receipt; fixture Copy creation/retirement, no migration-fault, populated-page or full Registry admin claim)"
        reap = "WORKERD owned process stopped and reaped\n"
        observed = check_workerd_output("copy-owner", marker + "\n" + reap)
        self.assertEqual(observed.example, "session_scope_workerd")
        self.assertEqual(observed.reaped_processes, 1)
        self.assertEqual(observed.completion_marker_occurrences, 1)
        for output in (
            marker + "\n",
            "COPY_OWNER_CASE health_page_repair_authority_and_retired_receipt passed\n"
            + reap,
            marker.replace("1 passed; 0 failed", "0 passed; 1 failed") + "\n" + reap,
        ):
            with self.subTest(output=output), self.assertRaises(ValueError):
                _ = check_workerd_output("copy-owner", output)

    def test_fleet_notices_use_the_normal_session_entry(self) -> None:
        command = workerd_command(
            "fleet-notices", "/node", Path("/mf"), Path("/state"), 8957
        )
        self.assertEqual(
            command[-7:],
            [
                "session_scope_workerd",
                "--",
                "/node",
                "/mf",
                "/state",
                "8957",
                "fleet-notices",
            ],
        )

    def test_fleet_notices_require_the_complete_group_and_owned_reap(self) -> None:
        marker = "FLEET_NOTICES_RESULT 1 passed; 0 failed; 0 ignored (three normal admitted sessions; two reader installations with two exact numbered hints each, Fleet mutation and replay, own alarm, real Delivery and positioned notice read; no physical restart claim)"
        reap = "WORKERD owned process stopped and reaped"
        observed = check_workerd_output("fleet-notices", marker + "\n" + reap)
        self.assertEqual(
            (observed.completion_marker_occurrences, observed.reaped_processes), (1, 1)
        )
        for output in (
            marker,
            reap,
            "FLEET_NOTICE_CASE committed_feed_advancement_delivers_once passed\n"
            + reap,
            marker.replace("1 passed; 0 failed", "0 passed; 1 failed") + "\n" + reap,
        ):
            with self.subTest(output=output), self.assertRaises(ValueError):
                _ = check_workerd_output("fleet-notices", output)

    def test_composed_entry_has_its_mode_before_runtime_arguments(self) -> None:
        self.assertEqual(
            workerd_command(
                "composed-numeric", "/node", Path("/mf"), Path("/state"), 8957
            ),
            [
                "cargo",
                "run",
                "--locked",
                "-p",
                "collaboration-central-tokio",
                "--features",
                "conformance",
                "--example",
                "composed_scope_workerd",
                "--",
                "composed-numeric",
                "/node",
                "/mf",
                "/state",
                "8957",
            ],
        )

    def test_owner_routes_use_retained_examples_without_a_synthetic_mode(self) -> None:
        for mode, example in (
            ("fleet-receipt", "step_scope_workerd"),
            ("bootstrap", "bootstrap_scope_workerd"),
        ):
            with self.subTest(mode=mode):
                self.assertEqual(
                    workerd_command(mode, "/node", Path("/mf"), Path("/state"), 8957),
                    [
                        "cargo",
                        "run",
                        "--locked",
                        "-p",
                        "collaboration-central-tokio",
                        "--features",
                        "conformance",
                        "--example",
                        example,
                        "--",
                        "/node",
                        "/mf",
                        "/state",
                        "8957",
                    ],
                )

    def test_normal_session_entry_has_explicit_mode_last(self) -> None:
        self.assertEqual(
            workerd_command(
                "access-carryover", "/node", Path("/mf"), Path("/state"), 8957
            )[-7:],
            [
                "session_scope_workerd",
                "--",
                "/node",
                "/mf",
                "/state",
                "8957",
                "access-carryover",
            ],
        )

    def test_conformance_build_stays_dev_with_fault_controls(self) -> None:
        self.assertEqual(
            worker_build_command(Path("/worker-build")),
            [
                "/worker-build",
                "crates/collaboration-central-cloudflare",
                "--dev",
                "--no-opt",
                "--",
                "--locked",
                "--features",
                "conformance-faults",
            ],
        )

    def test_sender_modes_use_the_normal_session_entry_with_mode_last(self) -> None:
        for mode in (
            "raw-submit-refusal",
            "sender-settlement",
            "sender-retirement",
        ):
            with self.subTest(mode=mode):
                command = workerd_command(
                    mode, "/node", Path("/mf"), Path("/state"), 8957
                )
                self.assertEqual(
                    command[-7:],
                    [
                        "session_scope_workerd",
                        "--",
                        "/node",
                        "/mf",
                        "/state",
                        "8957",
                        mode,
                    ],
                )

    def test_sender_results_require_their_own_marker_and_process_settlement(
        self,
    ) -> None:
        markers = {
            "raw-submit-refusal": "RAW_SUBMIT_RESULT 1 passed; 0 failed; 0 ignored (normal admitted sockets, permanent raw Submit refusal, same-socket survival and empty recipient custody)",
            "sender-settlement": "SENDER_SETTLEMENT_RESULT 1 passed; 0 failed; 0 ignored (normal Query and Settle through Registry to recipient Mailbox, known custody replay, unknown tombstone, late Submit refusal and custody readback)",
            "sender-retirement": "SENDER_RETIREMENT_RESULT 1 passed; 0 failed; 0 ignored (verified sender fencing, actual recipient Mailbox, same-CallId receipt replay and one control effect)",
        }
        reap = "WORKERD owned process stopped and reaped\n"
        for mode, marker in markers.items():
            with self.subTest(mode=mode):
                observed = check_workerd_output(mode, marker + "\n" + reap)
                self.assertEqual(observed.example, "session_scope_workerd")
                self.assertEqual(observed.reaped_processes, 1)
                self.assertEqual(observed.completion_marker_occurrences, 1)
                other_marker = next(
                    value for other, value in markers.items() if other != mode
                )
                for output in (
                    marker + "\n",
                    other_marker + "\n" + reap,
                    marker.replace("1 passed; 0 failed", "0 passed; 1 failed")
                    + "\n"
                    + reap,
                ):
                    with self.subTest(output=output), self.assertRaises(ValueError):
                        _ = check_workerd_output(mode, output)

    def test_defaults_are_current_and_retired_obligations_remain_visible(self) -> None:
        self.assertEqual(
            DEFAULT_WORKERD_MODES, ("composed-transactions", "composed-numeric")
        )
        self.assertEqual(
            [mode for mode, _ in workerd_scenarios(DEFAULT_WORKERD_MODES)],
            list(DEFAULT_WORKERD_MODES),
        )
        retirements = retired_scenarios()
        for key in (
            "launcher:workerd_lifecycle",
            "launcher:native_smoke",
            "native:local_storage",
            "native:local_conformance",
            "workerd:default",
            "workerd:holder-controls",
            "workerd:panic-inside-transaction",
        ):
            with self.subTest(key=key):
                self.assertRegex(retirements[key], "508|513")

    def test_http_body_modes_use_one_explicit_normal_session_scenario(self) -> None:
        for mode in ("http-body-keepalive", "http-body-over-limit"):
            with self.subTest(mode=mode):
                command = workerd_command(
                    mode, "/node", Path("/mf"), Path("/state"), 8957
                )
                self.assertEqual(
                    command[-7:],
                    [
                        "session_scope_workerd",
                        "--",
                        "/node",
                        "/mf",
                        "/state",
                        "8957",
                        mode,
                    ],
                )

    def test_http_body_modes_reject_other_results_or_missing_settlement(self) -> None:
        markers = {
            "http-body-keepalive": "HTTP_BODY_KEEPALIVE_RESULT 1 passed; 0 failed; 0 ignored (actual production enroll route, exactly2MiB unauthorized body with withheld suffix, two sends on one physical TCP connection, no unauthorized peer reservation)",
            "http-body-over-limit": "HTTP_BODY_OVER_LIMIT_RESULT 1 passed; 0 failed; 0 ignored (eight actual HTTP route/auth/method variants,2MiB+1 body,413 and physical close for each; one grouped scenario)",
        }
        reap = "WORKERD owned process stopped and reaped\n"
        for mode, marker in markers.items():
            with self.subTest(mode=mode):
                observation = check_workerd_output(mode, marker + "\n" + reap)
                self.assertEqual(observation.reaped_processes, 1)
                self.assertEqual(observation.completion_marker_occurrences, 1)
                other = next(value for name, value in markers.items() if name != mode)
                for output in (marker + "\n", other + "\n" + reap):
                    with self.subTest(output=output), self.assertRaises(ValueError):
                        _ = check_workerd_output(mode, output)

    def test_unknown_duplicate_or_empty_modes_do_not_run(self) -> None:
        for modes in ((), ("--workspace",), ("composed-numeric", "composed-numeric")):
            with self.subTest(modes=modes), self.assertRaises(ValueError):
                _ = workerd_scenarios(modes)

    def test_creation_gates_use_the_normal_session_entry(self) -> None:
        command = workerd_command(
            "creation-gates", "/node", Path("/mf"), Path("/state"), 8957
        )
        self.assertEqual(
            command[-7:],
            [
                "session_scope_workerd",
                "--",
                "/node",
                "/mf",
                "/state",
                "8957",
                "creation-gates",
            ],
        )

    def test_creation_gates_require_the_complete_group_and_owned_reap(self) -> None:
        marker = "CREATION_GATES_RESULT 1 passed; 0 failed; 0 ignored (normal enrolled sessions and real Fleet/Registry/Copy; four creation gates, same-attempt retry/replay, first piece and held Claim refusal; no imported positive Claim or physical restart claim)"
        reap = "WORKERD owned process stopped and reaped\n"
        observation = check_workerd_output("creation-gates", marker + "\n" + reap)
        self.assertEqual(observation.reaped_processes, 1)
        self.assertEqual(observation.completion_marker_occurrences, 1)
        for output in (
            marker + "\n",
            "CREATION_GATE_CASE accepted_carrier_registered_and_replayed passed\n"
            + reap,
            marker.replace("1 passed; 0 failed", "0 passed; 1 failed") + "\n" + reap,
        ):
            with self.subTest(output=output), self.assertRaises(ValueError):
                _ = check_workerd_output("creation-gates", output)

    def test_delivery_config_uses_the_explicit_relay_after_the_normal_mode(
        self,
    ) -> None:
        for relay in (
            "019f0000-0000-7000-8000-000000005241",
            "019f0000-0000-7000-8000-000000005242",
        ):
            with self.subTest(relay=relay):
                command = workerd_command(
                    "delivery-config",
                    "/node",
                    Path("/mf"),
                    Path("/state"),
                    8957,
                    delivery_relay_peer=relay,
                )
                self.assertEqual(
                    command[command.index("--example") + 1], "session_scope_workerd"
                )
                self.assertEqual(
                    command[command.index("--") + 1 :],
                    ["/node", "/mf", "/state", "8957", "delivery-config", relay],
                )

    def test_delivery_config_refuses_missing_or_non_v7_configuration(self) -> None:
        for relay in (None, "", "not-a-peer", "550e8400-e29b-41d4-a716-446655440524"):
            with (
                self.subTest(relay=relay),
                self.assertRaisesRegex(ValueError, "explicit UUIDv7"),
            ):
                _ = workerd_command(
                    "delivery-config",
                    "/node",
                    Path("/mf"),
                    Path("/state"),
                    8957,
                    delivery_relay_peer=relay,
                )

    def test_delivery_config_requires_the_complete_group_and_owned_reap(self) -> None:
        marker = "DELIVERY_CONFIG_RESULT 1 passed; 0 failed; 0 ignored (explicit relay distinct from receiver, exact first/replay/update/readmission answers through real Registry and Mailbox; no Router delivery-call closure claim)"
        reap = "WORKERD owned process stopped and reaped"
        observed = check_workerd_output("delivery-config", marker + "\n" + reap)
        self.assertEqual(
            (observed.completion_marker_occurrences, observed.reaped_processes), (1, 1)
        )
        for output in (
            marker,
            reap,
            "DELIVERY_CONFIG_CASE first configured_relay passed\n" + reap,
        ):
            with self.subTest(output=output), self.assertRaises(ValueError):
                _ = check_workerd_output("delivery-config", output)

    def test_completion_requires_the_current_case_and_physical_restart_settlement(
        self,
    ) -> None:
        marker = "COMPOSED_NUMERIC_CASE exact_i64_cells_real_identity_snapshot_and_receipt_replay passed"
        reap = "WORKERD owned process stopped and reaped\n"
        observed = check_workerd_output("composed-numeric", reap + marker + "\n" + reap)
        self.assertEqual(observed.reaped_processes, 2)
        self.assertEqual(observed.completion_marker_occurrences, 1)
        repeated = check_workerd_output(
            "composed-numeric", reap + marker + "\n" + marker + "\n" + reap
        )
        self.assertEqual(repeated.completion_marker_occurrences, 2)
        for output in (
            marker + "\n" + reap,
            reap * 2
            + "WORKERD local: 8 shared groups and 7 physical-restart groups passed\n",
        ):
            with self.subTest(output=output), self.assertRaises(ValueError):
                _ = check_workerd_output("composed-numeric", output)

    def test_transaction_repeats_are_occurrences_not_distinct_cases(self) -> None:
        marker = "COMPOSED_TRANSACTION_CASE sql_and_full_rollbacks_fence_300_row_snapshot_prefixes_and_reopen passed\n"
        output = (
            marker * 2
            + "WORKERD owned process stopped and reaped\n"
            + marker
            + "WORKERD owned process stopped and reaped\n"
        )
        observed = check_workerd_output("composed-transactions", output)
        self.assertEqual(observed.completion_marker_occurrences, 3)
        self.assertEqual(observed.reaped_processes, 2)


class TestSourceIdentity(unittest.TestCase):
    def test_git_and_archive_refuse_drift(self) -> None:
        with tempfile.TemporaryDirectory() as root:
            repository = Path(root) / "source"
            repository.mkdir()
            manifest = repository / "Cargo.toml"
            _ = manifest.write_text("[workspace]\n")
            for arguments in (
                ["init", "-q"],
                ["add", "Cargo.toml"],
                [
                    "-c",
                    "user.name=Apparatus Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "--no-gpg-sign",
                    "-qm",
                    "fixture",
                ],
            ):
                _ = subprocess.run(
                    ["git", *arguments], cwd=repository, check=True, capture_output=True
                )
            head = subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=repository, text=True
            ).strip()
            check_source(repository, head)
            with self.assertRaisesRegex(ValueError, "source target changed"):
                check_source(repository, "0" * 40)
            archive = Path(root) / "source.tar"
            with archive.open("wb") as output:
                _ = subprocess.run(
                    ["git", "archive", "HEAD"],
                    cwd=repository,
                    stdout=output,
                    check=True,
                )
            snapshot = Path(root) / "snapshot"
            snapshot.mkdir()
            with tarfile.open(archive) as contents:
                contents.extractall(snapshot, filter="data")
            check_source(snapshot, head, archive)
            extra = snapshot / "crates/new.rs"
            extra.parent.mkdir()
            _ = extra.write_text("// unexpected source\n")
            with self.assertRaisesRegex(ValueError, "extra archived source"):
                check_source(snapshot, head, archive)
            extra.unlink()
            _ = manifest.write_text("[workspace]\nmembers=[]\n")
            with self.assertRaisesRegex(ValueError, "source target changed"):
                check_source(repository, head)
            _ = (snapshot / "Cargo.toml").write_text("changed\n")
            with self.assertRaisesRegex(ValueError, "archived source changed"):
                check_source(snapshot, head, archive)

    def test_untracked_product_source_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as root:
            repository = Path(root)
            _ = subprocess.run(["git", "init", "-q"], cwd=repository, check=True)
            _ = (repository / "Cargo.toml").write_text("[workspace]\n")
            _ = subprocess.run(["git", "add", "Cargo.toml"], cwd=repository, check=True)
            _ = subprocess.run(
                [
                    "git",
                    "-c",
                    "user.name=Apparatus Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "--no-gpg-sign",
                    "-qm",
                    "fixture",
                ],
                cwd=repository,
                check=True,
            )
            head = subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=repository, text=True
            ).strip()
            (repository / "crates").mkdir()
            _ = (repository / "crates/unwired.rs").write_text("// unwired draft\n")
            with self.assertRaisesRegex(ValueError, "product changes"):
                check_source(repository, head)


if __name__ == "__main__":
    _ = unittest.main()
