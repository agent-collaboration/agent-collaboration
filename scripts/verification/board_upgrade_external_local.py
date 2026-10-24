"""Qualify external OB5 against a disposable real central and fixture Access.

No Cargo, deployment, real enrollment or home-store access. Required prebuilt
signed artifact copies are pinned by one frozen-build receipt object.
Run python -m scripts.verification.board_upgrade_external_local --help.
"""

import argparse
import os
import shutil
import subprocess
import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict

from .board_central_runtime import CentralPlan, RealCentral, enroll_host
from .board_journey_runtime import BoardRuntime, JourneyReceipt
from .board_journey_target import JourneyTarget
from .board_journey_values import file_digest, fixture_identity
from .board_records_journey import require_private_parent
from .board_upgrade_external import observe_external
from .board_upgrade_external_plan import ExternalPlan, FrozenArtifacts
from .board_upgrade_external_target import require_short_socket_path
from .board_upgrade_views import json_object, text_field


class LocalArguments(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    repository: Path
    build_head: str
    artifacts_json: Path
    runtime_parent: Path
    output_directory: Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--repository", type=Path, required=True)
    _ = parser.add_argument("--build-head", required=True)
    _ = parser.add_argument("--artifacts-json", type=Path, required=True)
    _ = parser.add_argument("--runtime-parent", type=Path, required=True)
    _ = parser.add_argument("--output-directory", type=Path, required=True)
    args = LocalArguments.model_validate(vars(parser.parse_args()))
    repository = Path(str(args.repository)).resolve()
    parent = require_private_parent(
        Path(str(args.runtime_parent)).resolve(), repository
    )
    output = Path(str(args.output_directory)).resolve()
    receipt = JourneyReceipt(output)
    output.chmod(0o700)
    frozen = FrozenArtifacts.model_validate_json(args.artifacts_json.read_bytes())
    if (
        frozen.pin != args.build_head
        or frozen.artifactDirectory != frozen.artifactDirectory.resolve(strict=True)
        or frozen.artifactDirectory.stat().st_uid != os.getuid()
        or frozen.artifactDirectory.stat().st_mode & 0o077
    ):
        raise ValueError("frozen build pin or private artifact directory differs")
    entries = [artifact.model_dump(mode="json") for artifact in frozen.artifacts]
    artifacts = {
        Path(
            text_field(json_object.validate_python(item), "path")
        ).name: json_object.validate_python(item)
        for item in entries
    }
    for name in [
        "codex-router-board-upgrade",
        "board-upgrade-native-observer",
        "board-upgrade-central",
        "board-debug-runtime",
        "agent-collaboration",
    ]:
        artifact = artifacts[name]
        binary = Path(text_field(artifact, "path"))
        expected = frozen.artifactDirectory / name
        source_path = (
            repository
            / "target/debug"
            / (
                "examples/board-debug-runtime"
                if name == "board-debug-runtime"
                else name
            )
        )
        if (
            binary != expected
            or binary != binary.resolve(strict=True)
            or file_digest(binary) != artifact["sha256"]
            or artifact["sourcePin"] != args.build_head
            or artifact["sourcePath"] != str(source_path)
            or artifact["strictSignatureVerified"] is not True
        ):
            raise ValueError(
                "local qualification artifact differs from the declared build"
            )
        _ = subprocess.run(
            ["codesign", "--verify", "--strict", str(binary)],
            check=True,
            capture_output=True,
        )
    excluded = [
        "--",
        ".",
        ":(exclude)scripts/verification/**",
        ":(exclude).agents/skills/verify-codex-router/**",
    ]
    for command in [
        ["git", "diff", "--name-only", args.build_head, "HEAD", *excluded],
        ["git", "status", "--porcelain", "--untracked-files=all", *excluded],
    ]:
        if subprocess.check_output(command, cwd=repository):
            raise ValueError(
                "local qualification product source differs from the build pin"
            )
    target = JourneyTarget(
        repository, str(args.build_head), None, frozen.artifactDirectory
    )
    target.launcher = frozen.artifactDirectory / "board-debug-runtime"
    target.cli = frozen.artifactDirectory / "agent-collaboration"
    target.digests = {
        "launcherSha256": artifacts["board-debug-runtime"]["sha256"],
        "cliSha256": artifacts["agent-collaboration"]["sha256"],
    }
    target.require_unchanged_binaries()
    runtime = BoardRuntime(target, receipt, parent)
    require_short_socket_path(runtime.directory)
    central_plan = CentralPlan(
        repository=repository,
        expected_head=str(args.build_head),
        artifacts=Path(str(args.artifacts_json)),
        central_binary=Path(text_field(artifacts["board-upgrade-central"], "path")),
        runtime_parent=parent,
        output_directory=output,
        scenario="replication",
    )
    central: RealCentral | None = None
    edge = None
    environment_before: dict[str, str | None] = {}
    try:
        central = RealCentral(central_plan, receipt, [fixture_identity()])
        edge = enroll_host(central, runtime)
        runtime.stop()
        if runtime.fixture_session is None:
            raise ValueError("fixture enrollment did not supply a real session")
        session = json_object.validate_json(runtime.fixture_session.read_bytes())
        identity = json_object.validate_json(
            (runtime.directory / "service-identity.json").read_bytes()
        )
        machine = {
            "installation": identity["serviceId"],
            "peer": session["peer"],
            "receiver": session["receiver"],
            "relay": session["relay"],
            "credential": text_field(session, "credential").removeprefix("Bearer "),
            "access_client_id": json_object.validate_python(session["access"])[
                "clientId"
            ],
            "access_client_secret": json_object.validate_python(session["access"])[
                "clientSecret"
            ],
        }
        names = {key: "OB5_LOCAL_" + key.upper() for key in machine}
        for key, name in names.items():
            environment_before[name] = os.environ.get(name)
            os.environ[name] = str(machine[key])
        external = ExternalPlan(
            repository=repository,
            expected_head=str(args.build_head),
            artifact_receipt=args.artifacts_json,
            endpoint=central.ready.url,
            machine_endpoint=edge.endpoint,
            authentication="localFixture",
            fixture_assertion=central.assertion,
            machine_environment=names,
            runtime_parent=parent,
            output_directory=output / "external",
        )
        plan = output / "external-plan.json"
        _ = plan.write_text(external.model_dump_json(indent=2))
        return observe_external(plan)
    finally:
        failures: list[str] = []
        jobs: list[tuple[str, t.Callable[[], None]]] = [("fixtureHost", runtime.stop)]
        if edge is not None:
            jobs.append(("fixtureEdge", edge.close))
        if central is not None:
            jobs.append(("fixtureCentral", central.close))
        for label, close in jobs:
            try:
                close()
            except Exception as error:  # noqa: BLE001 -- settle every exact owner and retain sanitized cleanup evidence
                failures.append(label + ":" + type(error).__name__)
        for name, original in environment_before.items():
            if original is None:
                _ = os.environ.pop(name, None)
            else:
                os.environ[name] = original
        # This fixture Host owns only synthetic credentials and state. Its
        # consumers have been reaped above; keep the nonsecret lifecycle trace.
        if runtime.child is None:
            shutil.rmtree(runtime.root)
        if (
            central is not None
            and central.child.poll() is not None
            and central.assertion.exists()
        ):
            central.assertion.unlink()
        receipt.record(
            {
                "kind": "localQualificationCleanup",
                "fixtureHostDeleted": not runtime.root.exists(),
                "fixtureEdgeClosed": True,
                "fixtureCentralStopped": central is not None,
                "cleanupFailures": list(failures),
            }
        )
        receipt.close()
        if failures:
            raise RuntimeError(
                "local qualification cleanup failed; inspect lifecycle receipt"
            )


if __name__ == "__main__":
    raise SystemExit(main())
