"""Controlled real children qualify the fixed public entry; no Host/vendor runs."""

import fcntl
import json
import os
import pty
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unittest
from pathlib import Path

from pydantic import ValidationError

from scripts.verification.tui_public_sessions import (
    PublicHostIdentity,
    PublicSessionsLifecycle,
    PublicSessionsTarget,
    public_sessions_child_configuration,
)
from scripts.verification.tui_verification import ArtifactBinding

ADAPTER = (
    Path(__file__).resolve().parents[2]
    / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/fixed_quota_child.py"
)


class PublicSessionsAdapterTests(unittest.TestCase):
    def test_public_lifecycle_cannot_claim_fixture_zero_effects_or_job_settlement(
        self,
    ) -> None:
        receipt = {
            "kind": "public-sessions-cua-lifecycle-only",
            "binary_sha256": "a" * 64,
            "public_capture_source_sha256": "b" * 64,
            "fixed_child_sha256": "c" * 64,
            "host_service_id": "00000000-0000-4000-8000-000000000001",
            "host_service_epoch": "00000000-0000-4000-8000-000000000002",
            "child_exit": 0,
            "owned_processes_stopped": True,
            "artifact_sink_stopped": True,
            "artifact_count": 3,
            "last_viewport": "48 12",
            "effect_evidence_sha256": "d" * 64,
            "job_settlement": "separate-runtime-helper-proof-required",
        }
        _ = PublicSessionsLifecycle.model_validate(receipt)
        for changed in (
            {"provider_requests": 0},
            {"state_and_secrets_unchanged": True},
            {"job_settlement": "settled"},
            {"child_exit": 1},
            {"artifact_count": -1},
            {"artifact_count": True},
        ):
            with self.subTest(changed=changed), self.assertRaises(ValidationError):
                _ = PublicSessionsLifecycle.model_validate({**receipt, **changed})
        for missing in (
            "host_service_epoch",
            "effect_evidence_sha256",
            "public_capture_source_sha256",
        ):
            incomplete = {
                key: value for key, value in receipt.items() if key != missing
            }
            with self.subTest(missing=missing), self.assertRaises(ValidationError):
                _ = PublicSessionsLifecycle.model_validate(incomplete)

    def prepare_child(
        self, root: Path, *, held: bool
    ) -> tuple[Path, Path, dict[str, str]]:
        output = root / "output"
        output.mkdir(mode=0o700)
        cwd = root / "approved source"
        cwd.mkdir()
        debug_root = root / "owned debug root"
        (debug_root / "agent-communication").mkdir(parents=True)
        host = PublicHostIdentity.model_validate(
            {
                "service_id": "00000000-0000-4000-8000-000000000001",
                "service_epoch": "00000000-0000-4000-8000-000000000002",
            }
        )
        _ = (debug_root / "agent-communication/service.json").write_text(
            json.dumps(
                {
                    "serviceId": str(host.service_id),
                    "serviceEpoch": str(host.service_epoch),
                }
            )
        )
        record = output / "controlled-invocation.json"
        executable = root / "controlled public entry"
        _ = executable.write_text(
            "#!/usr/bin/python3\nimport json,os,sys,time\n"
            + f"with open({str(record)!r},'w') as stream:\n"
            + " json.dump({'argv':sys.argv[1:],'cwd':os.getcwd(),'home':os.environ.get('HOME'),"
            + "'codex_present':'CODEX_HOME' in os.environ,'codex':os.environ.get('CODEX_HOME'),"
            + "'sentinel':os.environ.get('PUBLIC_TEST_SENTINEL')},stream)\n"
            + ("while True: time.sleep(0.05)\n" if held else "")
        )
        executable.chmod(0o700)
        target = PublicSessionsTarget(
            binary=ArtifactBinding.capture(executable),
            approved_cwd=cwd,
            debug_router_root=debug_root,
            expected_host=host,
        )
        configuration = public_sessions_child_configuration(target, host, output)
        configuration.update(
            ttyd="unused", temporary_directory=str(root), dynamic_library_path="unused"
        )
        return output, record, configuration

    def start_adapter(
        self, root: Path, configuration: dict[str, str], environment: dict[str, str]
    ) -> subprocess.Popen[bytes]:
        config = root / "configuration.json"
        _ = config.write_text(json.dumps(configuration))
        master, slave = pty.openpty()
        self.addCleanup(os.close, master)
        try:
            _ = fcntl.ioctl(
                slave, termios.TIOCSWINSZ, struct.pack("HHHH", 12, 48, 0, 0)
            )
            return subprocess.Popen(
                [sys.executable, str(ADAPTER)],
                stdin=slave,
                stdout=slave,
                stderr=subprocess.DEVNULL,
                env={**environment, "QUOTA_STORYBOARD_FIXTURE_COMMAND": str(config)},
            )
        finally:
            os.close(slave)

    def test_exact_public_argv_cwd_environment_and_host_guard(self) -> None:
        for codex_home in (None, "normal-codex-home"):
            with (
                self.subTest(codex_home=codex_home),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                output, record, configuration = self.prepare_child(root, held=False)
                environment = {
                    "HOME": "normal-home",
                    "PUBLIC_TEST_SENTINEL": "preserved",
                }
                if codex_home is not None:
                    environment["CODEX_HOME"] = codex_home
                process = self.start_adapter(root, configuration, environment)
                try:
                    self.assertEqual(process.wait(timeout=3), 0)
                finally:
                    if process.poll() is None:
                        process.terminate()
                        _ = process.wait(timeout=5)
                self.assertEqual(
                    json.loads(record.read_text()),
                    {
                        "argv": [],
                        "cwd": configuration["approved_cwd"],
                        "home": "normal-home",
                        "codex_present": codex_home is not None,
                        "codex": codex_home,
                        "sentinel": "preserved",
                    },
                )
                self.assertEqual((output / "quota-child.viewport").read_text(), "48 12")

        for changed in (
            {"expected_service_epoch": "00000000-0000-4000-8000-000000000003"},
            {"binary_sha256": "0" * 64},
            {"arguments": "--new"},
        ):
            with (
                self.subTest(changed=changed),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                _, record, configuration = self.prepare_child(root, held=False)
                process = self.start_adapter(root, {**configuration, **changed}, {})
                self.assertNotEqual(process.wait(timeout=3), 0)
                self.assertFalse(
                    record.exists(), "invalid target must reject before public child"
                )

    def test_wrapper_reaps_only_its_retained_public_child(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output, record, configuration = self.prepare_child(root, held=True)
            process = self.start_adapter(root, configuration, {})
            try:
                deadline = time.monotonic() + 3
                while not record.exists() and time.monotonic() < deadline:
                    self.assertIsNone(process.poll())
                    time.sleep(0.01)
                self.assertTrue(record.exists())
                child_pid = int((output / "quota-child.pid").read_text())
                process.terminate()
                _ = process.wait(timeout=5)
                with self.assertRaises(ProcessLookupError):
                    os.kill(child_pid, 0)
                self.assertNotEqual((output / "quota-child.exit").read_text(), "0")
            finally:
                if process.poll() is None:
                    process.terminate()
                    _ = process.wait(timeout=5)


if __name__ == "__main__":
    _ = unittest.main()
