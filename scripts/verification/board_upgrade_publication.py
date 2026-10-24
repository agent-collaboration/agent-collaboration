"""Run OB5 through the actual CLI and isolated Tokio central, retaining owned readbacks.

Use board_upgrade_journey --publication-plan PLAN.json [--scenario ...]. The plan
pins both binaries and source, supplied sealed copies and a fresh private output.
The same route accepts owner-supplied legacy copies later; it never acquires them.
"""

import json
import time
import typing as t
from pathlib import Path

from .board_journey_values import file_digest
from .board_records_journey import require_private_parent
from .board_upgrade_cli import fingerprint
from .board_upgrade_native_oracle import Checks
from .board_upgrade_publication_cli import PublicationCli, verify_target
from .board_upgrade_publication_runtime import CompleteGate, OwnedCentral, post_json
from .board_upgrade_publication_views import (
    CentralReadback,
    Header,
    Imported,
    PublicationPlan,
    Scenario,
    Uploaded,
    verify_published,
    verify_unpublished,
    workflows,
)
from .board_upgrade_source import source_rows
from .board_upgrade_views import (
    DryRun,
    UpgradeReport,
    json_object,
    text_field,
)
from .legacy_selection_oracle import read_sealed_copy


@t.final
class PublicationDrive:
    def __init__(self, plan: PublicationPlan, scenario: Scenario) -> None:
        self.plan = plan
        self.scenario = scenario
        self.checks = Checks()
        self.centrals: list[OwnedCentral] = []
        self.prepared: dict[Path, UpgradeReport] = {}
        verify_target(plan)
        _ = require_private_parent(plan.copy_root, plan.repository)
        _ = require_private_parent(plan.output_directory.parent, plan.repository)
        if len(set(plan.sources)) != len(plan.sources):
            raise ValueError("source copies must be distinct")
        for source in plan.sources:
            if source != source.resolve(strict=True) or not source.is_relative_to(
                plan.copy_root
            ):
                raise ValueError(
                    "source must be a canonical supplied file inside copy_root"
                )
            _ = read_sealed_copy(source, plan.copy_root)
        self.originals = {str(path): fingerprint(path) for path in plan.sources}
        self.root = plan.output_directory
        self.root.mkdir(mode=0o700, exist_ok=False)
        self.cli_driver = PublicationCli(plan, self.checks, self.originals)

    def save(self, outcome: str) -> None:
        paths = [
            Path(__file__),
            Path(__file__).with_name("board_upgrade_publication_runtime.py"),
            Path(__file__).with_name("board_upgrade_publication_cli.py"),
            Path(__file__).with_name("board_upgrade_publication_views.py"),
            Path(__file__).with_name("legacy_selection_oracle.py"),
            Path(__file__).with_name("board_upgrade_native") / "src/central.rs",
        ]
        _ = (self.root / "journey.json").write_text(
            json.dumps(
                {
                    "journey": "board-upgrade",
                    "stage": "publication",
                    "outcome": outcome,
                    "scenario": self.scenario,
                    "plan": self.plan.model_dump(mode="json"),
                    "sourceFingerprints": self.originals,
                    "commands": self.cli_driver.commands,
                    "centralSettlements": [
                        json_object.validate_json(
                            (
                                central.case / f"{central.label}-settlement.json"
                            ).read_bytes()
                        )
                        for central in self.centrals
                        if (central.case / f"{central.label}-settlement.json").exists()
                    ],
                    "assertions": [
                        record.model_dump(mode="json") for record in self.checks.records
                    ],
                    "passed": sum(record.passed for record in self.checks.records),
                    "failed": sum(not record.passed for record in self.checks.records),
                    "driverHashes": {
                        str(path.relative_to(self.plan.repository)): file_digest(path)
                        for path in paths
                    },
                    "limits": [
                        "B2c claim/write progress",
                        "C27 storage-full classification",
                        "cross-machine A/B composition",
                        "staging central",
                        "real owner sign-in",
                        "real legacy copies unless explicitly supplied and commissioned",
                        "publication receipt-loss interior windows remain separately covered by product tests",
                    ],
                },
                indent=2,
            )
            + "\n"
        )

    def verify_source_report(self, source: Path, report: UpgradeReport) -> None:
        rows = source_rows(source).tables
        self.checks.equal(
            "project IDs are exact legacy topics",
            sorted(text_field(row, "topic_id") for row in rows["board_topics"]),
            sorted(project.project for project in report.projects),
        )
        self.checks.equal("non-vacuous legacy projects", True, bool(report.projects))
        for project in report.projects:
            topic = next(
                row
                for row in rows["board_topics"]
                if row["topic_id"] == project.project
            )
            self.checks.equal(
                "exact legacy title/description",
                [topic["name"], topic["description"]],
                [project.title, project.description],
            )
            self.checks.equal(
                "post count independently from legacy",
                sum(
                    row["topic_id"] == project.project for row in rows["board_messages"]
                ),
                project.posts,
            )
        for record in report.fleet.records:
            self.checks.equal(
                "imported Fleet time unknown", {"kind": "legacyUnknown"}, record["time"]
            )

    def preparation(self, source: Path) -> UpgradeReport:
        if source not in self.prepared:
            case = self.root / f"dry-run-{len(self.prepared)}"
            case.mkdir(mode=0o700)
            before = sorted(path.name for path in case.iterdir())
            parsed = self.cli_driver.run(case, "dry-run", source)
            if not isinstance(parsed, DryRun):
                raise AssertionError("dry-run report missing")
            self.checks.equal(
                "dry-run report/log only",
                ["dry-run-report.json", "dry-run.stderr", "dry-run.stdout"],
                sorted(path.name for path in case.iterdir() if path.name not in before),
            )
            self.verify_source_report(source, parsed.report)
            self.prepared[source] = parsed.report
        return self.prepared[source]

    def central(self, case: Path, label: str, report: UpgradeReport) -> OwnedCentral:
        central = OwnedCentral(
            self.plan, case, label, [project.project for project in report.projects]
        )
        self.centrals.append(central)
        first = report.projects[0]
        health = post_json(
            f"{central.url}/admin/v1/import/projects/{first.project}",
            central.assertion_file,
            {"kind": "status", "value": {"digest": first.digest}},
        )
        kind = health["kind"]
        self.checks.equal(
            "read-only owner route is ready", True, kind in {"progress", "refused"}
        )
        if kind == "refused":
            self.checks.equal(
                "fresh status is exactly NotFound",
                {"kind": "notFound"},
                health["value"],
            )
        _ = (case / f"{label}-health.json").write_text(json.dumps(health, indent=2))
        return central

    def snapshot(
        self, central: OwnedCentral, label: str, expected: int
    ) -> CentralReadback:
        deadline = time.monotonic() + 10
        attempt = 0
        while True:
            snapshot = central.inspect(f"{label}-{attempt}")
            # A false publication answer must reach the actual state assertion,
            # rather than being hidden behind a projection-readiness timeout.
            following = all(
                copy.record
                and json_object.validate_json(copy.record[0])["state"] == "following"
                for copy in snapshot.copies
            )
            if (
                len(snapshot.directory) == expected
                or not following
                or time.monotonic() >= deadline
            ):
                return snapshot
            attempt += 1

    def uploaded(
        self, value: DryRun | Uploaded | None, source: Path, expected: dict[str, str]
    ) -> Uploaded:
        if not isinstance(value, Uploaded):
            raise TypeError("real CLI upload report missing")
        self.verify_source_report(source, value.report)
        answers = value.publication.projects
        self.checks.equal("non-vacuous upload answers", True, bool(answers))
        self.checks.equal(
            "answers cover every legacy project",
            sorted(expected),
            sorted(answer.project for answer in answers),
        )
        for answer in answers:
            self.checks.equal(
                "publication/replay answer",
                expected[answer.project],
                answer.answer.kind,
            )
            if answer.answer.kind == "imported":
                project = next(
                    project
                    for project in value.report.projects
                    if project.project == answer.project
                )
                self.checks.equal(
                    "Imported head matches CLI manifest",
                    project.manifest["position"],
                    answer.answer.value.head.model_dump(),
                )
        return value

    def replay(
        self, case: Path, source: Path, report: UpgradeReport, before: CentralReadback
    ) -> None:
        central = self.central(case, "reopened", report)
        answer = self.uploaded(
            self.cli_driver.run(
                case, "replay-relabeled", source, central, relabel=True
            ),
            source,
            {project.project: "alreadyImported" for project in report.projects},
        )
        snapshot = self.snapshot(central, "replayed", len(report.projects))
        retained = {
            key: json_object.validate_json(row.header)
            for key, row in workflows(before).items()
        }
        verify_published(self.checks, answer.report, snapshot, retained)
        closed = central.stop("closed")
        self.checks.equal(
            "replay changes no copy/seed/summary/workflow/projection bytes",
            before.model_dump(),
            closed.model_dump(),
        )

    def publication(self, source: Path, index: int) -> None:
        report = self.preparation(source)
        case = self.root / f"publication-{index}"
        case.mkdir(mode=0o700)
        central = self.central(case, "original", report)
        answer = self.uploaded(
            self.cli_driver.run(case, "upload", source, central),
            source,
            {project.project: "imported" for project in report.projects},
        )
        snapshot = self.snapshot(central, "published", len(report.projects))
        verify_published(self.checks, answer.report, snapshot, {})
        closed = central.stop("closed")
        self.checks.equal(
            "owning stop retains published state",
            snapshot.model_dump(),
            closed.model_dump(),
        )
        self.replay(case, source, answer.report, closed)

    def interruption(self, phase: t.Literal["before", "after"]) -> None:
        source = self.plan.sources[0]
        report = self.preparation(source)
        case = self.root / f"interruption-{phase}"
        case.mkdir(mode=0o700)
        central = self.central(case, "original", report)
        gate = CompleteGate(central.url, phase)
        try:
            _ = self.cli_driver.run(case, "interrupted", source, central, gate=gate)
        finally:
            gate.close()
        if gate.state.checkpoint is None:
            raise AssertionError("wire checkpoint missing")
        database = text_field(gate.state.checkpoint, "path").rsplit("/", 1)[-1]
        header = gate.state.begins[database]
        captured = Header.model_validate(header)
        first_report = next(
            project for project in report.projects if project.project == database
        )
        self.checks.equal(
            "checkpoint semantic identity comes from the legacy CLI",
            first_report.digest,
            captured.digest,
        )
        before = central.stop("interrupted-closed")
        saved = workflows(before)[database]
        if phase == "before":
            _ = verify_unpublished(self.checks, before, header)
        else:
            response = Imported.model_validate(gate.state.checkpoint["response"])
            self.checks.equal(
                "held publication answer matches its wire header",
                captured.manifest.position.model_dump(),
                response.value.head.model_dump(),
            )
            first_copy = next(
                copy for copy in before.copies if copy.database == database
            )
            subset = CentralReadback(
                copies=[first_copy],
                workflows=before.workflows,
                directory=before.directory,
            )
            verify_published(
                self.checks,
                report.model_copy(update={"projects": [first_report]}),
                subset,
                {database: header},
            )
            for copy in before.copies:
                if copy.database != database:
                    self.checks.equal(
                        "later projects not reached before interruption",
                        [],
                        copy.record,
                    )
        reopened = self.central(case, "resumed", report)
        expected = {
            project.project: "alreadyImported"
            if phase == "after" and project.project == database
            else "imported"
            for project in report.projects
        }
        resumed = self.uploaded(
            self.cli_driver.run(
                case, "resumed", source, reopened, relabel=phase == "after"
            ),
            source,
            expected,
        )
        snapshot = self.snapshot(reopened, "resumed", len(report.projects))
        verify_published(self.checks, resumed.report, snapshot, {database: header})
        self.checks.equal(
            "same import identity survives full reopen",
            saved.import_id,
            workflows(snapshot)[database].import_id,
        )
        if phase == "after":
            self.checks.equal(
                "lost response does not republish the first copy",
                next(
                    copy for copy in before.copies if copy.database == database
                ).model_dump(),
                next(
                    copy for copy in snapshot.copies if copy.database == database
                ).model_dump(),
            )
        closed = reopened.stop("closed")
        self.replay(case, source, resumed.report, closed)

    def run(self) -> None:
        if self.scenario in {"all", "dry-run"}:
            for source in self.plan.sources:
                _ = self.preparation(source)
                self.save("running")
        if self.scenario in {"all", "publication"}:
            for index, source in enumerate(self.plan.sources):
                self.publication(source, index)
                self.save("running")
        if self.scenario in {"all", "before-publication"}:
            self.interruption("before")
            self.save("running")
        if self.scenario in {"all", "after-publication"}:
            self.interruption("after")

    def close(self) -> None:
        for central in reversed(self.centrals):
            central.close()
        self.cli_driver.close()
        self.checks.equal(
            "all supplied source bytes/mtime held",
            self.originals,
            {str(path): fingerprint(path) for path in self.plan.sources},
        )


def observe_publication(plan_path: Path, scenario: Scenario) -> int:
    plan = PublicationPlan.model_validate_json(plan_path.read_bytes())
    drive = PublicationDrive(plan, scenario)
    try:
        _ = drive.run()
    except BaseException:
        drive.close()
        drive.save("failed")
        raise
    drive.close()
    drive.save("passed")
    # Reopen retained evidence after teardown rather than trusting an in-memory result.
    retained = json_object.validate_json((drive.root / "journey.json").read_bytes())
    print(
        json.dumps(
            {
                "outcome": retained["outcome"],
                "passed": retained["passed"],
                "failed": retained["failed"],
                "evidence": str(drive.root),
            }
        )
    )
    return 0
