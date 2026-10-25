"""Verify Turso metadata is described natively and compared exactly, beside the stock cache."""

import importlib.util
import io
import subprocess
import sys
import tempfile
import typing as t
import unittest
from contextlib import redirect_stderr
from pathlib import Path

TursoPreparationTarget = tuple[str, tuple[str, ...]]


class ProcessRunner(t.Protocol):
    def __call__(
        self,
        command: list[str],
        /,
        *,
        cwd: Path,
        env: dict[str, str] | None = None,
        check: bool,
    ) -> subprocess.CompletedProcess[bytes]: ...


class PreparationModule(t.Protocol):
    TURSO_PREPARATION_TARGETS: tuple[TursoPreparationTarget, ...]

    def preparation_commands(
        self,
        *,
        repository_root: Path,
        package_name: str,
        migration_sources: tuple[str, ...],
        schema_database: Path,
        staging_directory: Path,
    ) -> list[tuple[list[str], dict[str, str]]]: ...

    def prepare_turso_metadata(
        self,
        *,
        repository_root: Path,
        check_metadata: bool,
        run_process: ProcessRunner,
        targets: tuple[TursoPreparationTarget, ...],
    ) -> int: ...


def load_tooling_module(script_name: str) -> PreparationModule:
    script_path = Path(__file__).resolve().parents[1] / "tooling" / script_name
    module_spec = importlib.util.spec_from_file_location(script_name, script_path)
    if module_spec is None or module_spec.loader is None:
        raise RuntimeError(f"cannot load {script_path}")
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return t.cast(PreparationModule, t.cast(object, module))


PREPARE_SQLX_TURSO = load_tooling_module("prepare-sqlx-turso.py")
RUSTC_SCHEMA_WRAPPER = (
    Path(__file__).resolve().parents[1] / "tooling/sqlx-package-rustc-wrapper.py"
)
TARGETS = (("interaction-storage", ("crates/interaction-storage/migrations",)),)


@t.final
class PrepareSqlxTursoTests(unittest.TestCase):
    def __init__(self, methodName: str = "runTest") -> None:
        super().__init__(methodName)
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.repository_root = Path(self.directory.name)
        self.metadata_directory = (
            self.repository_root / "crates/interaction-storage/.sqlx"
        )
        self.metadata_directory.mkdir(parents=True)
        self.stock_metadata = self.repository_root / ".sqlx"
        self.stock_metadata.mkdir()
        _ = (self.stock_metadata / "query-stock.json").write_text("stock")

    def run_workflow(
        self,
        *,
        check_metadata: bool,
        described: dict[str, str] | None = None,
        failing_command: str | None = None,
    ) -> tuple[int, list[tuple[list[str], dict[str, str]]]]:
        calls: list[tuple[list[str], dict[str, str]]] = []
        described_metadata = (
            described if described is not None else {"query-a.json": "a"}
        )

        def fake_run(
            command: list[str],
            *,
            cwd: Path,
            env: dict[str, str] | None = None,
            check: bool,
        ) -> subprocess.CompletedProcess[bytes]:
            self.assertEqual(cwd, self.repository_root)
            self.assertFalse(check)
            environment = dict(env or {})
            calls.append((command, environment))
            if command[1] == failing_command:
                return subprocess.CompletedProcess(command, 9)
            if command[1] == "check":
                staging = Path(environment["SQLX_PREPARE_METADATA_DIR"])
                for name, content in described_metadata.items():
                    _ = (staging / name).write_text(content)
            return subprocess.CompletedProcess(command, 0)

        result = PREPARE_SQLX_TURSO.prepare_turso_metadata(
            repository_root=self.repository_root,
            check_metadata=check_metadata,
            run_process=fake_run,
            targets=TARGETS,
        )
        return result, calls

    def test_seeds_a_native_schema_then_describes_online_one_compiler_at_a_time(
        self,
    ) -> None:
        result, calls = self.run_workflow(check_metadata=False)

        self.assertEqual(result, 0)
        self.assertEqual(
            [command[1] for command, _ in calls], ["run", "clean", "check"]
        )
        (seed, seed_environment), (_, _), (check, check_environment) = calls
        self.assertIn("seed_native_schema", seed)
        self.assertEqual(seed[-1], "crates/interaction-storage/migrations")
        self.assertEqual(
            seed[seed.index("--package") + 1],
            f"path+{(self.repository_root / 'crates/interaction-storage').resolve().as_uri()}",
        )
        self.assertEqual(seed_environment["SQLX_OFFLINE"], "true")
        self.assertNotIn("DATABASE_URL", seed_environment)
        schema_database = seed[seed.index("--") + 1]
        self.assertEqual(
            check_environment["SQLX_PREPARE_DATABASE_URL"], f"turso:{schema_database}"
        )
        self.assertEqual(check_environment["SQLX_OFFLINE"], "true")
        self.assertEqual(
            check_environment["SQLX_PREPARE_PACKAGE"], "interaction-storage"
        )
        self.assertTrue(Path(check_environment["RUSTC_WRAPPER"]).is_file())
        self.assertNotIn("DATABASE_URL", check_environment)
        self.assertEqual(check[check.index("--jobs") + 1], "1")
        self.assertIn("--all-features", check)

        self.assertIn("--all-targets", check)

    def test_write_mode_replaces_only_the_crate_metadata(self) -> None:
        _ = (self.metadata_directory / "query-stale.json").write_text("stale")

        result, _ = self.run_workflow(
            check_metadata=False, described={"query-a.json": "a", "query-b.json": "b"}
        )

        self.assertEqual(result, 0)
        self.assertEqual(
            sorted(path.name for path in self.metadata_directory.glob("query-*.json")),
            ["query-a.json", "query-b.json"],
        )
        self.assertEqual(
            (self.stock_metadata / "query-stock.json").read_text(), "stock"
        )

    def test_board_queries_seed_through_the_driver_example_with_all_store_schemas(
        self,
    ) -> None:
        sources = (
            "crates/message-board-storage/project_migrations",
            "crates/message-board-storage/project_bookkeeping_migrations",
            "crates/message-board-storage/fleet_cache_migrations",
            "crates/message-board-storage/fleet_outbox_migrations",
            "crates/message-board-storage/cursor_migrations",
        )
        commands = PREPARE_SQLX_TURSO.preparation_commands(
            repository_root=self.repository_root,
            package_name="message-board-storage",
            migration_sources=sources,
            schema_database=self.repository_root / "schema.db",
            staging_directory=self.repository_root / "metadata",
        )
        seed = commands[0][0]
        check = commands[-1][0]
        self.assertEqual(
            seed[seed.index("--package") + 1],
            f"path+{(self.repository_root / 'crates/interaction-storage').resolve().as_uri()}",
        )
        self.assertEqual(tuple(seed[-len(sources) :]), sources)
        self.assertEqual(
            check[check.index("--package") + 1],
            f"path+{(self.repository_root / 'crates/message-board-storage').resolve().as_uri()}",
        )
        self.assertIn("--all-targets", check)
        self.assertIn("--all-features", check)
        self.assertIn(
            ("message-board-storage", sources),
            PREPARE_SQLX_TURSO.TURSO_PREPARATION_TARGETS,
        )

    def test_a_failed_step_leaves_committed_metadata_untouched(self) -> None:
        _ = (self.metadata_directory / "query-original.json").write_text("original")

        result, calls = self.run_workflow(check_metadata=False, failing_command="check")

        self.assertEqual(result, 9)
        self.assertEqual(len(calls), 3)
        self.assertEqual(
            sorted(path.name for path in self.metadata_directory.glob("query-*.json")),
            ["query-original.json"],
        )

    def test_check_passes_only_on_identical_names_and_bytes(self) -> None:
        _ = (self.metadata_directory / "query-a.json").write_text("a")

        result, _ = self.run_workflow(check_metadata=True)

        self.assertEqual(result, 0)

    def test_check_reports_missing_stale_and_changed_files(self) -> None:
        _ = (self.metadata_directory / "query-a.json").write_text("old a")
        _ = (self.metadata_directory / "query-stale.json").write_text("stale")

        error_output = io.StringIO()
        with redirect_stderr(error_output):
            result, _ = self.run_workflow(
                check_metadata=True,
                described={"query-a.json": "a", "query-new.json": "n"},
            )

        self.assertEqual(result, 1)
        report = error_output.getvalue()
        self.assertIn("missing query-new.json", report)
        self.assertIn("stale query-stale.json", report)
        self.assertIn("changed query-a.json", report)
        self.assertEqual(
            (self.metadata_directory / "query-a.json").read_text(), "old a"
        )

    def test_describing_no_queries_is_an_error(self) -> None:
        error_output = io.StringIO()
        with redirect_stderr(error_output):
            result, _ = self.run_workflow(check_metadata=False, described={})

        self.assertEqual(result, 1)
        self.assertIn("no checked queries", error_output.getvalue())

    def test_rustc_wrapper_scopes_online_description_to_the_owning_package(
        self,
    ) -> None:
        # Invoke the real wrapper process. The compiler stand-in observes only these flags.
        environment = {
            "SQLX_PREPARE_PACKAGE": "message-board-storage",
            "SQLX_PREPARE_MANIFEST_DIR": "board-crate",
            "CARGO_MANIFEST_DIR": "board-crate",
            "SQLX_PREPARE_DATABASE_URL": "turso:board.db",
            "SQLX_PREPARE_METADATA_DIR": "board-metadata",
            "SQLX_OFFLINE": "false",
            "DATABASE_URL": "turso:wrong-inherited.db",
            "SQLX_OFFLINE_DIR": "wrong-inherited-metadata",
        }
        command = [
            str(sys.executable),
            str(RUSTC_SCHEMA_WRAPPER),
            str(sys.executable),
            "-c",
            "import os; print(os.environ.get('SQLX_OFFLINE')); print(os.environ.get('DATABASE_URL')); print(os.environ.get('SQLX_OFFLINE_DIR'))",
        ]

        for package, expected in [
            ("message-board-storage", ["false", "turso:board.db", "board-metadata"]),
            ("durable-steps", ["true", "None", "None"]),
            (None, ["true", "None", "None"]),
        ]:
            with self.subTest(package=package):
                scoped = (
                    environment
                    if package is None
                    else environment | {"CARGO_PKG_NAME": package}
                )
                result = subprocess.run(
                    command, env=scoped, capture_output=True, text=True, check=False
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines(), expected)
        other_manifest = environment | {
            "CARGO_PKG_NAME": "message-board-storage",
            "CARGO_MANIFEST_DIR": "different-source-with-same-name",
        }
        result = subprocess.run(
            command, env=other_manifest, capture_output=True, text=True, check=False
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ["true", "None", "None"])


if __name__ == "__main__":
    _ = unittest.main()
