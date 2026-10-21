"""Typed launch configuration for the declared public Sessions CUA target.

The existing fixed child adapter will own Popen and teardown. This module starts
nothing and never records inherited environment values. Real Host qualification
must supply the observed identity before the launch configuration is used.
"""

import argparse
import json
import os
import subprocess
import typing as t
from pathlib import Path
from uuid import UUID

from pydantic import Field, field_validator

from scripts.verification.owned_runtime import RuntimePlan, verify_ready
from scripts.verification.tui_capture import collect_cua_artifacts, require_one_test
from scripts.verification.tui_capture_lifecycle import discover_capture_test
from scripts.verification.tui_verification import (
    ArtifactBinding,
    CapturePlan,
    CuaPtyReadback,
    EvidenceModel,
    TerminalViewport,
    read_cua_state,
    verify_artifact,
)
from scripts.verification.typed_observations import HostManifest


class PublicHostIdentity(EvidenceModel):
    service_id: UUID
    service_epoch: UUID


class PublicSessionsTarget(EvidenceModel):
    binary: ArtifactBinding
    approved_cwd: Path
    debug_router_root: Path
    expected_host: PublicHostIdentity

    @field_validator("approved_cwd", "debug_router_root")
    @classmethod
    def absolute_directory(cls, value: Path) -> Path:
        if not value.is_absolute():
            raise ValueError("public Sessions directories must be absolute")
        return value

    def require_target(self, observed_host: PublicHostIdentity) -> None:
        if observed_host != self.expected_host:
            raise ValueError("public Sessions Host service/epoch mismatch")
        verify_artifact(self.binary)
        if not self.binary.path.is_file() or not os.access(self.binary.path, os.X_OK):
            raise ValueError("bound public Sessions executable required")
        for directory in (self.approved_cwd, self.debug_router_root):
            if not directory.is_dir() or directory.resolve(strict=True) != directory:
                raise ValueError("public Sessions directory binding changed")


def public_sessions_launch_configuration(
    target: PublicSessionsTarget,
    observed_host: PublicHostIdentity,
    inherited_environment: dict[str, str],
) -> tuple[list[str], dict[str, str]]:
    target.require_target(observed_host)
    environment = dict(inherited_environment)
    environment["CODEX_ROUTER_DEBUG_ROUTER_ROOT"] = str(target.debug_router_root)
    return [str(target.binary.path)], environment


def public_sessions_child_configuration(
    target: PublicSessionsTarget, observed_host: PublicHostIdentity, output: Path
) -> dict[str, str]:
    """Private fixed-adapter input; inherited environment is never serialized."""
    target.require_target(observed_host)
    if (
        not output.is_absolute()
        or output.is_symlink()
        or output.resolve() != output
        or not output.parent.is_dir()
        or (output.exists() and not output.is_dir())
    ):
        raise ValueError("public Sessions requires its owned output directory")
    return {
        "surface": "public-sessions",
        "binary": str(target.binary.path),
        "binary_sha256": target.binary.sha256,
        "approved_cwd": str(target.approved_cwd),
        "debug_router_root": str(target.debug_router_root),
        "expected_service_id": str(target.expected_host.service_id),
        "expected_service_epoch": str(target.expected_host.service_epoch),
        "output": str(output),
    }


class PublicSessionsPlan(CapturePlan):
    public_target: PublicSessionsTarget
    runtime_plan: RuntimePlan
    runtime_effect_evidence: ArtifactBinding

    def qualify_public_target(self) -> None:
        self.verify_target()
        if self.harness_binary != self.public_target.binary:
            raise ValueError("public CUA target executable binding mismatch")
        verify_artifact(self.runtime_effect_evidence)
        if self.runtime_plan.router_root != self.public_target.debug_router_root:
            raise ValueError("public Sessions runtime root mismatch")
        observed = verify_ready(self.runtime_plan)
        manifest = HostManifest.model_validate(observed["manifest"])
        self.public_target.require_target(
            PublicHostIdentity(
                service_id=UUID(manifest.serviceId),
                service_epoch=UUID(manifest.serviceEpoch),
            )
        )

    def capture_binding(self) -> CapturePlan:
        return CapturePlan.model_validate(
            {name: getattr(self, name) for name in CapturePlan.model_fields}
        )


class PublicSessionsLifecycle(EvidenceModel):
    kind: t.Literal["public-sessions-cua-lifecycle-only"]
    binary_sha256: str
    public_capture_source_sha256: str
    fixed_child_sha256: str
    host_service_id: UUID
    host_service_epoch: UUID
    child_exit: t.Literal[0]
    owned_processes_stopped: t.Literal[True]
    artifact_sink_stopped: t.Literal[True]
    artifact_count: int = Field(strict=True, ge=0)
    last_viewport: str
    effect_evidence_sha256: str
    job_settlement: t.Literal["separate-runtime-helper-proof-required"]


def prepare_public_capture(plan: PublicSessionsPlan) -> dict[str, str]:
    """Actual Host qualification precedes every effect; never rewrites normal homes."""
    plan.qualify_public_target()
    if not plan.cua_artifact_scenes:
        raise ValueError("public capture needs exact declared original scenes")
    selector = "compiled_compact_current_storyboard_exports_both_real_tuis"
    require_one_test(
        discover_capture_test(
            [str(plan.capture_driver.path), selector, "--exact", "--list"],
            plan.repository,
        ),
        selector,
    )
    plan.output.mkdir(mode=0o700, exist_ok=False)
    configuration = public_sessions_child_configuration(
        plan.public_target,
        plan.public_target.expected_host,
        plan.output / "cua-sessions",
    )
    configuration.update(
        ttyd=str(plan.ttyd.path),
        temporary_directory=str(plan.temporary_directory),
        dynamic_library_path=plan.dynamic_library_path,
    )
    target_file = plan.output / "public-target.json"
    _ = target_file.write_text(json.dumps(configuration))
    target_file.chmod(0o600)
    _ = (plan.output / "declared-target.json").write_text(
        plan.model_dump_json(indent=2)
    )
    return {
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG": str(target_file),
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG_SHA256": ArtifactBinding.capture(
            target_file
        ).sha256,
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_BINARY": str(plan.public_target.binary.path),
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_SERVICE_ID": str(
            plan.public_target.expected_host.service_id
        ),
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_SERVICE_EPOCH": str(
            plan.public_target.expected_host.service_epoch
        ),
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_EFFECT_SHA256": plan.runtime_effect_evidence.sha256,
    }


def run_public_capture(plan: PublicSessionsPlan) -> None:
    launch = prepare_public_capture(plan)
    environment = dict(os.environ)
    for key in (
        "QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY",
        "QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO",
    ):
        _ = environment.pop(key, None)
    environment.update(launch)
    environment.update(
        {
            "QUOTA_STORYBOARD_CUA_SURFACE": "sessions",
            "QUOTA_STORYBOARD_CUA_ARTIFACT_SCENES": ",".join(plan.cua_artifact_scenes),
            "QUOTA_STORYBOARD_OUTPUT": str(plan.output),
            "QUOTA_STORYBOARD_VHS": str(plan.vhs.path),
            "QUOTA_STORYBOARD_TTYD": str(plan.ttyd.path),
            "QUOTA_STORYBOARD_IMAGE_VERIFIER": str(plan.image_verifier.path),
            "QUOTA_STORYBOARD_DYLD_LIBRARY_PATH": plan.dynamic_library_path,
            "TMPDIR": str(plan.temporary_directory),
        }
    )
    command = [
        str(plan.capture_driver.path),
        "compiled_compact_current_storyboard_exports_both_real_tuis",
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]
    _ = (plan.output / "command.json").write_text(json.dumps(command))
    with (
        (plan.output / "driver.stdout").open("w") as stdout,
        (plan.output / "driver.stderr").open("w") as stderr,
    ):
        child = subprocess.Popen(
            command,
            cwd=plan.repository,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=stdout,
            stderr=stderr,
        )
        timed_out = False
        try:
            result = child.wait(timeout=660)
        except subprocess.TimeoutExpired:
            timed_out = True
            # Rust owns the wrapper/PTY/browser. A request is not settlement and
            # never signals the separately helper-owned native/provider job.
            stop = plan.output / "cua-sessions/cua-stop"
            with stop.open("x") as stream:
                _ = stream.write("stop\n")
            try:
                result = child.wait(timeout=90)
            except subprocess.TimeoutExpired:
                result = None
        _ = (plan.output / "driver-exit.json").write_text(
            json.dumps(
                {
                    "pid": child.pid,
                    "exit": result,
                    "deadline_reached": timed_out,
                    "driver_settled": result is not None,
                }
            )
        )
    if timed_out or result != 0:
        raise ValueError(
            "public CUA failed or cleanup unsettled; retain evidence, do not replay"
        )
    plan.qualify_public_target()
    receipt = PublicSessionsLifecycle.model_validate_json(
        (plan.output / "cua-sessions/public-cua-receipt.json").read_bytes()
    )
    support = (
        plan.repository
        / "crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test"
    )
    if (
        receipt.binary_sha256 != plan.public_target.binary.sha256
        or receipt.public_capture_source_sha256
        != ArtifactBinding.capture(support / "public_sessions_capture.rs").sha256
        or receipt.fixed_child_sha256
        != ArtifactBinding.capture(support / "fixed_quota_child.py").sha256
        or receipt.host_service_id != plan.public_target.expected_host.service_id
        or receipt.host_service_epoch != plan.public_target.expected_host.service_epoch
        or receipt.effect_evidence_sha256 != plan.runtime_effect_evidence.sha256
    ):
        raise ValueError("public CUA lifecycle source/Host/effect binding mismatch")
    _ = (plan.output / "public-cua-summary.json").write_text(
        json.dumps(
            {
                "target": plan.model_dump(mode="json"),
                "receipt": receipt.model_dump(mode="json"),
                "original_artifacts": collect_cua_artifacts(
                    plan, "sessions", receipt.artifact_count
                ),
                "complete": False,
                "limits": "UI originals and actual Host/job effects/settlement require separate observation; no fixture zero-effect claim.",
            },
            indent=2,
        )
    )


def read_public_cua_state(
    plan: PublicSessionsPlan, viewport: TerminalViewport
) -> CuaPtyReadback:
    plan.qualify_public_target()
    return read_cua_state(plan.capture_binding(), "sessions", viewport)


class PublicCaptureArguments(argparse.Namespace):
    plan: Path = Path()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("plan", type=Path)
    arguments = parser.parse_args(namespace=PublicCaptureArguments())
    run_public_capture(
        PublicSessionsPlan.model_validate_json(arguments.plan.read_bytes())
    )


if __name__ == "__main__":
    main()
