"""Prepare or run source-pinned G4/OB7 against local workerd or declared staging.

Default preparation makes no network call, resolves no credentials and launches
nothing. --execute-local permits disposable synthetic local enrollment only.
--execute-staging requires a separately authorized, deployed Access target and
two already-enrolled stopped fixture roots; it never enrolls staging machines.
No operation touches an installed production/debug Router or protected port.
"""

import argparse
import contextlib
import os
import sys
import traceback
from pathlib import Path

from .board_journey_fixture import BoardFixture
from .board_journey_runtime import JourneyReceipt
from .central_runtime import check_source
from .central_staging_cases import (
    CaseContext,
    access,
    idle_resume,
    replication_deploy,
    takeover_recovery,
)
from .central_staging_http import HttpEndpoint, environment_value
from .central_staging_models import LocalTarget, Plan, StagingTarget
from .central_staging_runtime import LocalWorker, RouterInputs, Routers

GAPS = [
    "Local workerd does not establish deployed Access policy, public address closure or Cloudflare deployment durability.",
    "Decoded-only BadSignature is a local Worker verifier component observation, never an ingress bypass on staging (1115).",
    "No existing object construction/startup signal was found. Idle survival cannot evidence physical eviction (1115).",
    "Staging address completeness and deployed version must be established by the separate private inventory collection.",
]


def private_evidence(plan: Plan) -> None:
    plan.evidence.mkdir(parents=True, mode=0o700, exist_ok=False)
    if (
        plan.evidence.stat().st_uid != os.getuid()
        or plan.evidence.stat().st_mode & 0o077
    ):
        raise ValueError("evidence must be owner-private")
    path = plan.evidence / "declaration.json"
    _ = path.write_text(plan.model_dump_json(indent=2) + "\n")
    path.chmod(0o600)


def prepare(plan: Plan) -> int:
    private_evidence(plan)
    path = plan.evidence / "preparation.txt"
    _ = path.write_text(
        "OB7 UNOBSERVED: preparation only; no commands, credentials or requests.\n"
        + "\n".join(GAPS)
        + "\nSelected: "
        + ", ".join(plan.cases)
        + "\n"
    )
    path.chmod(0o600)
    print("G4 preparation saved; OB7 remains unobserved. No target contacted.")
    return 0


def run(plan: Plan) -> int:
    # Product source is pinned; apparatus may be the declared in-progress writer.
    check_source(plan.repository, plan.expected_head)
    private_evidence(plan)
    plan.runtime_parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if (
        plan.runtime_parent.stat().st_uid != os.getuid()
        or plan.runtime_parent.stat().st_mode & 0o077
    ):
        raise ValueError("runtime parent must be private and owned")
    # JourneyReceipt creates its own fresh subdirectory and owns every raw log.
    with (
        (plan.evidence / "console.log").open("x", encoding="utf-8") as console,
        contextlib.redirect_stdout(console),
    ):
        receipt = JourneyReceipt(plan.evidence / "journey")
        local: LocalWorker | None = None
        routers: Routers | None = None
        try:
            if isinstance(plan.target, LocalTarget):
                local = LocalWorker(plan, receipt)
                owner, origin = local.owner, local.origin
            else:
                target = plan.target
                owner = HttpEndpoint(
                    target.origin,
                    {
                        "cf-access-token": environment_value(
                            target.owner_token_environment
                        )
                    },
                )
                origin = target.origin
            routers = Routers(
                RouterInputs(plan=plan, receipt=receipt, owner=owner, origin=origin)
            )
            for case in plan.cases:
                # Cases use independent project policy while the same two Router
                # processes and machine sessions remain alive across the run.
                fixture = BoardFixture(routers.runtimes[0])
                fixture.create()
                fixture.wait_replicated()
                context = CaseContext(
                    plan=plan,
                    receipt=receipt,
                    owner=owner,
                    routers=routers,
                    fixture=fixture,
                    local=local,
                )
                match case:
                    case "access":
                        access(context)
                    case "replication-deploy":
                        replication_deploy(context)
                    case "takeover-recovery":
                        takeover_recovery(context)
                    case "idle-resume":
                        idle_resume(context)
                receipt.record(
                    {
                        "kind": "caseCompleted",
                        "case": case,
                        "targetBoundary": plan.target.kind,
                    }
                )
            receipt.record({"kind": "limits", "gaps": list(GAPS), "ob7Complete": False})
        except Exception as error:
            traceback.print_exception(error, file=console)
            raise
        finally:
            errors: list[str] = []
            for label, cleanup in [
                ("routers", routers.close if routers else None),
                ("workerd", local.close if local else None),
            ]:
                if cleanup is not None:
                    try:
                        cleanup()
                    except Exception as error:  # noqa: BLE001 -- one owner cannot prevent the other from reaping
                        errors.append(label + ":" + type(error).__name__)
            receipt.record({"kind": "cleanup", "errors": list(errors)})
            receipt.close()
            if errors:
                raise ValueError("owned cleanup failed; inspect private evidence")
    print(
        f"Completed {len(plan.cases)} selected groups on {plan.target.kind}; OB7 gaps remain in the private receipt."
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--plan", type=Path, required=True)
    execution = parser.add_mutually_exclusive_group()
    _ = execution.add_argument("--execute-local", action="store_true")
    _ = execution.add_argument("--execute-staging", action="store_true")

    class Arguments(argparse.Namespace):
        plan: Path = Path()
        execute_local: bool = False
        execute_staging: bool = False

    arguments = parser.parse_args(namespace=Arguments())
    try:
        plan = Plan.model_validate_json(arguments.plan.read_bytes())
        if arguments.execute_local:
            if not isinstance(plan.target, LocalTarget):
                raise ValueError("local execution does not authorize staging")
            return run(plan)
        if arguments.execute_staging:
            if not isinstance(plan.target, StagingTarget):
                raise ValueError(
                    "staging execution requires a deployed Access declaration"
                )
            return run(plan)
        return prepare(plan)
    except Exception:  # noqa: BLE001 -- never echo credential-bearing library diagnostics
        # Library errors can include endpoint values. Detail remains in private
        # owned process/HTTP evidence; never echo credentials or raw exceptions.
        print(
            "G4 did not complete; inspect the private declaration and retained evidence.",
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
