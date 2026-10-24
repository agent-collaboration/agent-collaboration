"""Drive board creation, records, replication and holder changes on a real central.

Use --plan with a CentralPlan JSON naming the exact source, the caller's qualified
artifact receipt, prebuilt board-upgrade-central, private runtime parent and a
fresh evidence directory. Set TMPDIR to that private cache parent before launch.
This command builds nothing, starts no provider and keeps its evidence after
closing only its own Hosts, fixture Access edges and local central. Exit 0 means
selected assertions passed; 1 is a deciding mismatch; 2 is setup/driver failure.
"""

import argparse
import json
import sys
import time
import traceback
import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, TypeAdapter

from .board_central_cases import (
    archive_and_notices,
    expiry,
    holder_change,
    replication,
    verified_forward,
)
from .board_central_runtime import (
    CentralPlan,
    FixtureAccessEdge,
    RealCentral,
    enroll_host,
)
from .board_joint_cases import joint_holder_and_forwarding
from .board_journey_fixture import BoardFixture
from .board_journey_runtime import BoardRuntime, JourneyReceipt, write_header
from .board_journey_target import JourneyTarget
from .board_journey_values import (
    JsonObject,
    array_value,
    field,
    file_digest,
    fixture_identity,
    object_value,
)
from .board_records_journey import CASES, drive_case, require_private_parent


class ArtifactBinding(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    path: Path
    sha256: str
    identifier: str
    team: str
    strictSignatureVerified: bool


def creation_succeeded(reply: JsonObject, project: str) -> bool:
    result = object_value(reply.get("result"))
    if result.get("kind") == "projectCreationPending":
        return field(result, "decision", "project") == project
    if result.get("kind") != "decided":
        return False
    decision = object_value(result.get("decision"))
    return decision.get("kind") == "accepted" and any(
        array_value(entry)[0] == {"kind": "project", "id": project}
        for entry in array_value(field(decision, "effects", "entries"))
    )


def fresh_create(fixture: BoardFixture) -> None:
    client, receipt = fixture.client, fixture.receipt
    request: JsonObject = {
        **write_header(fixture.owner),
        "workspaceId": fixture.workspace,
        "name": "G3 private workspace",
        "description": "Model-free local central observation",
    }
    deadline = time.monotonic() + 30
    while True:
        answer = client.read("workspace_create", request)
        if field(answer, "result", "kind") != "pendingUnreachable":
            receipt.equal(
                "real central session accepts workspace",
                "G3 setup",
                ("decided", field(answer, "result", "kind")),
            )
            break
        if time.monotonic() >= deadline:
            raise RuntimeError(
                "central session never accepted the stable workspace request"
            )
        time.sleep(0.2)
    _ = client.accepted(
        "board_create",
        {
            **write_header(fixture.owner),
            "workspaceId": fixture.workspace,
            "boardId": fixture.board,
            "name": "G3 board",
            "description": "Owned fixture",
            "kind": {"kind": "area"},
        },
    )
    first = client.cli(
        [
            "project",
            "create",
            "--board-id",
            fixture.board,
            "--project-id",
            fixture.project,
            "--title",
            "G3 fresh CLI project",
            "--actor",
            json.dumps(fixture.owner),
        ],
        "fresh project create exits successfully",
    )
    receipt.equal(
        "fresh CLI create returns a project or accepted-pending result",
        "1080 / G3 fresh project-create",
        (True, creation_succeeded(object_value(first["record"]), fixture.project)),
    )
    receipt.record({"kind": "freshCreate", "surface": "cli", "reply": first})
    project = fixture_identity()
    second = client.read(
        "project_create",
        {
            **write_header(fixture.owner),
            "projectId": project,
            "boardId": fixture.board,
            "title": "G3 fresh MCP project",
            "description": "Owned fresh project",
        },
    )
    receipt.equal(
        "fresh MCP create returns a project or accepted-pending result",
        "1080 / G3 fresh project-create",
        (True, creation_succeeded(second, project)),
    )
    receipt.record({"kind": "freshCreate", "surface": "mcp", "reply": second})


def run_plan(plan: CentralPlan) -> int:
    parent = require_private_parent(plan.runtime_parent, plan.repository)
    receipt = JourneyReceipt(plan.output_directory)
    central: RealCentral | None = None
    runtimes: list[BoardRuntime] = []
    edges: list[FixtureAccessEdge] = []
    exit_code = 2
    artifacts: list[ArtifactBinding] = []
    consumed: set[Path] = set()
    try:
        artifacts = TypeAdapter(list[ArtifactBinding]).validate_json(
            plan.artifacts.read_text()
        )
        consumed = {
            plan.central_binary,
            plan.repository / "target/debug/agent-collaboration",
            plan.repository / "target/debug/examples/board-debug-runtime",
        }
        if plan.scenario == "joint":
            consumed.add(plan.repository / "target/debug/codex-router-board-upgrade")
        if consumed - {artifact.path for artifact in artifacts}:
            raise RuntimeError("shared receipt omits a consumed artifact")
        for artifact in artifacts:
            if artifact.path not in consumed:
                continue
            receipt.equal(
                f"shared artifact unchanged: {artifact.path.name}",
                "exact target",
                (artifact.sha256, file_digest(artifact.path)),
            )
        receipt.record(
            {
                "kind": "artifactScope",
                "consumed": [str(path) for path in sorted(consumed)],
                "outsideTarget": [
                    str(artifact.path)
                    for artifact in artifacts
                    if artifact.path not in consumed
                ],
            }
        )
        target = JourneyTarget(
            plan.source_root or plan.repository,
            plan.expected_head,
            None,
            artifact_root=plan.repository / "target",
            known_unrelated_changes=plan.known_unrelated_changes,
        )
        receipt.target = target.inspect(plan.repository)
        receipt.record(
            {
                "kind": "target",
                "identity": receipt.target,
                "plan": t.cast(JsonObject, plan.model_dump(mode="json")),
            }
        )
        first = BoardRuntime(target, receipt, parent)
        runtimes.append(first)
        fixture = BoardFixture(first)
        central = RealCentral(plan, receipt, [fixture.project])
        edges.append(enroll_host(central, first))
        if plan.scenario == "fresh-create":
            fresh_create(fixture)
        if plan.scenario in {
            "replication",
            "holder-change",
            "archive",
            "verified-origin",
            "i1",
            "joint",
        }:
            second = BoardRuntime(target, receipt, parent)
            runtimes.append(second)
            edges.append(enroll_host(central, second))
            if plan.scenario == "joint":
                joint_holder_and_forwarding(fixture, BoardFixture(second), central)
            elif plan.scenario == "verified-origin":
                verified_forward(fixture, BoardFixture(second))
            elif plan.scenario == "archive":
                fixture.create()
                archive_and_notices(fixture, BoardFixture(second))
            else:
                replication(fixture, BoardFixture(second).client, central)
            if plan.scenario == "i1":
                expiry(fixture, BoardFixture(second).client, central)
            if plan.scenario == "holder-change":
                holder_change(fixture, BoardFixture(second), central)
                archive_and_notices(fixture, BoardFixture(second))
        if plan.scenario == "records":
            first.stop()
            names = plan.record_cases or list(CASES)
            if any(name not in CASES for name in names):
                raise RuntimeError("unknown record case")
            results = [drive_case(name, BoardFixture(first)) for name in names]
            if any(results):
                raise RuntimeError(f"records case exits: {results}")
        exit_code = 0
    except Exception as error:  # noqa: BLE001 -- evidence survives every failed attempt
        receipt.record(
            {
                "kind": "failure",
                "detail": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
        exit_code = 1
    finally:
        for runtime in reversed(runtimes):
            try:
                runtime.stop()
                runtime.target.require_unchanged_binaries()
            except Exception as error:  # noqa: BLE001 -- record cleanup independently
                receipt.record({"kind": "cleanupFailure", "detail": str(error)})
                exit_code = 2
        for edge in reversed(edges):
            try:
                edge.close()
            except Exception as error:  # noqa: BLE001 -- record cleanup independently
                receipt.record({"kind": "cleanupFailure", "detail": str(error)})
                exit_code = 2
        if central is not None:
            try:
                central.close()
            except Exception as error:  # noqa: BLE001 -- record cleanup independently
                receipt.record({"kind": "cleanupFailure", "detail": str(error)})
                exit_code = 2
        for artifact in artifacts:
            if artifact.path not in consumed:
                continue
            if file_digest(artifact.path) != artifact.sha256:
                receipt.record(
                    {
                        "kind": "cleanupFailure",
                        "detail": f"consumed binary changed: {artifact.path}",
                    }
                )
                exit_code = 2
        receipt.record({"kind": "result", "exit": exit_code})
        receipt.close()
    return exit_code


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument(
        "--plan",
        type=Path,
        required=True,
        help="CentralPlan JSON; no implicit build, root or installed Router",
    )

    class Options(argparse.Namespace):
        plan: Path = Path()

    options = parser.parse_args(namespace=Options())
    return run_plan(CentralPlan.model_validate_json(options.plan.read_text()))


if __name__ == "__main__":
    sys.exit(main())
