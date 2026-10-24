"""Run and reap the actual importer; retain its reports and source/identity guards."""

import json
import os
import shutil
import subprocess
import time
import typing as t
from dataclasses import dataclass
from pathlib import Path

from .board_journey_values import file_digest
from .board_upgrade_cli import fingerprint
from .board_upgrade_native_oracle import Checks
from .board_upgrade_publication_runtime import CompleteGate, OwnedCentral
from .board_upgrade_publication_views import (
    PublicationPlan,
    Uploaded,
    publication_report,
)
from .board_upgrade_views import (
    DECLARED_OWNER,
    DECLARED_PROVENANCE,
    DryRun,
    Object,
    json_object,
)


@dataclass(frozen=True)
class PublicationEndpoint:
    url: str
    assertion_file: Path


def verify_target(
    plan: PublicationPlan,
    *,
    check_source: bool = True,
    binary_directory: Path | None = None,
) -> None:
    current = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=plan.repository,
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()
    if check_source and current != plan.expected_head:
        raise ValueError("declared source HEAD differs")
    dirty = subprocess.run(
        [
            "git",
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ".",
            ":(exclude)scripts/verification/**",
            ":(exclude).agents/skills/verify-codex-router/**",
        ],
        cwd=plan.repository,
        text=True,
        capture_output=True,
        check=True,
    ).stdout
    if check_source and dirty:
        raise ValueError(f"product source is dirty: {dirty}")
    target = (binary_directory or plan.repository / "target/debug").resolve()
    for path, digest in [
        (plan.binary, plan.binary_sha256),
        (plan.central_binary, plan.central_sha256),
    ]:
        if (
            path != path.resolve(strict=True)
            or path.parent != target
            or file_digest(path) != digest
        ):
            raise ValueError(
                "declared debug executable/hash escaped its qualified directory or changed"
            )


@t.final
class PublicationCli:
    def __init__(
        self,
        plan: PublicationPlan,
        checks: Checks,
        originals: dict[str, Object],
        *,
        check_source: bool = True,
        binary_directory: Path | None = None,
        pre_signed: bool = False,
    ) -> None:
        self.plan = plan
        self.checks = checks
        self.originals = originals
        self.check_source = check_source
        self.binary_directory = binary_directory
        self.pre_signed = pre_signed
        self.commands: list[Object] = []
        self.active: subprocess.Popen[bytes] | None = None
        self.root = plan.output_directory
        self.temporary = self.root / "cli-temporary"
        self.temporary.mkdir(mode=0o700)
        self.identity = self.root / "identity"
        self.identity.mkdir(mode=0o700)
        self.identity_file = self.identity / "service-identity.json"
        _ = self.identity_file.write_text(
            json.dumps({"version": 1, "serviceId": DECLARED_PROVENANCE["installation"]})
        )
        self.identity_file.chmod(0o600)
        self.identity_before = fingerprint(self.identity_file)

    def run(
        self,
        case: Path,
        label: str,
        source: Path,
        central: OwnedCentral | PublicationEndpoint | None = None,
        *,
        gate: CompleteGate | None = None,
        relabel: bool = False,
        machine_label: str = "verification source",
        expected_exit: int = 0,
        additional_sources: tuple[Path, ...] = (),
    ) -> DryRun | Uploaded | None:
        verify_target(
            self.plan,
            check_source=self.check_source,
            binary_directory=self.binary_directory,
        )
        report = case / f"{label}-report.json"
        args = (
            []
            if self.pre_signed
            else [str(self.plan.repository / "scripts/cargo_debug_signing_runner.sh")]
        ) + [
            str(self.plan.binary),
            "--source",
            str(source),
            "--router-directory",
            str(self.identity),
            "--report",
            str(report),
            "--owner-human-id",
            "second-importer" if relabel else DECLARED_OWNER,
            "--machine-label",
            "relabeled verification source" if relabel else machine_label,
        ]
        for additional in additional_sources:
            args.extend(["--source", str(additional)])
        environment = dict(
            os.environ, TMPDIR=str(self.temporary), PYTHONDONTWRITEBYTECODE="1"
        )
        _ = environment.pop("CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE", None)
        if central is None:
            args.append("--dry-run")
        else:
            args.extend(["--central-url", gate.url if gate else central.url])
            environment["CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE"] = str(
                central.assertion_file
            )
        with (
            (case / f"{label}.stdout").open("wb") as stdout,
            (case / f"{label}.stderr").open("wb") as stderr,
        ):
            process = subprocess.Popen(
                args,
                cwd=self.plan.repository,
                env=environment,
                stdout=stdout,
                stderr=stderr,
            )
            self.active = process
            try:
                if gate:
                    deadline = time.monotonic() + self.plan.timeout_seconds
                    while not gate.state.reached.wait(0.05):
                        if process.poll() is not None:
                            raise AssertionError(
                                "CLI exited before the selected publication checkpoint"
                            )
                        if time.monotonic() >= deadline:
                            raise TimeoutError("CLI did not reach the owned HTTP gate")
                    self.checks.equal(
                        "HTTP checkpoint has no driver error", None, gate.state.failure
                    )
                    self.checks.equal(
                        "CLI is live at actual wire checkpoint", None, process.poll()
                    )
                    if gate.state.checkpoint is None:
                        raise AssertionError("actual checkpoint request missing")
                    _ = (case / "wire-checkpoint.json").write_text(
                        json.dumps(gate.state.checkpoint, indent=2)
                    )
                    if gate.state.phase == "after":
                        response = json_object.validate_python(
                            gate.state.checkpoint["response"]
                        )
                        self.checks.equal(
                            "held reply confirms actual publication",
                            "imported",
                            response["kind"],
                        )
                    process.terminate()
                    code = process.wait(timeout=10)
                    self.checks.equal("owned CLI interrupted by signal", True, code < 0)
                    self.checks.equal(
                        "interrupted CLI has no completion report",
                        False,
                        report.exists(),
                    )
                else:
                    code = process.wait(timeout=self.plan.timeout_seconds)
                    self.checks.equal(label + " CLI exit", expected_exit, code)
            finally:
                if process.poll() is None:
                    process.kill()
                    _ = process.wait(timeout=10)
                self.commands.append(
                    json_object.validate_python(
                        {"label": label, "arguments": args, "exit": process.returncode}
                    )
                )
                self.active = None
        self.checks.equal(
            label + " source bytes/mtime unchanged",
            self.originals[str(source)],
            fingerprint(source),
        )
        self.checks.equal(
            label + " existing identity unchanged",
            self.identity_before,
            fingerprint(self.identity_file),
        )
        for additional in additional_sources:
            self.checks.equal(
                label + " additional source bytes/mtime unchanged",
                self.originals[str(additional)],
                fingerprint(additional),
            )
        if gate:
            residue = sorted(
                str(path.relative_to(self.temporary))
                for path in self.temporary.rglob("*")
                if path.is_file()
            )
            _ = (case / "interrupted-cli-temporary-files.json").write_text(
                json.dumps(residue, indent=2)
            )
            # This fresh directory belongs only to the now-reaped CLI. Logs, wire
            # checkpoint and durable central readback live outside it.
            shutil.rmtree(self.temporary)
            self.temporary.mkdir(mode=0o700)
            return None
        self.checks.equal(
            label + " disposable files closed",
            [],
            sorted(path.name for path in self.temporary.iterdir()),
        )
        raw = json_object.validate_json(report.read_bytes())
        if expected_exit == 2:
            self.checks.equal(label + " refused report", "refused", raw["kind"])
            return None
        self.checks.equal(
            label + " report kind", "uploaded" if central else "dryRun", raw["kind"]
        )
        parsed = publication_report.validate_python(raw)
        if parsed.kind == "refused":
            raise AssertionError("valid supplied source was refused")
        return parsed

    def close(self) -> None:
        if self.active is not None and self.active.poll() is None:
            raise RuntimeError("owned CLI still uses its temporary state; retaining it")
        residue = sorted(
            str(path.relative_to(self.temporary))
            for path in self.temporary.rglob("*")
            if path.is_file()
        )
        _ = (self.root / "cli-cleanup.json").write_text(
            json.dumps(
                {"temporaryFilesRemoved": residue, "allOwnedCliProcessesReaped": True},
                indent=2,
            )
        )
        if self.temporary.exists():
            shutil.rmtree(self.temporary)
