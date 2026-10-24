"""Pin external OB5 product source, signed binaries and normal artifact locations."""

import os
import subprocess
from pathlib import Path

from .board_journey_values import file_digest
from .board_records_journey import require_private_parent
from .board_upgrade_external_plan import ExternalPlan
from .board_upgrade_views import Object, json_object


def require_short_socket_path(directory: Path) -> None:
    if len(os.fsencode(directory / "control.sock")) >= 104:
        raise ValueError(
            "Host socket path exceeds macOS limit; use a short private /private/tmp parent"
        )


def verify_external_source(plan: ExternalPlan) -> None:
    """Apparatus-only commits may advance HEAD; product bytes must match the build."""
    excluded = [
        "--",
        ".",
        ":(exclude)scripts/verification/**",
        ":(exclude).agents/skills/verify-codex-router/**",
    ]
    for command in [
        ["git", "diff", "--name-only", plan.expected_head, "HEAD", *excluded],
        ["git", "status", "--porcelain", "--untracked-files=all", *excluded],
    ]:
        if subprocess.check_output(command, cwd=plan.repository):
            raise ValueError("product source differs from the declared build pin")


def qualify_external_target(plan: ExternalPlan) -> Object:
    verify_external_source(plan)
    plan.validate_endpoint()
    _ = require_private_parent(plan.runtime_parent, plan.repository)
    _ = require_private_parent(plan.output_directory.parent, plan.repository)
    if (
        plan.receipt.pin != plan.expected_head
        or plan.artifacts != plan.artifacts.resolve(strict=True)
        or plan.artifacts.stat().st_uid != os.getuid()
        or plan.artifacts.stat().st_mode & 0o077
    ):
        raise ValueError(
            "frozen artifact receipt differs from the declared pin/private directory"
        )
    bindings = [
        (plan.binary, plan.binary_sha256),
        (plan.native_binary, plan.native_sha256),
        (
            plan.artifact("board-debug-runtime").path,
            plan.launcher_sha256,
        ),
        (plan.artifact("agent-collaboration").path, plan.cli_sha256),
    ]
    for path, digest in bindings:
        expected_parent = plan.artifacts
        declared = plan.artifact(path.name)
        source_path = (
            plan.repository
            / "target/debug"
            / (
                "examples/board-debug-runtime"
                if path.name == "board-debug-runtime"
                else path.name
            )
        )
        if (
            path != path.resolve(strict=True)
            or path.parent != expected_parent
            or file_digest(path) != digest
            or declared.sourcePath != source_path
            or declared.sourcePin != plan.expected_head
            or not declared.strictSignatureVerified
            or declared.team != "974QD84WVC"
            or declared.identifier != "dev.shravansunder." + path.name + ".debug"
        ):
            raise ValueError("declared binary changed")
        _ = subprocess.run(
            ["codesign", "--verify", "--strict", str(path)],
            check=True,
            capture_output=True,
        )
        signature = subprocess.run(
            ["codesign", "-dv", "--verbose=4", str(path)],
            capture_output=True,
            text=True,
            check=True,
        ).stderr
        if (
            "Identifier=" + declared.identifier not in signature
            or "TeamIdentifier=" + declared.team not in signature
        ):
            raise ValueError("actual debug signature differs from frozen receipt")
    # Record both identities when only apparatus commits have advanced HEAD.
    _ = subprocess.run(
        ["git", "cat-file", "-e", plan.expected_head + "^{commit}"],
        cwd=plan.repository,
        check=True,
    )
    return {
        "declaredBuildHead": plan.expected_head,
        "workingHead": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=plan.repository, text=True
        ).strip(),
        "workingStatus": subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=plan.repository, text=True
        ),
        "plan": json_object.validate_json(plan.model_dump_json()),
        "binaryBindings": [
            {"path": str(path), "sha256": digest} for path, digest in bindings
        ],
    }
