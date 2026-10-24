"""Launch two exact local/SSH installations and drive reachable OB4 cases.

--plan names binaries/hashes/source, private roots, ports and environment names.
readiness mode makes no central connection. local-conformance uses disposable
local owner enrollment and a real Tokio central. Full matrix credit requires
all cases: partial or unreachable cases exit 2, never a vacuous green. This
driver builds nothing, deploys nothing, and makes no implicit staging call.
"""

import argparse
import json
import sys
import traceback
import typing as t
from pathlib import Path

from .board_central_runtime import CentralPlan, FixtureAccessEdge, RealCentral
from .board_journey_runtime import JourneyReceipt
from .board_journey_values import JsonObject, file_digest, fixture_identity
from .cross_machine_cases import (
    CASES,
    enroll_installation,
    fleet_outage,
    forwarding,
    recipient_gaps,
)
from .cross_machine_central import MatrixCentral
from .cross_machine_copy_control import qualify_pre_apply_hold
from .cross_machine_lifecycle import Installation, stage_remote
from .cross_machine_mail import recipient_cases
from .cross_machine_plan import CentralTarget, CrossMachinePlan, FixtureOwnerAccess
from .cross_machine_recipients import (
    Recipient,
    qualify_recipients,
)
from .cross_machine_retention import qualify_normal_replay


def run_plan(plan: CrossMachinePlan) -> int:
    receipt = JourneyReceipt(plan.output_directory)
    installations: list[Installation] = []
    edges: list[FixtureAccessEdge] = []
    central: RealCentral | None = None
    recipients: list[Recipient] = []
    results: dict[str, str] = {name: "not-run" for name in CASES}
    exit_code = 2
    try:
        receipt.record(
            {
                "kind": "driverIdentity",
                "python": sys.version,
                "sourceDigests": {
                    name: file_digest(Path(__file__).parent / name)
                    for name in [
                        "cross_machine_journey.py",
                        "cross_machine_plan.py",
                        "cross_machine_agent.py",
                        "cross_machine_lifecycle.py",
                        "cross_machine_cases.py",
                        "cross_machine_mail.py",
                        "cross_machine_recipients.py",
                        "cross_machine_central.py",
                        "cross_machine_copy_control.py",
                        "cross_machine_notifications.py",
                        "cross_machine_session_gate.py",
                        "cross_machine_retention.py",
                        "board_upgrade_external_transport.py",
                        "cross_machine_seed.py",
                        "board_central_runtime.py",
                        "board_journey_runtime.py",
                        "board_journey_values.py",
                        "board_store_readback.py",
                        "typed_observations.py",
                        "interaction_readback.py",
                    ]
                },
            }
        )
        receipt.record(
            {
                "kind": "declaredTarget",
                "plan": t.cast(JsonObject, plan.model_dump(mode="json")),
            }
        )
        for declaration in plan.installations:
            if declaration.ssh_host is not None:
                stage_remote(declaration, plan.repository, receipt)
            installation = Installation(declaration, plan.repository, receipt)
            installations.append(installation)
            installation.start()
            mcp = installation.read("endpoints_list", {})
            cli = installation.cli(["endpoints", "list"])
            receipt.record(
                {
                    "kind": "readinessPublicCalls",
                    "name": declaration.name,
                    "mcp": mcp,
                    "cli": cli,
                }
            )
            if plan.caller is not None:
                receipt.record(
                    {
                        "kind": "recipientSetupReadiness",
                        "name": declaration.name,
                        "controller": installation.cli(["whoami"]),
                        "workspace": installation.request(
                            {"kind": "prepareRecipientWorkspace"}
                        ),
                    }
                )
        receipt.equal(
            "distinct persisted installation identities",
            "OB4 target",
            (
                True,
                installations[0].manifest["serviceId"]
                != installations[1].manifest["serviceId"],
            ),
        )
        if plan.mode == "readiness":
            exit_code = 0
        else:
            if plan.mode == "local-conformance" and plan.central_binary is None:
                raise ValueError("central binary absent")
            if plan.mode == "local-conformance" and plan.central_binary is not None:
                receipt.equal(
                    "declared central artifact hash",
                    "OB4 target",
                    (plan.central_binary.sha256, file_digest(plan.central_binary.path)),
                )
                central_plan = CentralPlan(
                    repository=plan.repository,
                    expected_head=plan.central_binary.source_head,
                    artifacts=plan.output_directory / "unused-artifacts.json",
                    central_binary=plan.central_binary.path,
                    runtime_parent=plan.installations[0].temporary_parent,
                    output_directory=plan.output_directory,
                    scenario="fresh-create",
                )
                central = RealCentral(central_plan, receipt, [fixture_identity()])
                for installation in installations:
                    edges.append(enroll_installation(installation, central))
                owner = MatrixCentral(
                    CentralTarget(
                        endpoint=central.ready.url,
                        authentication=FixtureOwnerAccess(
                            kind="localFixture", assertion_file=central.assertion
                        ),
                    )
                )
            else:
                if plan.central_target is None:
                    raise ValueError("declared external central origin absent")
                owner = MatrixCentral(plan.central_target)
            receipt.record(
                {
                    "kind": "centralOwnerHttpReadiness",
                    "endpoint": owner.declaration.endpoint,
                    "response": owner.get("/admin/v1/copies?limit=1"),
                }
            )
            context: JsonObject | None = None
            try:
                context = fleet_outage(installations[1], installations[1], receipt)
                results["fleet-central-unreachable"] = "observed"
            except Exception as error:  # noqa: BLE001 -- one failed case never erases the remaining matrix
                results["fleet-central-unreachable"] = "failed"
                receipt.record(
                    {
                        "kind": "caseFailure",
                        "case": "fleet-central-unreachable",
                        "detail": str(error),
                    }
                )
            if context is not None:
                try:
                    forwarded = forwarding(
                        installations[0],
                        installations[1],
                        owner,
                        context,
                        receipt,
                        plan.importer_binary,
                    )
                    results["forwarding"] = "partial"
                    receipt.record(
                        {
                            "kind": "gap",
                            "case": "forwarding",
                            "detail": "Online root/reply, CLI/MCP replay, origin, actor, non-post refusal and exact offline lastSeen driven; unheld is driven when importer declared. Answer hold/cap and central nonretention need separate controls/readbacks; local central's 2s timeout proves no production30s bound.",
                        }
                    )
                    if all(
                        item.plan.recipient_provider is None for item in installations
                    ):
                        try:
                            frozen_control = qualify_pre_apply_hold(
                                installations[0],
                                installations[1],
                                owner,
                                forwarded,
                                receipt,
                            )
                        except Exception as control_error:  # noqa: BLE001 -- control feasibility never substitutes for actual recipient proof
                            receipt.record(
                                {
                                    "kind": "controlQualificationFailure",
                                    "case": "held-copy-ack",
                                    "detail": str(control_error),
                                    "raw": {"traceback": traceback.format_exc()},
                                }
                            )
                        else:
                            try:
                                qualify_normal_replay(
                                    installations[0],
                                    str(frozen_control["project"]),
                                    str(frozen_control["item"]),
                                    receipt,
                                )
                            except Exception as replay_error:  # noqa: BLE001 -- preserve the independent retention qualification failure
                                receipt.record(
                                    {
                                        "kind": "controlQualificationFailure",
                                        "case": "receipt-retention",
                                        "detail": str(replay_error),
                                        "raw": {"traceback": traceback.format_exc()},
                                    }
                                )
                except Exception as error:  # noqa: BLE001
                    results["forwarding"] = "failed"
                    receipt.record(
                        {
                            "kind": "caseFailure",
                            "case": "forwarding",
                            "detail": str(error),
                            "raw": {"traceback": traceback.format_exc()},
                        }
                    )
            for name, reason in recipient_gaps(installations, receipt).items():
                results[name] = "blocked"
                receipt.record({"kind": "gap", "case": name, "detail": reason})
            if all(item.plan.recipient_provider is not None for item in installations):
                recipients, gaps = qualify_recipients(
                    installations,
                    t.cast(JsonObject, plan.caller.model_dump(mode="json"))
                    if plan.caller
                    else None,
                    receipt,
                )
                if not gaps and len(recipients) == 2:
                    if context is None:
                        receipt.record(
                            {
                                "kind": "gap",
                                "detail": "Notification fixtures require a settled actual fleet workspace.",
                            }
                        )
                    else:
                        recipient_cases(
                            recipients,
                            list(installations),
                            context,
                            receipt,
                            results,
                            owner,
                            plan.recipient_cases,
                        )
            exit_code = (
                1
                if "failed" in results.values()
                else (
                    0 if all(value == "observed" for value in results.values()) else 2
                )
            )
    except Exception as error:  # noqa: BLE001 -- preserve setup failure and still stop exact owned processes
        receipt.record(
            {
                "kind": "failure",
                "detail": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
        exit_code = 1
    finally:
        for recipient in reversed(recipients):
            try:
                recipient.close()
            except Exception as error:  # noqa: BLE001
                receipt.record({"kind": "cleanupFailure", "detail": str(error)})
                exit_code = 1
        for installation in reversed(installations):
            try:
                installation.close()
            except Exception as error:  # noqa: BLE001
                receipt.record({"kind": "cleanupFailure", "detail": str(error)})
                exit_code = 1
        for edge in reversed(edges):
            try:
                edge.close()
            except Exception as error:  # noqa: BLE001 -- one cleanup failure must not skip other owners
                receipt.record({"kind": "cleanupFailure", "detail": str(error)})
                exit_code = 1
        if central is not None:
            try:
                central.close()
            except Exception as error:  # noqa: BLE001
                receipt.record({"kind": "cleanupFailure", "detail": str(error)})
                exit_code = 1
        if any(
            (record.get("kind") == "assertion" and record.get("passed") is False)
            or record.get("kind")
            in {
                "failure",
                "controlQualificationFailure",
                "caseFailure",
                "cleanupFailure",
            }
            for record in receipt.records
        ):
            exit_code = 1
        receipt.record({"kind": "caseResults", "results": t.cast(JsonObject, results)})
        receipt.record(
            {
                "kind": "result",
                "exit": exit_code,
                "readinessOnly": plan.mode == "readiness",
            }
        )
        receipt.close()
    return exit_code


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--plan", type=Path)
    _ = parser.add_argument("--list-cases", action="store_true")

    class Options(argparse.Namespace):
        plan: Path | None = None
        list_cases: bool = False

    arguments = parser.parse_args(namespace=Options())
    if arguments.list_cases:
        print(json.dumps(CASES, indent=2))
        return 0
    if arguments.plan is None:
        parser.error("--plan is required for launch and observation")
    return run_plan(CrossMachinePlan.model_validate_json(arguments.plan.read_text()))


if __name__ == "__main__":
    raise SystemExit(main())
