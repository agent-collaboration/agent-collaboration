"""Drive board records and stop/reopen readback on a declared model-free Host.

Run from the repository root with Python 3.13 and Pydantic v2. See --help.
No builds, profile edits, installed Router calls or directory removal happen.
Exit 0: scoped assertions pass; 1: mismatch; 2: setup/driver failure;
3: required readback or real-entry coverage is unavailable.
"""

import argparse
import os
import sqlite3
import sys
import tempfile
import traceback
import typing as t
from pathlib import Path

from .board_journey_fixture import BoardFixture
from .board_journey_records import keyed_replay_moves_and_late_facts
from .board_journey_roles import role_authority_and_cardinality
from .board_journey_runtime import BoardRuntime, JourneyReceipt
from .board_journey_subjects import required_subjects_and_recorded_refile
from .board_journey_target import JourneyTarget
from .board_journey_values import JourneyFailure, JsonObject, JsonValue, file_digest
from .board_journey_work import meaningful_views_and_pending_settlement
from .board_store_readback import (
    REPLICATED_KEYS,
    read_stopped_store,
    schema_observations,
)

CASES: t.Final[dict[str, t.Callable[[BoardFixture], None]]] = {
    "recorded-work": keyed_replay_moves_and_late_facts,
    "pending-views": meaningful_views_and_pending_settlement,
    "role-authority": role_authority_and_cardinality,
    "thread-subjects": required_subjects_and_recorded_refile,
}


class RecordsOptions(argparse.Namespace):
    def __init__(self, repository: Path) -> None:
        super().__init__()
        self.source_root: Path = repository
        self.expected_head: str = ""
        self.artifact_root: Path | None = None
        self.runtime_parent: Path = Path()
        self.output_directory: Path = Path()
        self.cases: list[str] | None = None
        self.list_cases: bool = False


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument(
        "--source-root", type=Path, help="Clean source or a verified Git export"
    )
    _ = parser.add_argument(
        "--expected-head",
        help="Full product source commit, resolved in this repository",
    )
    _ = parser.add_argument(
        "--artifact-root",
        type=Path,
        help="Explicit coordinated Cargo target directory; executable hashes are pinned",
    )
    _ = parser.add_argument(
        "--runtime-parent",
        type=Path,
        help="Existing private task directory under system temp, outside the checkout",
    )
    _ = parser.add_argument(
        "--output-directory",
        type=Path,
        help="New evidence directory inside --runtime-parent",
    )
    _ = parser.add_argument(
        "--case", choices=list(CASES), action="append", dest="cases"
    )
    _ = parser.add_argument(
        "--list-cases",
        action="store_true",
        help="Describe selectable cases without launching",
    )
    return parser


def require_private_parent(parent: Path, repository: Path) -> Path:
    if not parent.is_absolute():
        raise JourneyFailure("runtime parent must be an explicit absolute directory")
    parent = parent.resolve(strict=True)
    roots = {Path(tempfile.gettempdir()).resolve(), Path("/tmp").resolve()}
    if (
        not parent.is_dir()
        or parent.stat().st_uid != os.getuid()
        or parent.stat().st_mode & 0o077
        or parent.is_relative_to(repository)
        or not any(parent != root and parent.is_relative_to(root) for root in roots)
    ):
        raise JourneyFailure(
            "runtime parent must be owned, private, temporary and outside the checkout"
        )
    return parent


def project_database(fixture: BoardFixture) -> Path:
    directory = fixture.runtime.directory / "projects"
    candidates = [directory / f"{fixture.project}.sqlite"]
    candidates.extend((directory / fixture.project).glob("e*.sqlite"))
    candidates = [path.resolve() for path in candidates if path.is_file()]
    if len(candidates) != 1 or not candidates[0].is_relative_to(
        fixture.runtime.root.resolve()
    ):
        raise JourneyFailure(
            "the single-epoch fixture has no unambiguous owned project file"
        )
    return candidates[0]


def read_only_health(fixture: BoardFixture) -> None:
    discovery = fixture.client.read("endpoints_list", {})
    fixture.receipt.equal(
        "read-only discovery identifies the owning Host epoch",
        "runtime health",
        (fixture.runtime.manifest.get("serviceEpoch"), discovery.get("serviceEpoch")),
    )


def record_readback(fixture: BoardFixture, stage: str) -> JsonObject:
    if fixture.runtime.child is not None:
        raise JourneyFailure("readback needs a completed owning Host stop")
    snapshot = read_stopped_store(project_database(fixture))
    path = fixture.receipt.directory / f"{fixture.runtime.root.name}-{stage}.json"
    _ = path.write_text(snapshot.model_dump_json(indent=2) + "\n", encoding="utf-8")
    fixture.receipt.record(
        {
            "kind": "storeReadback",
            "stage": stage,
            "path": str(path),
            "sha256": file_digest(path),
            "database": snapshot.database,
            "engine": snapshot.engine,
        }
    )
    for observation in schema_observations(
        snapshot, fixture.project, fixture.workstream
    ):
        fixture.receipt.record(
            {
                "kind": "assertion",
                "obligation": "PD2 §3a / OB1",
                **t.cast(JsonObject, observation.model_dump(mode="json")),
            }
        )
    # Physical file digests can change at startup. Equality concerns board rows,
    # keys and schema; library maintenance tables are not board records.
    return t.cast(
        JsonObject,
        {
            name: table.model_dump(mode="json")
            for name, table in snapshot.tables.items()
            if name in REPLICATED_KEYS
        },
    )


def drive_case(name: str, fixture: BoardFixture) -> int:
    receipt = fixture.receipt
    runtime = fixture.runtime
    outcome = 0
    start_record = len(receipt.records)
    receipt.record({"kind": "case", "case": name, "stage": "start"})
    try:
        runtime.start()
        read_only_health(fixture)
        fixture.create()
        CASES[name](fixture)
        before_feed = fixture.feed()
        first_manifest = runtime.manifest.copy()
        runtime.stop()
        try:
            before_rows = record_readback(fixture, "after-first-stop")
        except (JourneyFailure, OSError, ValueError, sqlite3.DatabaseError) as error:
            receipt.record(
                {
                    "kind": "coverageGap",
                    "case": name,
                    "stage": "offline-readback",
                    "detail": str(error),
                }
            )
            outcome = 3
        else:
            runtime.start()
            read_only_health(fixture)
            receipt.equal(
                "reopen retains service identity",
                "VD §2 owning reopen",
                (first_manifest.get("serviceId"), runtime.manifest.get("serviceId")),
            )
            receipt.equal(
                "reopen publishes a fresh service epoch",
                "VD §2 owning reopen",
                (
                    False,
                    first_manifest.get("serviceEpoch")
                    == runtime.manifest.get("serviceEpoch"),
                ),
            )
            receipt.equal(
                "public feed survives owning reopen exactly",
                "VD §2 / RC-5",
                (before_feed, fixture.feed()),
            )
            runtime.stop()
            after_rows = record_readback(fixture, "after-second-stop")
            receipt.equal(
                "stored board rows and schema survive owning reopen exactly",
                "VD §2 / OB1",
                (before_rows, after_rows),
            )
    except JourneyFailure as error:
        outcome = 1
        receipt.record(
            {
                "kind": "caseFailure",
                "case": name,
                "detail": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
    except Exception as error:  # noqa: BLE001 -- preserve raw driver failure and run owned cleanup
        outcome = 2
        receipt.record(
            {
                "kind": "driverFailure",
                "case": name,
                "detail": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
    finally:
        try:
            runtime.stop()
            runtime.target.require_unchanged_binaries()
        except Exception as error:  # noqa: BLE001 -- retain any cleanup failure as a nonzero outcome
            outcome = 2
            receipt.record(
                {"kind": "cleanupFailure", "case": name, "detail": str(error)}
            )
        if outcome == 0 and any(
            record.get("kind") == "assertion" and record.get("passed") is False
            for record in receipt.records[start_record:]
        ):
            outcome = 1
        receipt.record(
            {
                "kind": "case",
                "case": name,
                "stage": "complete",
                "exit": outcome,
                "retainedOwnedRoot": str(runtime.root),
            }
        )
    return outcome


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    parser = argument_parser()
    options = parser.parse_args(namespace=RecordsOptions(repository))
    if options.list_cases:
        for name in CASES:
            print(name)
        return 0
    if (
        not options.expected_head
        or options.runtime_parent == Path()
        or options.output_directory == Path()
    ):
        parser.error(
            "--expected-head, --runtime-parent and --output-directory are required"
        )
    parent = require_private_parent(options.runtime_parent, repository)
    output = options.output_directory.resolve()
    if not output.is_relative_to(parent) or output == parent:
        parser.error(
            "output directory must be a new child of the private runtime parent"
        )
    receipt = JourneyReceipt(output)
    try:
        target = JourneyTarget(
            options.source_root, options.expected_head, None, options.artifact_root
        )
        receipt.target = target.inspect(repository)
        receipt.target.update(
            {
                "adoptedReference": "board VD OB1 and §2; PD2 §3a at 3dd15c23",
                "driverDigests": {
                    path.name: file_digest(path)
                    for path in sorted(Path(__file__).parent.glob("*.py"))
                },
                "limits": [
                    "Model-free owning Host; native sessions and provider callbacks are separate journeys",
                    "Library writer, capture, multi-epoch admission and OB10 merge coverage require their declared A targets",
                    "Replication and holder-change require runnable A3a/A3c composition; no missing-entry run counts as behavioral red",
                    "Four reused record/view/role/subject cases are not every V1/V2/V5/V8 obligation",
                ],
            }
        )
        receipt.record({"kind": "target", "identity": receipt.target})
        results = [
            drive_case(name, BoardFixture(BoardRuntime(target, receipt, parent)))
            for name in options.cases or list(CASES)
        ]
        checks = [
            record for record in receipt.records if record.get("kind") == "assertion"
        ]
        gaps = sum(record.get("kind") == "coverageGap" for record in receipt.records)
        failed = sum(record.get("passed") is False for record in checks)
        exit_code = (
            2
            if 2 in results
            else 3
            if 3 in results or gaps
            else 1
            if failed or 1 in results
            else 0
        )
        receipt.record(
            {
                "kind": "result",
                "exit": exit_code,
                "caseExits": list[JsonValue](results),
                "assertionsPassed": sum(
                    record.get("passed") is True for record in checks
                ),
                "assertionsFailed": failed,
                "coverageGaps": gaps,
            }
        )
        return exit_code
    except Exception as error:  # noqa: BLE001 -- top-level evidence boundary
        receipt.record(
            {
                "kind": "setupFailure",
                "detail": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
        return 2
    finally:
        receipt.close()


if __name__ == "__main__":
    sys.exit(main())
