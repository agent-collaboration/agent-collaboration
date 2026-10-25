"""Verify independent SQLx schemas produce one lossless workspace cache."""

import importlib.util
import io
import subprocess
import tempfile
import typing as t
import unittest
from contextlib import redirect_stderr
from pathlib import Path
from types import ModuleType


def load_prepare_sqlx_module() -> ModuleType:
    script_path = Path(__file__).resolve().parents[1] / "tooling/prepare-sqlx.py"
    module_spec = importlib.util.spec_from_file_location("prepare_sqlx", script_path)
    if module_spec is None or module_spec.loader is None:
        raise RuntimeError(f"cannot load {script_path}")
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


PREPARE_SQLX = load_prepare_sqlx_module()


@t.final
class PrepareSqlxTests(unittest.TestCase):
    def __init__(self, methodName: str = "runTest") -> None:
        super().__init__(methodName)
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.repository_root = Path(self.directory.name)
        self.metadata_directory = self.repository_root / ".sqlx"
        self.metadata_directory.mkdir()
        tool_directory = self.repository_root / "tmp/rust-tools/bin"
        tool_directory.mkdir(parents=True)
        (tool_directory / "sqlx").touch()

    def run_workflow(
        self,
        *,
        check_metadata: bool,
        prepare_failure_package: str | None = None,
        conflicting_metadata: bool = False,
    ) -> tuple[int, list[tuple[list[str], dict[str, str]]]]:
        calls: list[tuple[list[str], dict[str, str]]] = []

        def fake_run(
            command: list[str],
            *,
            cwd: Path,
            env: dict[str, str] | None = None,
            check: bool,
        ) -> subprocess.CompletedProcess[str]:
            self.assertEqual(cwd, self.repository_root)
            self.assertFalse(check)
            captured_environment = dict(env or {})
            calls.append((command, captured_environment))
            if command[:2] == ["cargo", "check"]:
                package_name = captured_environment["SQLX_PREPARE_PACKAGE"]
                staging = Path(captured_environment["SQLX_PREPARE_METADATA_DIR"])
                metadata_name = (
                    "query-shared.json"
                    if conflicting_metadata
                    else f"query-{package_name}.json"
                )
                (staging / metadata_name).write_text(package_name)
                return subprocess.CompletedProcess(
                    command,
                    7 if package_name == prepare_failure_package else 0,
                )
            return subprocess.CompletedProcess(command, 0)

        result = PREPARE_SQLX.prepare_sqlx_metadata(
            repository_root=self.repository_root,
            check_metadata=check_metadata,
            run_process=fake_run,
        )
        return result, calls

    def test_prepares_each_package_against_its_required_schema_set(self) -> None:
        result, calls = self.run_workflow(check_metadata=False)

        self.assertEqual(result, 0)
        migration_calls = [
            (command, environment)
            for command, environment in calls
            if command[1:3] == ["migrate", "run"]
        ]
        prepare_calls = [
            (command, environment)
            for command, environment in calls
            if command[:2] == ["cargo", "check"]
        ]
        self.assertEqual(
            ["--ignore-missing" in command for command, _ in migration_calls],
            [False, False, False, True],
        )
        self.assertEqual(
            [command[command.index("--source") + 1] for command, _ in migration_calls],
            [
                "crates/automation-storage/migrations",
                "crates/codex-router-state/migrations",
                "crates/collaboration-service/migrations",
                "crates/automation-storage/migrations",
            ],
        )
        self.assertEqual(
            [environment["SQLX_PREPARE_PACKAGE"] for _, environment in prepare_calls],
            ["automation-storage", "codex-router-state", "collaboration-service"],
        )
        self.assertEqual(
            [
                Path(
                    environment["SQLX_PREPARE_DATABASE_URL"].removeprefix("sqlite://")
                ).name
                for _, environment in prepare_calls
            ],
            [
                "automation-schema.sqlite",
                "account-schema.sqlite",
                "provider-operation-schema.sqlite",
            ],
        )

    def test_successful_prepare_publishes_combined_metadata(self) -> None:
        (self.metadata_directory / "query-stale.json").write_text("stale")
        (self.metadata_directory / "README.md").write_text("preserve me")

        result, _ = self.run_workflow(check_metadata=False)

        self.assertEqual(result, 0)
        self.assertEqual(
            sorted(path.name for path in self.metadata_directory.glob("query-*.json")),
            [
                "query-automation-storage.json",
                "query-codex-router-state.json",
                "query-collaboration-service.json",
            ],
        )
        self.assertEqual(
            (self.metadata_directory / "README.md").read_text(), "preserve me"
        )

    def test_failed_prepare_restores_original_metadata(self) -> None:
        (self.metadata_directory / "query-original.json").write_text("original")

        result, _ = self.run_workflow(
            check_metadata=False,
            prepare_failure_package="collaboration-service",
        )

        self.assertEqual(result, 7)
        self.assertEqual(
            sorted(path.name for path in self.metadata_directory.glob("query-*.json")),
            ["query-original.json"],
        )
        self.assertEqual(
            (self.metadata_directory / "query-original.json").read_text(), "original"
        )

    def test_conflicting_same_hash_metadata_restores_original_cache(self) -> None:
        (self.metadata_directory / "query-original.json").write_text("original")

        error_output = io.StringIO()
        with redirect_stderr(error_output):
            result, _ = self.run_workflow(
                check_metadata=False,
                conflicting_metadata=True,
            )

        self.assertEqual(result, 1)
        self.assertIn("conflicting SQLx metadata", error_output.getvalue())
        self.assertEqual(
            sorted(path.name for path in self.metadata_directory.glob("query-*.json")),
            ["query-original.json"],
        )

    def test_check_validates_all_packages_without_changing_final_metadata(self) -> None:
        expected = {
            f"query-{package}.json": package
            for package in [
                "automation-storage",
                "codex-router-state",
                "collaboration-service",
            ]
        }
        for name, value in expected.items():
            (self.metadata_directory / name).write_text(value)
        result, calls = self.run_workflow(check_metadata=True)
        self.assertEqual(result, 0)
        descriptions = [
            (command, environment)
            for command, environment in calls
            if command[:2] == ["cargo", "check"]
        ]
        self.assertEqual(len(descriptions), 3)
        for command, environment in descriptions:
            self.assertIn("--all-targets", command)
            self.assertEqual(environment["SQLX_OFFLINE"], "true")
            self.assertNotIn("DATABASE_URL", environment)
            self.assertTrue(Path(environment["RUSTC_WRAPPER"]).is_file())
        self.assertEqual(
            PREPARE_SQLX.read_query_metadata(self.metadata_directory),
            {name: value.encode() for name, value in expected.items()},
        )

    def test_check_refuses_stale_or_missing_catalog_and_restores_it(self) -> None:
        original = self.metadata_directory / "query-existing.json"
        original.write_text("existing")
        with redirect_stderr(io.StringIO()):
            result, _ = self.run_workflow(check_metadata=True)
        self.assertEqual(result, 1)
        self.assertEqual(
            PREPARE_SQLX.read_query_metadata(self.metadata_directory),
            {"query-existing.json": b"existing"},
        )


if __name__ == "__main__":
    _ = unittest.main()
