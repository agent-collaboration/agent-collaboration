#!/usr/bin/env python3
"""Observe the board's real CLI/MCP outcomes against an identified debug build."""

import argparse
import datetime
import sys
import traceback
import typing as t
from pathlib import Path

from .board_journey_delivery import (
    poll_ranges_survive_policy_change_and_restart,
    same_reader_membership_corruption_is_isolated,
)
from .board_journey_failures import (
    aggregate_cursor_recovers_known_unavailable_project,
    unavailable_siblings_preserve_healthy_inbox,
)
from .board_journey_fixture import BoardFixture
from .board_journey_records import keyed_replay_moves_and_late_facts
from .board_journey_target import JourneyTarget
from .board_journey_runtime import (
    BoardRuntime,
    JourneyReceipt,
)
from .board_journey_values import (
    JsonObject,
    JsonValue,
    file_digest,
    run_command,
)
from .board_journey_scope import (
    project_watch_keeps_foreign_thread_context,
    rejected_scope_and_workspace_summary,
    semantic_acknowledgement_rejection,
)
from .board_journey_pages import complete_post_continuation_and_participants
from .board_journey_work import meaningful_views_and_pending_settlement
from .board_journey_roles import role_authority_and_cardinality
from .board_journey_subjects import required_subjects_and_recorded_refile
from .board_journey_notifications import reviewer_role_notifications

CASES: t.Final[dict[str, t.Callable[[BoardFixture], None]]] = {
    "rejected-scope": rejected_scope_and_workspace_summary,
    "project-watch": project_watch_keeps_foreign_thread_context,
    "membership-restart": same_reader_membership_corruption_is_isolated,
    "workspace-failures": unavailable_siblings_preserve_healthy_inbox,
    "aggregate-recovery": aggregate_cursor_recovers_known_unavailable_project,
    "semantic-ack": semantic_acknowledgement_rejection,
    "pending-views": meaningful_views_and_pending_settlement,
    "poll-restart": poll_ranges_survive_policy_change_and_restart,
    "recorded-work": keyed_replay_moves_and_late_facts,
    "complete-pages": complete_post_continuation_and_participants,
    "role-authority": role_authority_and_cardinality,
    "thread-subjects": required_subjects_and_recorded_refile,
    "role-notifications": reviewer_role_notifications,
}


class JourneyOptions(argparse.Namespace):
    def __init__(self, repository: Path) -> None:
        super().__init__()
        self.source_root: Path = repository
        self.expected_head: str | None = None
        self.output_directory: Path | None = None
        self.cases: list[str] | None = None
        self.list_cases: bool = False
        self.peer_qualification: Path | None = None


def argument_parser(repository: Path) -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument(
        "--source-root",
        type=Path,
        default=repository,
        help="Clean build source, or a verified Git export with its own target",
    )
    _ = parser.add_argument(
        "--expected-head", help="Required full source commit for an actual run"
    )
    _ = parser.add_argument(
        "--output-directory",
        type=Path,
        help="New output directory; existing receipts are never overwritten",
    )
    _ = parser.add_argument(
        "--case",
        action="append",
        choices=list(CASES),
        dest="cases",
        help="Run only named cases; default is all cases",
    )
    _ = parser.add_argument(
        "--list-cases",
        action="store_true",
        help="List cases without launching or building",
    )
    _ = parser.add_argument(
        "--peer-qualification",
        type=Path,
        help="Producer-byte qualification receipt required by G1",
    )
    return parser


def observe_case(name: str, fixture: BoardFixture) -> bool:
    receipt = fixture.receipt
    failed = False
    receipt.record({"kind": "case", "case": name, "stage": "start"})
    try:
        fixture.runtime.start()
        fixture.create()
        CASES[name](fixture)
    except Exception as error:
        failed = True
        receipt.record(
            {
                "kind": "caseFailure",
                "case": name,
                "errorType": type(error).__name__,
                "error": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
    finally:
        try:
            fixture.runtime.stop()
            fixture.runtime.target.require_unchanged_binaries()
        except Exception as error:
            failed = True
            receipt.record(
                {
                    "kind": "cleanupFailure",
                    "case": name,
                    "errorType": type(error).__name__,
                    "error": str(error),
                }
            )
    receipt.record(
        {
            "kind": "case",
            "case": name,
            "stage": "complete",
            "passed": not failed,
            "retainedOwnedRoot": str(fixture.runtime.root),
        }
    )
    return not failed


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    parser = argument_parser(repository)
    options = parser.parse_args(namespace=JourneyOptions(repository))
    if options.list_cases:
        for name in CASES:
            print(name)
        return 0
    expected = options.expected_head
    if expected is None:
        parser.error("--expected-head is required for a runtime observation")
    source_root = options.source_root
    selected = options.cases or list(CASES)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime(
        "%Y-%m-%dT%H%M%SZ"
    )
    output = options.output_directory
    if output is None:
        output = repository / "tmp/verify-m2/observations" / timestamp
    receipt = JourneyReceipt(output.resolve())
    try:
        target = JourneyTarget(source_root, expected, options.peer_qualification)
        receipt.target = target.inspect(repository)
        driver_head = run_command(["git", "rev-parse", "HEAD"], repository)
        driver_hashes: JsonObject = {
            str(path.relative_to(repository)): file_digest(path)
            for path in sorted(Path(__file__).parent.glob("*.py"))
        }
        driver_identity: JsonObject = {
            "driverHead": driver_head.stdout.strip(),
            "driverSource": str(Path(__file__).resolve()),
            "driverFiles": driver_hashes,
            "invocation": list[JsonValue](sys.argv),
            "selectedCases": list[JsonValue](selected),
            "declaredLimits": [
                "G1 session-entry author is a producer-qualified stand-in; native producer/turns remain M4",
                "O-R2-F2 transient scan retry is integration/mutation proof, per the Lead ruling",
                "No real central, native Codex lifecycle, external P3 delivery or production process use",
            ],
        }
        receipt.target.update(driver_identity)
        receipt.record({"kind": "target", "identity": receipt.target})
        results: JsonObject = {}
        for name in selected:
            runtime = BoardRuntime(target, receipt)
            results[name] = observe_case(name, BoardFixture(runtime))
        target.require_unchanged_binaries()
        passed = sum(result is True for result in results.values())
        failed = sum(result is False for result in results.values())
        receipt.record(
            {
                "kind": "result",
                "casesPassed": passed,
                "casesFailed": failed,
                "caseOutcomes": results,
                "receipt": str(receipt.directory / "journey.json"),
            }
        )
        return 1 if failed else 0
    except Exception as error:
        receipt.record(
            {
                "kind": "setupFailure",
                "errorType": type(error).__name__,
                "error": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
        return 2
    finally:
        receipt.close()


if __name__ == "__main__":
    sys.exit(main())
