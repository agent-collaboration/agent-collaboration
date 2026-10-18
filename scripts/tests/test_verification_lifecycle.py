"""Controlled harmless-process verification apparatus regressions, never app proof."""

import datetime as dt
import hashlib
import os
import subprocess
import sys
import tempfile
import unittest
from collections import abc
from contextlib import contextmanager
from pathlib import Path

from scripts.tests.verification_controller_fixture import (
    LifecycleFixture,
    lookup_response,
)
from scripts.verification import (
    native_correlation,
    owned_runtime,
    process_inventory,
    process_ownership,
    readonly_codex_home,
    runtime_qualification,
)
from scripts.verification.native_correlation import read_carrier_correlation
from scripts.verification.process_ownership import (
    lookup_process_identity,
    same_identity,
)
from scripts.verification.typed_observations import (
    IdentityRecord,
    NativeExpectation,
    ProcessIdentity,
    observed_object,
    read_observed_json,
)


class GlobalClaudeQualificationTests(unittest.TestCase):
    @staticmethod
    def executable(directory: Path) -> Path:
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / "claude"
        _ = path.write_bytes(b"owned qualification fixture; never executed")
        path.chmod(0o700)
        return path

    def test_canonical_global_binding_records_exact_file_hash(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            executable = self.executable(root / "global-bin")
            planned_alias = root / "planned-claude"
            planned_alias.symlink_to(executable)
            binding = runtime_qualification.qualify_global_claude(
                planned_alias, str(executable.parent)
            )
            self.assertEqual(binding.canonicalPath, executable.resolve())
            self.assertEqual(
                binding.sha256,
                hashlib.sha256(
                    b"owned qualification fixture; never executed"
                ).hexdigest(),
            )

    def test_relative_package_and_canonical_package_alias_entries_are_skipped(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            package = self.executable(root / "project/node_modules/.bin")
            alias = root / "package-alias"
            alias.symlink_to(package.parent, target_is_directory=True)
            global_file = self.executable(root / "global-bin")
            search = os.pathsep.join(
                [
                    "relative/bin",
                    str(package.parent),
                    str(alias),
                    str(global_file.parent),
                ]
            )
            self.assertEqual(
                runtime_qualification.resolve_global_claude(search),
                global_file.resolve(),
            )

    def test_transient_and_sdk_target_aliases_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for index, relative in enumerate(
                [
                    "_npx/fixture",
                    "dlx/fixture",
                    "node_modules/claude-agent-sdk-darwin-arm64",
                    "node_modules/@anthropic-ai+claude-agent-sdk-fixture",
                ]
            ):
                with self.subTest(relative=relative):
                    target = self.executable(root / relative)
                    directory = root / f"alias-{index}"
                    directory.mkdir()
                    (directory / "claude").symlink_to(target)
                    with self.assertRaisesRegex(
                        RuntimeError, "unavailable on Host PATH"
                    ):
                        _ = runtime_qualification.resolve_global_claude(str(directory))

    def test_normal_global_claude_code_package_target_remains_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            target = self.executable(
                root / "global/lib/node_modules/@anthropic-ai/claude-code/bin"
            )
            directory = root / "global/bin"
            directory.mkdir()
            (directory / "claude").symlink_to(target)
            self.assertEqual(
                runtime_qualification.resolve_global_claude(str(directory)),
                target.resolve(),
            )

    def test_missing_empty_and_relative_path_do_not_fall_back(self) -> None:
        for search in [None, "", "relative/bin"]:
            with (
                self.subTest(search=search),
                self.assertRaisesRegex(RuntimeError, "unavailable on Host PATH"),
            ):
                _ = runtime_qualification.resolve_global_claude(search)

    def test_nonexecutable_directory_and_broken_candidates_are_skipped(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            nonexecutable = self.executable(root / "nonexecutable")
            nonexecutable.chmod(0o600)
            directory = root / "directory"
            (directory / "claude").mkdir(parents=True)
            broken = root / "broken"
            broken.mkdir()
            (broken / "claude").symlink_to(root / "missing")
            for candidate in [nonexecutable.parent, directory, broken]:
                with (
                    self.subTest(candidate=candidate),
                    self.assertRaisesRegex(RuntimeError, "unavailable on Host PATH"),
                ):
                    _ = runtime_qualification.resolve_global_claude(str(candidate))


@contextmanager
def harmless_child() -> abc.Generator[
    tuple[subprocess.Popen[bytes], ProcessIdentity], None, None
]:
    _ = lookup_process_identity(os.getpid())
    child = subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(60)"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    recorded = lookup_process_identity(child.pid)
    assert recorded is not None
    identity = ProcessIdentity.model_validate({**recorded, "parent": os.getpid()})
    try:
        yield child, identity
    finally:
        if child.poll() is None:
            if not same_identity(lookup_process_identity(child.pid), recorded):
                raise RuntimeError("test child identity changed; no signal")
            child.terminate()
        _ = child.wait(timeout=5)


class OwnershipSnapshotTests(unittest.TestCase):
    def test_empty_stdout_with_denial_stderr_is_not_absence(self) -> None:
        result = subprocess.CompletedProcess(
            ["/bin/ps"], 1, stdout="", stderr="Operation not permitted"
        )
        with lookup_response(result), self.assertRaises(subprocess.CalledProcessError):
            _ = process_ownership.lookup_process_identity(12)

    def test_clean_absence_and_malformed_rows_keep_distinct_results(self) -> None:
        with lookup_response(
            subprocess.CompletedProcess(["/bin/ps"], 1, stdout="", stderr="")
        ):
            self.assertIsNone(process_ownership.lookup_process_identity(12))
        with (
            lookup_response(
                subprocess.CompletedProcess(
                    ["/bin/ps"], 0, stdout="malformed", stderr=""
                )
            ),
            self.assertRaises(ValueError),
        ):
            _ = process_ownership.lookup_process_identity(12)

    @contextmanager
    def snapshot(
        self, fresh: abc.Callable[[int], IdentityRecord | None]
    ) -> abc.Generator[dict[int, ProcessIdentity], None, None]:
        root = ProcessIdentity(
            pid=10,
            parent=1,
            uid=os.getuid(),
            group=10,
            birth="root",
            executable="fixture-root",
        )
        child = ProcessIdentity(
            pid=11,
            parent=10,
            uid=os.getuid(),
            group=10,
            birth="child",
            executable="fixture-child",
        )
        foreign = ProcessIdentity(
            pid=12,
            parent=10,
            uid=0 if os.getuid() else 1,
            group=10,
            birth="foreign",
            executable="<defunct>",
        )
        observed = {10: root, 11: child, 12: foreign}
        known = {10: root}
        original_table = owned_runtime.process_table
        original_lookup = process_ownership.lookup_process_identity

        def table() -> dict[int, ProcessIdentity]:
            return observed

        try:
            owned_runtime.process_table = table
            process_ownership.lookup_process_identity = fresh
            yield known
        finally:
            owned_runtime.process_table = original_table
            process_ownership.lookup_process_identity = original_lookup

    def test_proven_absent_foreign_snapshot_is_not_acquired(self) -> None:
        checked: list[int] = []

        def absent(pid: int) -> IdentityRecord | None:
            checked.append(pid)
            return None

        with self.snapshot(absent) as known:
            owned_runtime.observe_owned(10, known)
            self.assertEqual(set(known), {10, 11})
            self.assertEqual(checked, [12])

    def test_any_still_present_mismatch_keeps_uid_rejection(self) -> None:
        for current_uid in [0 if os.getuid() else 1, os.getuid()]:
            with self.subTest(current_uid=current_uid):

                def present(
                    pid: int, observed_uid: int = current_uid
                ) -> IdentityRecord | None:
                    return {
                        "pid": pid,
                        "uid": observed_uid,
                        "group": 10,
                        "birth": "foreign",
                        "executable": "<defunct>",
                    }

                with (
                    self.snapshot(present) as known,
                    self.assertRaisesRegex(RuntimeError, "owned process UID changed"),
                ):
                    owned_runtime.observe_owned(10, known)

    def test_denied_or_malformed_fresh_lookup_fails_closed(self) -> None:
        for error_type in [PermissionError, ValueError]:
            with self.subTest(error_type=error_type):

                def unavailable(
                    _pid: int, failure: type[Exception] = error_type
                ) -> IdentityRecord | None:
                    raise failure("controlled fresh lookup failure")

                with (
                    self.snapshot(unavailable) as known,
                    self.assertRaises((RuntimeError, PermissionError, ValueError)),
                ):
                    owned_runtime.observe_owned(10, known)


class ControlledLifecycleTests(unittest.TestCase):
    def test_lifecycle_never_opens_saved_configuration_for_writing(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "stop")
            profile_before = fixture.plan.profile.read_bytes()
            config_before = fixture.codex_config.read_bytes()

            self.assertIsNone(fixture.drive())

            self.assertEqual(fixture.configuration_writes, [])
            self.assertEqual(fixture.plan.profile.read_bytes(), profile_before)
            self.assertEqual(fixture.codex_config.read_bytes(), config_before)

    def test_profile_edit_before_host_survives_and_reports_conflict(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "profile-before-host")
            error = fixture.drive()
            self.assertEqual(fixture.plan.profile.read_bytes(), fixture.owner_profile)
            self.assertIsInstance(error, RuntimeError)
            self.assertIn("saved profile changed", str(error))
            self.assertEqual(fixture.phases, [])
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertIsNotNone(settlement["failure"])
            self.assertFalse(settlement["codexHomeUnchangedExactly"])

    def test_expiry_inside_consumer_check_prevents_host_launch(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "inspection-expired")
            error = fixture.drive()
            self.assertEqual(fixture.phases, [])
            self.assertIsInstance(error, TimeoutError)
            self.assertIn("allocation deadline expired at launch", str(error))
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertIsNotNone(settlement["failure"])
            self.assertTrue(settlement["codexHomeUnchangedExactly"])

    def test_later_profile_edit_is_reported_without_restoration(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "profile-after-host")
            _ = fixture.drive()
            self.assertEqual(fixture.plan.profile.read_bytes(), fixture.owner_profile)
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertEqual(
                observed_object(settlement["failure"])["class"],
                "ConfigurationChanged",
            )
            self.assertFalse(settlement["codexHomeUnchangedExactly"])

    def test_normal_explicit_stop_preserves_both_configuration_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "stop")
            original = fixture.plan.profile.read_bytes()
            self.assertIsNone(fixture.drive())
            self.assertEqual(fixture.plan.profile.read_bytes(), original)
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertIsNone(settlement["failure"])
            self.assertTrue(settlement["codexHomeUnchangedExactly"])
            state = observed_object(settlement["configuration"])
            self.assertEqual(state["before"], state["after"])
            after = observed_object(state["after"])
            self.assertEqual(
                observed_object(after["profile"])["sha256"],
                hashlib.sha256(original).hexdigest(),
            )
            self.assertEqual(
                observed_object(after["config"])["sha256"],
                hashlib.sha256(fixture.codex_config.read_bytes()).hexdigest(),
            )
            self.assertEqual(fixture.configuration_writes, [])
            for stage in ["before-host-A", "host-A-ready", "host-A-stopped"]:
                self.assertTrue(
                    (fixture.plan.evidence / f"configuration-{stage}.json").is_file()
                )

    def test_config_edit_before_host_is_preserved_and_prevents_launch(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "config-before-host")

            self.assertIsInstance(fixture.drive(), RuntimeError)

            self.assertEqual(fixture.codex_config.read_bytes(), fixture.owner_config)
            self.assertEqual(fixture.phases, [])
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertFalse(settlement["codexHomeUnchangedExactly"])
            self.assertEqual(len(fixture.configuration_writes), 1)

    def test_config_edit_after_host_is_reported_and_never_restored(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "config-after-host")

            self.assertIsInstance(fixture.drive(), RuntimeError)

            self.assertEqual(fixture.codex_config.read_bytes(), fixture.owner_config)
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertFalse(settlement["codexHomeUnchangedExactly"])
            self.assertEqual(len(fixture.configuration_writes), 1)

    def test_reopen_cannot_launch_host_b_after_cleanup_expires_allocation(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "reopen-expired")
            self.assertIsInstance(fixture.drive(), TimeoutError)
            self.assertEqual(fixture.phases, ["A"])
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertIsNotNone(settlement["failure"])

    def test_deadline_cannot_launch_unrequested_host_b_or_record_success(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "deadline")
            error = fixture.drive()
            self.assertEqual(fixture.phases, ["A"])
            self.assertIsInstance(error, TimeoutError)
            self.assertIn("Host A allocation deadline expired", str(error))
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertIsNotNone(settlement["failure"])

    def test_clean_unexpected_exit_cannot_launch_host_b_or_record_success(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "early-exit")
            error = fixture.drive()
            self.assertEqual(fixture.phases, ["A"])
            self.assertIsInstance(error, RuntimeError)
            self.assertIn(
                "Host A exited before an explicit lifecycle directive", str(error)
            )
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertIsNotNone(settlement["failure"])

    def test_claude_path_mismatch_rejects_before_any_requester(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "stop", mismatch=True)
            error = fixture.drive()
            self.assertEqual(fixture.launched, [])
            self.assertIsInstance(error, RuntimeError)
            self.assertIn(
                "planned Claude executable differs from qualified Host PATH selection",
                str(error),
            )

    def test_explicit_reopen_then_stop_is_the_only_two_host_journey(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "reopen")
            self.assertIsNone(fixture.drive())
            self.assertEqual(fixture.phases, ["A", "B"])
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertIsNone(settlement["failure"])
            binding = observed_object(
                read_observed_json(
                    (fixture.plan.evidence / "driver-binding.json").read_text()
                )["claudeGlobalExecutable"]
            )
            self.assertEqual(
                binding["canonicalPath"], str(fixture.plan.claude_executable.resolve())
            )
            self.assertEqual(
                binding["sha256"],
                hashlib.sha256(b"owned executable fixture; never executed").hexdigest(),
            )

    def test_native_metadata_failure_still_closes_and_reaps_owned_pipe(self) -> None:
        _ = lookup_process_identity(os.getpid())
        child = subprocess.Popen(
            [sys.executable, "-c", "import sys; sys.stdin.buffer.read()"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
            bufsize=0,
        )
        original = native_correlation.lookup_process_identity

        def denied(_pid: int) -> IdentityRecord | None:
            raise PermissionError("controlled metadata failure")

        try:
            native_correlation.lookup_process_identity = denied
            with self.assertRaises(PermissionError):
                _ = read_carrier_correlation(
                    child,
                    NativeExpectation(
                        thread_id="fixture", turn_id="fixture", push_id="fixture"
                    ),
                )
        finally:
            native_correlation.lookup_process_identity = original
        self.assertEqual(child.returncode, 0)

    def test_inventory_timeout_records_failure_and_settles_owned_children(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "inventory-timeout")
            self.assertIsInstance(fixture.drive(), subprocess.TimeoutExpired)
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertIsNotNone(settlement["failure"])
            self.assertEqual(fixture.phases, ["A"])
            for child in fixture.children:
                self.assertIsNotNone(child.poll())
            hosts = settlement["hosts"]
            assert isinstance(hosts, list)
            self.assertEqual(observed_object(hosts[0])["remaining"], {})

    def test_recorded_harmless_child_is_released(self) -> None:
        with harmless_child() as (child, identity):
            result = owned_runtime.stop_owned(child, {child.pid: identity})
            self.assertEqual(result.remaining, {})
            self.assertEqual(result.errors, [])
            self.assertIsNotNone(child.poll())

    def test_denied_observation_retains_unverified_inventory_without_signal(
        self,
    ) -> None:
        with harmless_child() as (child, identity):
            original = owned_runtime.process_table

            def denied() -> dict[int, ProcessIdentity]:
                raise PermissionError("controlled unavailable observation")

            try:
                owned_runtime.process_table = denied
                result = owned_runtime.stop_owned(child, {child.pid: identity})
            finally:
                owned_runtime.process_table = original
            self.assertIn(child.pid, result.remaining)
            self.assertEqual(result.signals, [])
            self.assertTrue(result.errors)
            self.assertIsNone(child.poll())

    def test_skipped_inventory_poll_never_defaults_to_success(self) -> None:
        with (
            harmless_child() as (child, identity),
            tempfile.TemporaryDirectory() as folder,
        ):
            evidence = Path(folder)
            # Only the inventory action is exercised; this plan never launches an app.
            plan = owned_runtime.RuntimePlan(
                repository=evidence,
                source_head="0" * 40,
                qualification=evidence / "unused",
                router_root=evidence / "unused-root",
                evidence=evidence,
                port=43128,
                expected_version="test-only",
                profile=evidence / "unused-profile",
                codex_config=evidence / "unused-config",
                native_cwd=evidence,
                claude_node=Path(sys.executable),
                claude_adapter=evidence / "unused-adapter",
                claude_executable=Path(sys.executable),
                cursor_executable=Path(sys.executable),
                deadline=dt.datetime.now(dt.UTC),
            )
            inventory = evidence / "owned.json"
            owned_runtime.save_record(
                inventory, {"identities": {str(child.pid): identity.model_dump()}}
            )
            original = process_inventory.monotonic
            ticks = iter([0.0, 100.0, 200.0, 300.0])
            try:
                process_inventory.monotonic = lambda: next(ticks)
                with self.assertRaisesRegex(RuntimeError, "cleanup incomplete"):
                    process_inventory.settle_inventory(plan, inventory)
            finally:
                process_inventory.monotonic = original
            settlement = read_observed_json(
                (evidence / "owned-settlement.json").read_text()
            )
            self.assertIn(str(child.pid), observed_object(settlement["remaining"]))

    def test_actual_local_pipe_drives_only_synthetic_read_protocol(self) -> None:
        # This is a local apparatus fixture, never native/provider acceptance evidence.
        code = """import json, sys
for line in sys.stdin:
    message = json.loads(line)
    if message.get("id") == 1:
        print(json.dumps({"id": 1, "result": {}}), flush=True)
    elif message.get("id") == 2:
        thread = {"id": "fixture-thread", "turns": [{"id": "fixture-turn", "items": [{"type": "userMessage", "id": "fixture-native-item", "clientId": "fixture-push", "content": [{"type": "text", "text": "fixture"}]}]}]}
        print(json.dumps({"id": 2, "result": {"thread": thread}}), flush=True)
"""
        _ = lookup_process_identity(os.getpid())
        child = subprocess.Popen(
            [sys.executable, "-u", "-c", code],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
            bufsize=0,
        )
        result = read_carrier_correlation(
            child,
            NativeExpectation(
                thread_id="fixture-thread",
                turn_id="fixture-turn",
                push_id="fixture-push",
            ),
        )
        self.assertTrue(result["supported"])
        self.assertEqual(child.returncode, 0)


class ConfigurationAttributionTests(unittest.TestCase):
    def assert_array_child_move_rejected(
        self, before_text: str, after_text: str
    ) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            _ = config.write_text(before_text)
            before = readonly_codex_home.snapshot_configuration(profile, config)
            _ = config.write_text(after_text)
            edited_config = readonly_codex_home.fingerprint(config)

            observation = readonly_codex_home.observe_configuration(
                before, profile, config
            )
            self.assertEqual(observation.kind, "observed")
            with self.assertRaises(readonly_codex_home.ConfigurationChanged):
                readonly_codex_home.require_read_only_configuration(observation)

            self.assertEqual(readonly_codex_home.fingerprint(profile), before.profile)
            self.assertEqual(readonly_codex_home.fingerprint(config), edited_config)
            self.assertEqual(profile.read_text(), "saved profile")
            self.assertEqual(config.read_text(), after_text)

    def test_protected_child_move_to_later_array_entry_is_rejected(self) -> None:
        self.assert_array_child_move_rejected(
            '[[skills.config]]\npath = "first"\n[skills.config.rules]\nx = 1\n[[skills.config]]\npath = "second"\n',
            '[[skills.config]]\npath = "first"\n[[skills.config]]\npath = "second"\n[skills.config.rules]\nx = 1\n',
        )

    def test_protected_child_move_to_earlier_array_entry_is_rejected(self) -> None:
        self.assert_array_child_move_rejected(
            '[[skills.config]]\npath = "first"\n[[skills.config]]\npath = "second"\n[skills.config.rules]\nx = 1\n',
            '[[skills.config]]\npath = "first"\n[skills.config.rules]\nx = 1\n[[skills.config]]\npath = "second"\n',
        )

    def test_nested_protected_array_child_move_is_rejected(self) -> None:
        self.assert_array_child_move_rejected(
            '[[skills.config]]\npath = "outer"\n[[skills.config.rules]]\nname = "first"\n[skills.config.rules.details]\nx = 1\n[[skills.config.rules]]\nname = "second"\n',
            '[[skills.config]]\npath = "outer"\n[[skills.config.rules]]\nname = "first"\n[[skills.config.rules]]\nname = "second"\n[skills.config.rules.details]\nx = 1\n',
        )

    def test_external_array_child_moves_remain_allowed(self) -> None:
        for table in ["desktop.panels", 'projects."/another/repository".views']:
            with self.subTest(table=table), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                profile, config = root / "profile", root / "config"
                _ = profile.write_text("saved profile")
                first = f'[[{table}]]\nname = "first"\n'
                second = f'[[{table}]]\nname = "second"\n'
                child = f'[{table}.details]\nvalue = "PRIVATE_VALUE_CANARY"\n'
                _ = config.write_text(first + child + second)
                before = readonly_codex_home.snapshot_configuration(profile, config)
                _ = config.write_text(first + second + child)
                edited_config = readonly_codex_home.fingerprint(config)

                observation = readonly_codex_home.observe_configuration(
                    before, profile, config
                )
                readonly_codex_home.require_read_only_configuration(observation)

                self.assertFalse(observation.unchanged)
                self.assertNotIn("PRIVATE_VALUE_CANARY", observation.model_dump_json())
                self.assertEqual(
                    readonly_codex_home.fingerprint(profile), before.profile
                )
                self.assertEqual(readonly_codex_home.fingerprint(config), edited_config)

    def test_other_worktree_with_same_path_prefix_is_still_unrelated(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            _ = config.write_text('model = "fixed"\n')
            run_path = root / "repository"
            before = readonly_codex_home.snapshot_configuration(
                profile, config, (str(run_path),)
            )
            _ = config.write_text(
                f'model = "fixed"\n[projects."{run_path}.another-worktree"]\ntrust_level = "trusted"\n'
            )
            self.assertTrue(
                readonly_codex_home.observe_configuration(
                    before, profile, config, (str(run_path),)
                ).guard_passed
            )

    def test_native_cwd_requires_preexisting_trust_without_writing_it(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            config = root / "config"
            _ = config.write_text('model = "fixed"\n')
            with self.assertRaises(readonly_codex_home.ConfigurationChanged):
                readonly_codex_home.require_trusted_native_cwd(config, root)
            self.assertEqual(config.read_text(), 'model = "fixed"\n')
            _ = config.write_text(
                f'[projects."{root.resolve()}"]\ntrust_level = "trusted"\n'
            )
            readonly_codex_home.require_trusted_native_cwd(config, root)

    def test_external_desktop_and_unrelated_project_changes_keep_values_private(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            _ = config.write_text('model = "fixed"\n')
            before = readonly_codex_home.snapshot_configuration(
                profile, config, (str(root),)
            )
            _ = config.write_text(
                'model = "fixed"\n[desktop.preferences]\nsecret = "PRIVATE_VALUE_CANARY"\n[projects."/another/repository"]\ntrust_level = "trusted"\n'
            )
            observation = readonly_codex_home.observe_configuration(
                before, profile, config, (str(root),)
            )
            self.assertTrue(observation.guard_passed)
            self.assertFalse(observation.unchanged)
            self.assertNotIn("PRIVATE_VALUE_CANARY", observation.model_dump_json())
            self.assertIsNone(
                readonly_codex_home.require_read_only_configuration(observation)
            )

    def test_root_unknown_and_run_references_fail_inside_external_tables(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            _ = config.write_text('model = "fixed"\n')
            references = (str(root), "exact-run-identifier")
            before = readonly_codex_home.snapshot_configuration(
                profile, config, references
            )
            for suffix in [
                'model = "changed"\n',
                'model = "fixed"\n[unknown]\nvalue = true\n',
                f'model = "fixed"\n[desktop.preferences]\npath = "{root}/owned"\n',
                'model = "fixed"\n[desktop.preferences]\nid = "exact-run-identifier"\n',
                f'model = "fixed"\n[projects."{root}/owned"]\ntrust_level = "trusted"\n',
                'model = "fixed"\n[projects."/"]\ntrust_level = "trusted"\n',
            ]:
                with self.subTest(suffix=suffix):
                    _ = config.write_text(suffix)
                    observation = readonly_codex_home.observe_configuration(
                        before, profile, config, references
                    )
                    self.assertFalse(observation.guard_passed)
                    with self.assertRaises(readonly_codex_home.ConfigurationChanged):
                        readonly_codex_home.require_read_only_configuration(observation)

    def test_removed_sections_and_strict_saved_profile(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            _ = config.write_text(
                'model = "fixed"\n[desktop.preferences]\nflag = true\n'
            )
            before = readonly_codex_home.snapshot_configuration(profile, config)
            _ = config.write_text('model = "fixed"\n')
            self.assertTrue(
                readonly_codex_home.observe_configuration(
                    before, profile, config
                ).guard_passed
            )
            _ = profile.write_text("different profile")
            self.assertFalse(
                readonly_codex_home.observe_configuration(
                    before, profile, config
                ).guard_passed
            )

    def test_multiline_header_lookalikes_never_become_external_sections(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            _ = config.write_text('note = """\n[desktop.preferences]\nold\n"""\n')
            before = readonly_codex_home.snapshot_configuration(profile, config)
            _ = config.write_text('note = """\n[desktop.preferences]\nnew\n"""\n')
            observation = readonly_codex_home.observe_configuration(
                before, profile, config
            )
            self.assertFalse(observation.guard_passed)
            self.assertEqual(len(before.config_sections), 1)

    def test_invalid_tables_stop_without_restore(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            _ = config.write_text('model = "fixed"\n')
            before = readonly_codex_home.snapshot_configuration(profile, config)
            for text in ["invalid = [", "[desktop\nflag = true\n"]:
                _ = config.write_text(text)
                self.assertFalse(
                    readonly_codex_home.observe_configuration(
                        before, profile, config
                    ).guard_passed
                )
                self.assertEqual(config.read_text(), text)

    def test_repeated_array_tables_keep_every_occurrence_and_reject_unexpected_edit(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            original = '[[skills.config]]\npath = "first"\n[[skills.config]]\npath = "second"\n'
            _ = config.write_text(original)
            before = readonly_codex_home.snapshot_configuration(profile, config)
            self.assertEqual(
                [
                    s.occurrence
                    for s in before.config_sections
                    if s.table == ("skills", "config")
                ],
                [0, 1],
            )
            self.assertTrue(
                readonly_codex_home.observe_configuration(
                    before, profile, config
                ).guard_passed
            )
            _ = config.write_text(original.replace('"second"', '"changed"'))
            self.assertFalse(
                readonly_codex_home.observe_configuration(
                    before, profile, config
                ).guard_passed
            )

    def test_unchanged_preexisting_trusted_cwd_is_allowed_and_new_identity_is_protected(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            profile, config = root / "profile", root / "config"
            _ = profile.write_text("saved profile")
            original = f'[projects."{root}"]\ntrust_level = "trusted"\n'
            _ = config.write_text(original)
            before = readonly_codex_home.snapshot_configuration(
                profile, config, (str(root),)
            )
            self.assertTrue(
                readonly_codex_home.observe_configuration(
                    before, profile, config, (str(root),)
                ).guard_passed
            )
            readonly_codex_home.register_guard_identifiers(
                root, {"target": {"sessionId": "actual-new-run-session"}}
            )
            references = readonly_codex_home.configuration_references(root, (root,))
            _ = config.write_text(
                original + '[desktop.preferences]\nsession = "actual-new-run-session"\n'
            )
            self.assertFalse(
                readonly_codex_home.observe_configuration(
                    before, profile, config, references
                ).guard_passed
            )

    def test_controller_labels_external_change_without_claiming_whole_file_equality(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as folder:
            fixture = LifecycleFixture(Path(folder), "external-desktop-after-host")
            self.assertIsNone(fixture.drive())
            settlement = read_observed_json(
                (fixture.plan.evidence / "lifecycle-settlement.json").read_text()
            )
            self.assertTrue(settlement["configurationGuardPassed"])
            self.assertFalse(settlement["codexHomeUnchangedExactly"])
            self.assertIsNone(settlement["failure"])
