"""Qualify the external OB4 driver on real local HTTP/WS with fixture Access.

Only this arrangement owns/enrolls a disposable local central. The matrix driver
receives already-enrolled declarations and keeps the same public edge for G4.
"""

import argparse
import os
import typing as t
from pathlib import Path

from .board_central_runtime import CentralPlan, FixtureAccessEdge, RealCentral
from .board_journey_runtime import JourneyReceipt
from .board_journey_values import JsonObject, file_digest, fixture_identity
from .cross_machine_cases import enroll_installation
from .cross_machine_journey import run_plan
from .cross_machine_lifecycle import Installation
from .cross_machine_plan import CentralTarget, CrossMachinePlan, FixtureOwnerAccess


def qualify_local(plan: CrossMachinePlan) -> int:
    if plan.mode != "local-conformance" or plan.central_binary is None:
        raise ValueError(
            "local matrix qualification requires its declared local-conformance inputs"
        )
    receipt = JourneyReceipt(plan.output_directory)
    installations: list[Installation] = []
    edges: list[FixtureAccessEdge] = []
    central: RealCentral | None = None
    original_environment: dict[str, str | None] = {}
    try:
        receipt.equal(
            "declared local central hash",
            "OB4 target",
            (plan.central_binary.sha256, file_digest(plan.central_binary.path)),
        )
        central = RealCentral(
            CentralPlan(
                repository=plan.repository,
                expected_head=plan.central_binary.source_head,
                artifacts=plan.output_directory / "unused-artifacts.json",
                central_binary=plan.central_binary.path,
                runtime_parent=plan.installations[0].temporary_parent,
                output_directory=plan.output_directory,
                scenario="fresh-create",
            ),
            receipt,
            [fixture_identity()],
        )
        for declaration in plan.installations:
            installation = Installation(declaration, plan.repository, receipt)
            installations.append(installation)
            installation.start()
            edges.append(enroll_installation(installation, central))
            installation.close()
            if installation.plan.session is None or installation.environment is None:
                raise ValueError("fixture did not produce an enrolled machine session")
            names = installation.plan.session.credentials
            for name in [names.machine, names.access_id, names.access_secret]:
                original_environment[name] = os.environ.get(name)
                os.environ[name] = installation.environment[name]
        matrix = CrossMachinePlan(
            repository=plan.repository,
            output_directory=plan.output_directory / "matrix",
            installations=(installations[0].plan, installations[1].plan),
            mode="matrix",
            central_target=CentralTarget(
                endpoint=central.ready.url,
                authentication=FixtureOwnerAccess(
                    kind="localFixture", assertion_file=central.assertion
                ),
            ),
            importer_binary=plan.importer_binary,
            caller=plan.caller,
            recipient_cases=plan.recipient_cases,
            allow_external_central=True,
        )
        path = plan.output_directory / "matrix-plan.json"
        _ = path.write_text(matrix.model_dump_json(indent=2))
        path.chmod(0o600)
        receipt.record(
            {
                "kind": "externalMatrixQualification",
                "plan": str(path),
                "sourceSha256": file_digest(Path(__file__)),
                "declaration": t.cast(JsonObject, matrix.model_dump(mode="json")),
            }
        )
        return run_plan(matrix)
    finally:
        failures: list[str] = []
        jobs: list[tuple[str, t.Callable[[], None]]] = [
            (item.plan.name, item.close) for item in reversed(installations)
        ]
        jobs.extend(("fixtureEdge", edge.close) for edge in reversed(edges))
        if central is not None:
            jobs.append(("fixtureCentral", central.close))
        for label, close in jobs:
            try:
                close()
            except Exception as error:  # noqa: BLE001 -- one owner cannot prevent the other exact consumers from stopping
                failures.append(label + ":" + type(error).__name__)
        for name, original in original_environment.items():
            if original is None:
                _ = os.environ.pop(name, None)
            else:
                os.environ[name] = original
        receipt.record({"kind": "localFixtureCleanup", "failures": list(failures)})
        receipt.close()
        if failures:
            raise RuntimeError(
                "local matrix fixture cleanup failed; inspect its retained receipt"
            )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--plan", type=Path, required=True)

    class Options(argparse.Namespace):
        plan: Path | None = None

    arguments = parser.parse_args(namespace=Options())
    if arguments.plan is None:
        raise ValueError("--plan is required")
    return qualify_local(
        CrossMachinePlan.model_validate_json(arguments.plan.read_bytes())
    )


if __name__ == "__main__":
    raise SystemExit(main())
