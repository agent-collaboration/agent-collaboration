"""Run source-pinned central conformance on Tokio or local workerd.

Uses retained one-core conformance entrypoints, never an installed Router.
Logs and executable hashes are private evidence. Legacy selectors are explicitly
retired; selected current scenarios do not establish their full equivalence.
Run from the repository root; --help lists the required isolated paths.
"""

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import typing as t
from datetime import UTC, datetime
from pathlib import Path

from pydantic import BaseModel, ConfigDict

from scripts.verification.central_worker_scenarios import (
    DEFAULT_WORKERD_MODES,
    RETIRED_NATIVE_TARGETS,
    RETIRED_WORKERD_SCENARIOS,
    WORKERD_SCENARIOS,
    WorkerdObservation,
    check_workerd_output,
    read_owner_retirement_state,
    retired_scenarios,
    worker_build_command,
    workerd_command,
    workerd_scenarios,
)

SOURCE_PATHS = ("Cargo.toml", "Cargo.lock", ".cargo", "crates", "scripts/tooling")
BUILD_TARGET = Path(__file__).resolve().parents[2] / "target"
SUMMARY_PATTERN = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
    + r"(?:[ \t]+\d+ measured;[ \t]+(\d+) filtered out;)?",
    re.MULTILINE,
)


class TestCounts(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True)
    passed: int
    failed: int
    ignored: int
    targets: int
    filtered_out: int = 0


class ExecutableEvidence(BaseModel):
    path: str
    sha256: str


class RunArguments(BaseModel):
    expected_head: str
    evidence: Path
    purpose: t.Literal["baseline-characterization", "declared-target"]
    repository: Path = Path.cwd()
    source_archive: Path | None = None


class RuntimeArguments(RunArguments):
    runtime: t.Literal["tokio", "workerd"]
    test_target: list[str] | None = None
    worker_build: Path | None = None
    miniflare: Path | None = None
    port: int = 8957
    package: str = "collaboration-central-conformance"
    features: str | None = None
    test_name: str | None = None
    workerd_mode: list[str] | None = None
    delivery_relay_peer: str | None = None


class PackageVersion(BaseModel):
    name: str
    version: str


class WorkerdBinding(BaseModel):
    version: str
    executable: Path


class CommandEvidence(BaseModel):
    argv: list[str]
    exit_code: int
    log: str
    binaries: list[ExecutableEvidence]


class RunEvidence(BaseModel):
    source_head: str
    repository: str
    source_archive: ExecutableEvidence | None = None
    runtime: t.Literal["tokio", "workerd", "staging-build-only"]
    purpose: t.Literal["baseline-characterization", "declared-target"]
    started_at: str
    commands: list[CommandEvidence] = []
    retired_scenarios: dict[str, str] = {}
    gaps: list[str]
    conclusion: t.Literal["incomplete", "completed", "failed"] = "incomplete"
    detail: str = ""


def file_evidence(path: Path) -> ExecutableEvidence:
    with path.open("rb") as content:
        digest = hashlib.file_digest(content, "sha256").hexdigest()
    return ExecutableEvidence(path=str(path.resolve()), sha256=digest)


def check_source(
    repository: Path, expected_head: str, archive: Path | None = None
) -> None:
    if not re.fullmatch(r"[0-9a-f]{40}", expected_head):
        raise ValueError("expected head must be a complete lowercase commit hash")
    if archive is not None:
        with archive.open("rb") as source:
            archived_head = subprocess.check_output(
                ["git", "get-tar-commit-id"], stdin=source, text=True
            ).strip()
        if archived_head != expected_head:
            raise ValueError("archive commit differs from declared source")
        archived_paths: set[str] = set()
        with tarfile.open(archive) as contents:
            for member in contents.getmembers():
                if not member.isfile() or not any(
                    member.name == prefix or member.name.startswith(prefix + "/")
                    for prefix in SOURCE_PATHS
                ):
                    continue
                archived_paths.add(member.name)
                saved = contents.extractfile(member)
                if saved is None:
                    raise ValueError(f"archive file unavailable: {member.name}")
                with saved:
                    expected = hashlib.sha256(saved.read()).hexdigest()
                if file_evidence(repository / member.name).sha256 != expected:
                    raise ValueError(f"archived source changed: {member.name}")
        generated_bundle = "crates/collaboration-central-cloudflare/build/"
        for prefix in SOURCE_PATHS:
            source_path = repository / prefix
            candidates = (
                source_path.rglob("*") if source_path.is_dir() else [source_path]
            )
            for candidate in candidates:
                relative = candidate.relative_to(repository).as_posix()
                if (
                    candidate.is_file()
                    and relative not in archived_paths
                    and not relative.startswith(generated_bundle)
                ):
                    raise ValueError(f"extra archived source: {relative}")
        return
    observed = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=repository, text=True
    ).strip()
    dirty = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=all", "--", *SOURCE_PATHS],
        cwd=repository,
        text=True,
    )
    if observed != expected_head or dirty:
        raise ValueError(
            f"source target changed: head={observed}; product changes={dirty!r}"
        )


def read_test_counts(output: str, expected_targets: int) -> TestCounts:
    summaries = [
        (
            match.group(1),
            int(match.group(2)),
            int(match.group(3)),
            int(match.group(4)),
            int(match.group(5) or 0),
        )
        for match in SUMMARY_PATTERN.finditer(output)
    ]
    if len(summaries) != expected_targets:
        raise ValueError(
            f"expected {expected_targets} test targets, observed {len(summaries)}"
        )
    if any(
        state != "ok" or passed == 0 or failed != 0
        for state, passed, failed, _, _ in summaries
    ):
        raise ValueError("failed or zero-test target is not a passing observation")
    return TestCounts(
        passed=sum(row[1] for row in summaries),
        failed=sum(row[2] for row in summaries),
        ignored=sum(row[3] for row in summaries),
        targets=len(summaries),
        filtered_out=sum(row[4] for row in summaries),
    )


def native_command(
    targets: tuple[str, ...],
    package: str = "collaboration-central-conformance",
    features: str | None = None,
    test_name: str | None = None,
) -> list[str]:
    if package == "collaboration-central-conformance":
        for target in targets:
            if target in RETIRED_NATIVE_TARGETS:
                raise ValueError(f"{target}: {RETIRED_NATIVE_TARGETS[target]}")
    if not targets or any(
        not re.fullmatch(r"[a-z][a-z0-9_]*", name) for name in targets
    ):
        raise ValueError("test targets must be explicit Cargo integration target names")
    if package not in (
        "collaboration-central-conformance",
        "collaboration-central-tokio",
        "collaboration-central-core",
    ):
        raise ValueError(
            "only central core, conformance or Tokio packages are in this journey"
        )
    command = ["cargo", "test", "--locked", "-p", package]
    if features is not None:
        if not re.fullmatch(r"[a-z][a-z0-9_-]*(,[a-z][a-z0-9_-]*)*", features):
            raise ValueError("features must be explicit Cargo feature names")
        command.extend(["--features", features])
    for target in targets:
        command.extend(["--test", target])
    if test_name is not None:
        if not re.fullmatch(
            r"[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*", test_name
        ):
            raise ValueError("test name must be one exact Rust test identity")
        return [*command, test_name, "--", "--exact", "--nocapture"]
    return [*command, "--", "--nocapture"]


def run_command(
    argv: list[str], evidence: Path, label: str, record: RunEvidence
) -> str:
    repository = Path(record.repository)
    archive = Path(record.source_archive.path) if record.source_archive else None
    if (
        record.source_archive
        and file_evidence(Path(record.source_archive.path)).sha256
        != record.source_archive.sha256
    ):
        raise ValueError("source archive identity changed")
    check_source(repository, record.source_head, archive)
    disk = subprocess.run(
        ["df", "-h", str(evidence)], check=True, capture_output=True, text=True
    )
    _ = (evidence / f"{label}-disk.txt").write_text(disk.stdout)
    if shutil.disk_usage(evidence).free < 45 * 1024**3:
        raise ValueError("less than 45 GiB free; build/run stopped")
    environment = os.environ | {
        "CARGO_PROFILE_DEV_DEBUG": "0",
        "CARGO_PROFILE_TEST_DEBUG": "0",
        "CARGO_INCREMENTAL": "0",
        "CARGO_BUILD_JOBS": "4",
        "CARGO_TERM_COLOR": "never",
        "CARGO_TARGET_DIR": str(BUILD_TARGET),
        "TMPDIR": str(evidence / "runtime"),
        "CC": "/usr/bin/clang",
        "CXX": "/usr/bin/clang++",
    }
    log = evidence / f"{label}.log"
    print(f"{label}: {argv!r}; log={log}", flush=True)
    with log.open("w") as output:
        result = subprocess.run(
            argv,
            check=False,
            cwd=repository,
            env=environment,
            stdout=output,
            stderr=subprocess.STDOUT,
        )
    content = log.read_text(errors="replace")
    binaries: list[ExecutableEvidence] = []
    for match in re.finditer(
        r"^\s*Running .*?\(([^\n)]+)\)\s*$", content, re.MULTILINE
    ):
        executable = repository / match.group(1)
        if executable.is_file():
            binaries.append(file_evidence(executable))
    record.commands.append(
        CommandEvidence(
            argv=argv, exit_code=result.returncode, log=str(log), binaries=binaries
        )
    )
    save_record(evidence, record)
    if (
        record.source_archive
        and file_evidence(Path(record.source_archive.path)).sha256
        != record.source_archive.sha256
    ):
        raise ValueError("source archive identity changed during command")
    check_source(repository, record.source_head, archive)
    if result.returncode != 0:
        raise ValueError(f"{label} exited {result.returncode}; inspect {log}")
    return content


def save_record(evidence: Path, record: RunEvidence) -> None:
    _ = (evidence / "run.json").write_text(record.model_dump_json(indent=2) + "\n")


def prepare_run(
    arguments: RunArguments,
    runtime: t.Literal["tokio", "workerd", "staging-build-only"],
    gaps: list[str],
) -> tuple[Path, RunEvidence]:
    expected_head: str = arguments.expected_head
    destination: Path = arguments.evidence
    evidence = destination.resolve()
    repository = arguments.repository.resolve()
    if not destination.is_absolute() or evidence.is_relative_to(repository):
        raise ValueError(
            "evidence must be an absolute task-owned directory outside the checkout"
        )
    check_source(repository, expected_head, arguments.source_archive)
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    (evidence / "runtime").mkdir(mode=0o700)
    record = RunEvidence(
        source_head=expected_head,
        repository=str(repository),
        source_archive=file_evidence(arguments.source_archive)
        if arguments.source_archive
        else None,
        runtime=runtime,
        purpose=arguments.purpose,
        started_at=datetime.now(UTC).isoformat(),
        gaps=gaps,
        retired_scenarios=retired_scenarios(),
    )
    save_record(evidence, record)
    _ = (evidence / "apparatus.json").write_text(
        file_evidence(Path(__file__)).model_dump_json(indent=2) + "\n"
    )
    _ = (evidence / "apparatus-modules.json").write_text(
        "[\n"
        + ",\n".join(
            file_evidence(path).model_dump_json()
            for path in (
                Path(__file__),
                Path(__file__).with_name("central_worker_scenarios.py"),
            )
        )
        + "\n]\n"
    )
    return evidence, record


def add_run_arguments(parser: argparse.ArgumentParser) -> None:
    _ = parser.add_argument(
        "--expected-head", required=True, help="complete declared source commit"
    )
    _ = parser.add_argument(
        "--evidence",
        required=True,
        type=Path,
        help="fresh private directory outside checkout",
    )
    _ = parser.add_argument(
        "--purpose",
        choices=("baseline-characterization", "declared-target"),
        required=True,
    )
    _ = parser.add_argument(
        "--repository",
        type=Path,
        default=Path.cwd(),
        help="source root; default current checkout",
    )
    _ = parser.add_argument(
        "--source-archive",
        type=Path,
        help="git archive tar for an isolated baseline snapshot",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    add_run_arguments(parser)
    _ = parser.add_argument("--runtime", choices=("tokio", "workerd"), required=True)
    _ = parser.add_argument(
        "--package",
        default="collaboration-central-conformance",
        help="native central package",
    )
    _ = parser.add_argument("--features", help="comma-separated native Cargo features")
    _ = parser.add_argument(
        "--test-name", help="one exact Rust test identity; enables --exact"
    )
    _ = parser.add_argument(
        "--workerd-mode",
        action="append",
        choices=(*WORKERD_SCENARIOS, *RETIRED_WORKERD_SCENARIOS),
        help="current mode(s); default composed-transactions and composed-numeric; retired modes fail explicitly",
    )
    _ = parser.add_argument(
        "--delivery-relay-peer",
        help="explicit UUIDv7 deployment relay required by the delivery-config Worker mode",
    )
    _ = parser.add_argument(
        "--test-target",
        action="append",
        help="explicit native integration target; former defaults retired by decision 508",
    )
    _ = parser.add_argument(
        "--worker-build", type=Path, help="task-owned worker-build 0.8.7 executable"
    )
    _ = parser.add_argument(
        "--miniflare",
        type=Path,
        help="task-owned Miniflare 5.20261012.0-beta dist/src/index.js",
    )
    _ = parser.add_argument(
        "--port",
        type=int,
        default=8957,
        help="free loopback port for the owned harness",
    )
    arguments = RuntimeArguments.model_validate(vars(parser.parse_args()))
    runtime = arguments.runtime
    gaps = [
        "Selected current scenarios do not prove equivalence to retired legacy aggregates.",
        "Local G18 at cb930a01 was accepted in 609; normal Worker session/holder coverage depends on the explicitly selected modes, and hosted panic/V3d remains separate.",
        "Unselected scenarios and remaining 469/473/476/497/501/502 obligations are not observed by this run.",
        "No staging, physical eviction, real IO failure or blocking-fsync observation.",
        "Conformance is not structural proof of one-core ownership or OB8.",
    ]
    evidence, record = prepare_run(arguments, runtime, gaps)
    try:
        if runtime == "tokio":
            if not arguments.test_target:
                raise ValueError(
                    "legacy native defaults retired by decision 508; supply --test-target explicitly"
                )
            targets = tuple(arguments.test_target)
            output = run_command(
                native_command(
                    targets, arguments.package, arguments.features, arguments.test_name
                ),
                evidence,
                "tokio-tests",
                record,
            )
            counts = read_test_counts(output, len(targets))
            _ = (evidence / "test-counts.json").write_text(
                counts.model_dump_json(indent=2) + "\n"
            )
        else:
            worker_build: Path | None = arguments.worker_build
            miniflare: Path | None = arguments.miniflare
            if worker_build is None or miniflare is None:
                raise ValueError(
                    "workerd requires explicit task-owned --worker-build and --miniflare"
                )
            modes = tuple(arguments.workerd_mode or DEFAULT_WORKERD_MODES)
            run_workerd(
                worker_build,
                miniflare,
                arguments.port,
                evidence,
                record,
                modes,
                delivery_relay_peer=arguments.delivery_relay_peer,
            )
        record.conclusion = "completed"
        record.detail = "Selected existing scenarios completed; listed obligations remain unverified."
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        record.conclusion = "failed"
        record.detail = str(error)
        print(str(error), file=sys.stderr)
    save_record(evidence, record)
    return 0 if record.conclusion == "completed" else 1


def run_workerd(
    worker_build: Path,
    miniflare: Path,
    port: int,
    evidence: Path,
    record: RunEvidence,
    modes: tuple[str, ...] = DEFAULT_WORKERD_MODES,
    *,
    delivery_relay_peer: str | None = None,
) -> None:
    selected = workerd_scenarios(modes)
    if not 1024 <= port <= 65535:
        raise ValueError("workerd port must be unprivileged")
    version = run_command(
        [str(worker_build), "--version"], evidence, "worker-build-version", record
    )
    if version.strip() != "0.8.7":
        raise ValueError("worker-build must be 0.8.7")
    package = miniflare.resolve().parents[2] / "package.json"

    pinned = PackageVersion.model_validate_json(package.read_text())
    if (pinned.name, pinned.version) != ("miniflare", "5.20261012.0-beta"):
        raise ValueError("Miniflare must be 5.20261012.0-beta")
    node = shutil.which("node")
    if node is None:
        raise ValueError("Node executable unavailable")
    _ = run_command([node, "--version"], evidence, "node-version", record)
    binding_json = run_command(
        [
            node,
            "-e",
            "const r=require('node:module').createRequire(process.argv[1]);"
            + "const w=r('workerd');console.log(JSON.stringify({version:w.version,executable:w.default}));",
            str(miniflare.resolve()),
        ],
        evidence,
        "workerd-binding",
        record,
    )
    binding = WorkerdBinding.model_validate_json(binding_json)
    if binding.version != "1.20261012.1":
        raise ValueError("workerd must be 1.20261012.1")
    _ = run_command(
        [str(binding.executable), "--version"], evidence, "workerd-version", record
    )
    _ = (evidence / "tools.json").write_text(
        "[\n"
        + ",\n".join(
            file_evidence(path).model_dump_json()
            for path in (
                worker_build,
                miniflare,
                package,
                Path(node),
                binding.executable,
            )
        )
        + "\n]\n"
    )
    scenario_commands = {
        mode: workerd_command(
            mode,
            node,
            miniflare,
            evidence / f"state-{mode}",
            port,
            delivery_relay_peer=delivery_relay_peer,
        )
        for mode, _ in selected
    }
    _ = run_command(
        worker_build_command(worker_build),
        evidence,
        "worker-build",
        record,
    )
    build = Path(record.repository) / "crates/collaboration-central-cloudflare/build"
    _ = (evidence / "worker-bundle.json").write_text(
        "[\n"
        + ",\n".join(
            file_evidence(build / name).model_dump_json()
            for name in ("index.js", "index_bg.wasm")
        )
        + "\n]\n"
    )
    observations: list[WorkerdObservation] = []
    for mode, scenario in selected:
        output = run_command(
            scenario_commands[mode],
            evidence,
            f"workerd-{mode}",
            record,
        )
        # Cargo's runner-form example line differs from libtest's parenthesized
        # executable line. Record this established example path explicitly.
        record.commands[-1].binaries = [
            file_evidence(BUILD_TARGET / "debug/examples" / scenario.example)
        ]
        save_record(evidence, record)
        observations.append(check_workerd_output(mode, output))
        if mode == "owner-retirement":
            readback = read_owner_retirement_state(evidence / f"state-{mode}")
            _ = (evidence / "workerd-owner-retirement-readback.json").write_text(
                readback.model_dump_json(indent=2) + "\n"
            )
        _ = (evidence / "workerd-observations.json").write_text(
            "[\n"
            + ",\n".join(observation.model_dump_json() for observation in observations)
            + "\n]\n"
        )


if __name__ == "__main__":
    raise SystemExit(main())
