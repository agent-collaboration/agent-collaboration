"""Drive the declared importer, export owning native snapshots, keep private receipts."""

import contextlib
import json
import os
import selectors
import shutil
import socket
import sqlite3
import subprocess
import sys
import typing as t
from pathlib import Path

from pydantic import TypeAdapter

from .board_journey_target import JourneyTarget
from .board_journey_values import file_digest
from .board_records_journey import require_private_parent
from .board_upgrade_native_oracle import (
    Checks,
    verify_native,
)
from .board_upgrade_source import source_readers, source_rows
from .board_upgrade_views import (
    DECLARED_OWNER,
    DECLARED_PROVENANCE,
    DryRun,
    NativeObservation,
    Object,
    ObservationPlan,
    Refused,
    UpgradeReport,
    json_object,
    run_report,
    string_list,
    text_field,
)

FIXTURES = (
    "legacy-complete.sqlite",
    "legacy-reopened.sqlite",
    "role-history.sqlite",
    "watch-scope.sqlite",
    "mixed-watch-scope.sqlite",
)


def fingerprint(path: Path) -> Object:
    status = path.stat()
    return {
        "sha256": file_digest(path),
        "size": status.st_size,
        "mtimeNs": status.st_mtime_ns,
    }


@t.final
class UpgradeDrive:
    def __init__(self, plan: ObservationPlan) -> None:
        self.plan = plan
        self.checks = Checks()
        self.commands: list[Object] = []
        self.root = plan.output_directory
        _ = require_private_parent(self.root.parent, plan.reference_repository)
        self.root.mkdir(mode=0o700, exist_ok=False)
        self.sources = self.root / "sources"
        self.sources.mkdir(mode=0o700)
        self.temporary = self.root / "native-temporary"
        self.temporary.mkdir(mode=0o700)
        self.identity = self.root / "identity"
        self.identity.mkdir(mode=0o700)
        self.identity_file = self.identity / "service-identity.json"
        _ = self.identity_file.write_text(
            json.dumps({"version": 1, "serviceId": DECLARED_PROVENANCE["installation"]})
        )
        self.identity_file.chmod(0o600)
        self.identity_before = fingerprint(self.identity_file)
        self.original_sources: dict[str, Object] = {}
        fixture_root = plan.repository / "crates/board-legacy-upgrade/tests/fixtures"
        for name in (*FIXTURES, "collision.sqlite"):
            path = fixture_root / name
            if path.is_symlink() or path.resolve().parent != fixture_root.resolve():
                raise ValueError("fixture path escaped the declared source")
            self.original_sources[str(path)] = fingerprint(path)
            _ = shutil.copy2(path, self.sources / name)

    def save(self, outcome: str) -> None:
        _ = (self.root / "journey.json").write_text(
            json.dumps(
                {
                    "outcome": outcome,
                    "sourceHead": self.plan.expected_head,
                    "binary": str(self.plan.binary),
                    "binarySha256": self.plan.binary_sha256,
                    "nativeBinary": str(self.plan.native_binary),
                    "nativeSha256": self.plan.native_sha256,
                    "commands": self.commands,
                    "assertions": [
                        record.model_dump(mode="json") for record in self.checks.records
                    ],
                    "passed": sum(record.passed for record in self.checks.records),
                    "failed": sum(not record.passed for record in self.checks.records),
                    "limits": [
                        "synthetic fixtures only; real legacy copies owed",
                        "no central publication/recovery, alreadyImported/importConflict server answer, claim, staging or cross-machine observation",
                        "native preparation uses the product's provisional disposable writer",
                    ],
                },
                indent=2,
            )
            + "\n"
        )

    def call(self, label: str, arguments: list[str], expected_exit: int) -> None:
        self.checks.equal(
            "declared CLI bytes held",
            self.plan.binary_sha256,
            file_digest(self.plan.binary),
        )
        self.checks.equal(
            "native observer bytes held",
            self.plan.native_sha256,
            file_digest(self.plan.native_binary),
        )
        environment = dict(
            os.environ, TMPDIR=str(self.temporary), PYTHONDONTWRITEBYTECODE="1"
        )
        environment["CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE"] = str(
            self.root / "credentials-must-not-be-read"
        )
        result = subprocess.run(
            arguments,
            cwd=self.plan.repository,
            env=environment,
            capture_output=True,
            text=True,
            timeout=90,
            check=False,
        )
        _ = (self.root / f"{label}.stdout").write_text(result.stdout)
        _ = (self.root / f"{label}.stderr").write_text(result.stderr)
        self.commands.append(
            json_object.validate_python(
                {"label": label, "arguments": arguments, "exit": result.returncode}
            )
        )
        self.checks.equal(f"{label} exit", expected_exit, result.returncode)
        self.checks.equal(
            f"{label} native temporary files settled",
            [],
            sorted(path.name for path in self.temporary.iterdir()),
        )

    def cli(
        self,
        label: str,
        sources: list[Path],
        expected_exit: int = 0,
        *,
        dry_run: bool = True,
        owner: str = DECLARED_OWNER,
        machine: str = "Verifier fixture machine",
    ) -> DryRun | Refused:
        report = self.root / f"{label}-report.json"
        before = {str(path): fingerprint(path) for path in sources}
        before_files = {path.name for path in self.root.iterdir()}
        arguments = [
            str(self.plan.repository / "scripts/cargo_debug_signing_runner.sh"),
            str(self.plan.binary),
        ]
        for path in sources:
            arguments.extend(["--source", str(path)])
        arguments.extend(
            [
                "--router-directory",
                str(self.identity),
                "--report",
                str(report),
                "--owner-human-id",
                owner,
                "--machine-label",
                machine,
            ]
        )
        if dry_run:
            arguments.append("--dry-run")
        with contextlib.ExitStack() as resources:
            endpoint = None
            if not dry_run:
                endpoint = resources.enter_context(socket.socket())
                endpoint.bind(("127.0.0.1", 0))
                endpoint.listen()
                endpoint.setblocking(False)
                _, port = TypeAdapter(tuple[str, int]).validate_python(
                    endpoint.getsockname()
                )
                arguments.extend(["--central-url", f"http://127.0.0.1:{port}"])
            self.call(label, arguments, expected_exit)
            if endpoint is not None:
                try:
                    connection = endpoint.accept()[0]
                except BlockingIOError:
                    contacted = False
                else:
                    connection.close()
                    contacted = True
                self.checks.equal("collision never contacts central", False, contacted)
        expected_new = {report.name, f"{label}.stdout", f"{label}.stderr"}
        self.checks.equal(
            f"{label} persistent effects are report/log only",
            sorted(expected_new),
            sorted({path.name for path in self.root.iterdir()} - before_files),
        )
        self.checks.equal(
            f"{label} source bytes/mtime unchanged",
            before,
            {str(path): fingerprint(path) for path in sources},
        )
        self.checks.equal(
            f"{label} identity unchanged",
            self.identity_before,
            fingerprint(self.identity_file),
        )
        return run_report.validate_json(report.read_bytes())

    def native(self, label: str, source: Path) -> NativeObservation:
        result_path = self.root / f"{label}-native.json"
        request = self.root / f"{label}-request.json"
        rows = source_rows(source)
        _ = request.write_text(
            json.dumps(
                {
                    "source": str(source),
                    "output": str(result_path),
                    "readers": source_readers(rows),
                    "owner": DECLARED_OWNER,
                    "provenance": DECLARED_PROVENANCE,
                }
            )
        )
        before = fingerprint(source)
        self.call(
            label + "-native",
            [
                str(self.plan.repository / "scripts/cargo_debug_signing_runner.sh"),
                str(self.plan.native_binary),
                str(request),
            ],
            0,
        )
        self.checks.equal(
            f"{label} native source bytes/mtime unchanged", before, fingerprint(source)
        )
        result = NativeObservation.model_validate_json(result_path.read_bytes())
        verify_native(source, result, self.checks)
        return result

    def verify_report(self, source: Path, report: UpgradeReport) -> None:
        rows = source_rows(source).tables
        self.checks.equal(
            "CLI project IDs match source topics",
            sorted(text_field(row, "topic_id") for row in rows["board_topics"]),
            sorted(row.project for row in report.projects),
        )
        self.checks.equal(
            "CLI workspace IDs/count",
            sorted(text_field(row, "project_id") for row in rows["board_projects"]),
            sorted(text_field(row, "workspace") for row in report.workspace_counts),
        )
        self.checks.equal(
            "CLI board IDs/count",
            sorted(text_field(row, "board_id") for row in rows["project_boards"]),
            sorted(text_field(row, "board") for row in report.board_counts),
        )
        self.checks.equal(
            "CLI total post count",
            len(rows["board_messages"]),
            sum(row.posts for row in report.projects),
        )
        for project in report.projects:
            topic = next(
                row
                for row in rows["board_topics"]
                if row["topic_id"] == project.project
            )
            self.checks.equal(
                "CLI exact legacy title/description",
                [topic["name"], topic["description"]],
                [project.title, project.description],
            )
        for record in report.fleet.records:
            self.checks.equal(
                "fleet imported time unknown", {"kind": "legacyUnknown"}, record["time"]
            )
            if record["kind"] == "directory":
                topic = next(
                    row
                    for row in rows["board_topics"]
                    if row["topic_id"] == record["project"]
                )
                self.checks.equal(
                    "directory exact title-only fields",
                    {
                        "kind": "directory",
                        "project": topic["topic_id"],
                        "board": topic["board_id"],
                        "title": topic["name"],
                        "time": {"kind": "legacyUnknown"},
                    },
                    record,
                )

    def drive(self) -> None:
        for name in FIXTURES:
            source = self.sources / name
            label = name.removesuffix(".sqlite")
            result = self.cli(label, [source])
            if result.kind != "dryRun":
                raise AssertionError("valid fixture was refused")
            self.verify_report(source, result.report)
            native = self.native(label, source)
            self.checks.equal(
                f"{label} independent CLI/native digest correlation",
                [row.digest for row in result.report.projects],
                [row.digest for row in native.report.projects],
            )
            self.save("running")
        source = self.sources / "role-history.sqlite"
        first = self.cli("repeat-original", [source])
        relabelled = self.cli(
            "repeat-other-label-owner",
            [source],
            owner="second-owner",
            machine="Other verifier machine",
        )
        if first.kind != "dryRun" or relabelled.kind != "dryRun":
            raise AssertionError("valid relabelled preparation was refused")
        self.checks.equal(
            "logical digests exclude provenance and owner",
            [row.digest for row in first.report.projects],
            [row.digest for row in relabelled.report.projects],
        )
        self.checks.equal(
            "fleet digest excludes provenance",
            first.report.fleet_digest,
            relabelled.report.fleet_digest,
        )
        changed = self.sources / "changed-description.sqlite"
        _ = shutil.copy2(source, changed)
        with contextlib.closing(sqlite3.connect(changed)) as connection:
            _ = connection.execute(
                "UPDATE board_topics SET description=? WHERE topic_id=?",
                (
                    "  exact\r\nchanged\0\tdescription  ",
                    "019a0000-0000-7000-8000-000000000015",
                ),
            )
            connection.commit()
        updated = self.cli("changed-description", [changed])
        if updated.kind != "dryRun":
            raise AssertionError("valid description change was refused")
        self.checks.equal(
            "changed logical content changes digest",
            True,
            [row.digest for row in first.report.projects]
            != [row.digest for row in updated.report.projects],
        )
        _ = self.native("changed-description", changed)
        for label, value in [
            ("empty-description", ""),
            ("maximum-description", "λ\0\n" * 4096),
        ]:
            candidate = self.sources / f"{label}.sqlite"
            _ = shutil.copy2(source, candidate)
            with contextlib.closing(sqlite3.connect(candidate)) as connection:
                _ = connection.execute(
                    "UPDATE board_topics SET description=? WHERE topic_id=?",
                    (value, "019a0000-0000-7000-8000-000000000016"),
                )
                connection.commit()
            result = self.cli(label, [candidate])
            if result.kind != "dryRun":
                raise AssertionError("allowed description refused")
            self.verify_report(candidate, result.report)
            _ = self.native(label, candidate)
        invalid = self.sources / "oversized-description.sqlite"
        _ = shutil.copy2(source, invalid)
        with contextlib.closing(sqlite3.connect(invalid)) as connection:
            _ = connection.execute(
                "UPDATE board_topics SET description=? WHERE topic_id=?",
                ("x" * 16385, "019a0000-0000-7000-8000-000000000016"),
            )
            connection.commit()
        refused = self.cli("oversized-description", [invalid], 2)
        if refused.kind != "refused":
            raise AssertionError("oversized description accepted")
        self.checks.equal(
            "description refusal names exact topic",
            True,
            "019a0000-0000-7000-8000-000000000016" in refused.reason
            and "description" in refused.reason,
        )
        holder_code = "import sqlite3,sys; c=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); c.execute('SELECT count(*) FROM sqlite_schema').fetchone(); print('READY',flush=True); sys.stdin.read(); c.close()"
        holder: subprocess.Popen[bytes] = subprocess.Popen(
            [sys.executable, "-u", "-c", holder_code, str(source)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        try:
            if holder.stdout is None:
                raise ValueError("owned source-holder stdout missing")
            source_holder_stdout: t.IO[bytes] = holder.stdout
            with selectors.DefaultSelector() as selector:
                _ = selector.register(source_holder_stdout, selectors.EVENT_READ)
                if not selector.select(5):
                    raise ValueError("owned source-holder readiness timeout")
                self.checks.equal(
                    "real read-only source holder ready",
                    b"READY\n".decode(),
                    source_holder_stdout.readline().decode(),
                )
            result = self.cli("open-source", [source], 2)
            if result.kind != "refused":
                raise AssertionError("open source accepted")
            self.checks.equal(
                "open-source refusal identifies real consumer",
                True,
                "open in another process" in result.reason,
            )
        finally:
            if holder.stdin is not None:
                holder.stdin.close()
            try:
                _ = holder.wait(timeout=10)
            except subprocess.TimeoutExpired:
                holder.kill()
                _ = holder.wait(timeout=5)
            for pipe in [holder.stdout, holder.stderr]:
                if pipe is not None:
                    pipe.close()
        collision = self.cli(
            "collision-real-run",
            [
                self.sources / "legacy-complete.sqlite",
                self.sources / "collision.sqlite",
            ],
            2,
            dry_run=False,
        )
        if collision.kind == "dryRun":
            raise AssertionError("collision run did not refuse")
        if collision.report is None:
            raise AssertionError("collision run has no typed refusal report")
        self.checks.equal(
            "collision-specific real-run refusal",
            "name collisions refuse publication",
            collision.reason,
        )
        self.checks.equal(
            "workspace and board collisions each retain two IDs",
            ["board", "workspace"],
            sorted(
                text_field(row, "kind")
                for row in collision.report.collisions
                if len(string_list.validate_python(row["ids"])) == 2
            ),
        )
        for label, sql in [
            ("schema-index", "DROP INDEX board_projects_name_unique"),
            ("schema-history", "DROP TABLE _sqlx_migrations"),
            (
                "schema-checkpoint",
                "INSERT INTO activity_checkpoint(singleton,last_sequence,cursor_key) VALUES(2,0,randomblob(32))",
            ),
            (
                "schema-checksum",
                "UPDATE _sqlx_migrations SET checksum=randomblob(48) WHERE version=202610020001",
            ),
        ]:
            broken = self.sources / f"{label}.sqlite"
            _ = shutil.copy2(source, broken)
            with contextlib.closing(sqlite3.connect(broken)) as connection:
                _ = connection.executescript(sql)
                connection.commit()
            refusal = self.cli(label, [broken], 2)
            if refusal.kind != "refused":
                raise AssertionError("invalid schema was accepted")
            self.checks.equal(
                f"{label} schema refusal precedes import",
                True,
                "schema" in refusal.reason,
            )
        for path, before in self.original_sources.items():
            self.checks.equal(
                "original pinned fixture unchanged", before, fingerprint(Path(path))
            )


def observe(plan_path: Path) -> int:
    plan = ObservationPlan.model_validate_json(plan_path.read_bytes())
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=plan.repository,
        text=True,
        capture_output=True,
        check=False,
    ).stdout.strip()
    if (
        head == plan.expected_head
        and plan.repository.resolve() == plan.reference_repository.resolve()
    ):
        dirty = subprocess.run(
            [
                "git",
                "status",
                "--porcelain",
                "--untracked-files=all",
                "--",
                ".",
                ":(exclude)scripts/verification/**",
                ":(exclude).agents/skills/verify-codex-router/**",
            ],
            cwd=plan.repository,
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        if dirty:
            raise ValueError("immutable product source changed")
    else:
        _ = JourneyTarget(plan.repository, plan.expected_head, None).verify_export(
            plan.reference_repository, plan.expected_head
        )
    if (
        file_digest(plan.binary) != plan.binary_sha256
        or file_digest(plan.native_binary) != plan.native_sha256
    ):
        raise ValueError("declared binary hash mismatch before observation")
    drive = UpgradeDrive(plan)
    try:
        drive.drive()
    except BaseException:
        drive.save("failed")
        raise
    drive.save("fixture-observation-complete")
    print(
        json.dumps(
            {
                "outcome": "fixture-observation-complete",
                "assertions": len(drive.checks.records),
                "commands": len(drive.commands),
                "evidence": str(drive.root),
            }
        )
    )
    return 0
