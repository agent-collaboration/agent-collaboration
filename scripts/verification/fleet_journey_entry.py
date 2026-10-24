"""Record a fleet journey's source entry gate without launching or building it."""

import argparse
import hashlib
import os
import subprocess
import tempfile
import typing as t
from dataclasses import dataclass
from pathlib import Path

from pydantic import BaseModel, ConfigDict


class CargoTarget(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    name: str
    kind: list[str]


class CargoPackage(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    name: str
    targets: list[CargoTarget]


class CargoWorkspace(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    packages: list[CargoPackage]
    workspace_root: str


class EntryArguments(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    expected_head: str | None
    output_directory: Path | None
    list_cases: bool


class EntryCommand(BaseModel):
    arguments: list[str]
    exit: int


class EntryCoverage(BaseModel):
    case: str
    expected: str
    conclusion: t.Literal["unknown"] = "unknown"


class UnbuiltTarget(BaseModel):
    status: t.Literal["notBuilt"] = "notBuilt"
    binary: None = None


class EntryReceipt(BaseModel):
    journey: str
    conclusion: t.Literal["unknown"] = "unknown"
    stage: t.Literal["entryRefused"] = "entryRefused"
    reason: str
    source_head: str
    source_root: str
    build: UnbuiltTarget
    client: str
    workspace_packages: int
    matching_packages: int
    matching_executable_targets: int
    product_assertions: t.Literal[0] = 0
    cases_run: t.Literal[0] = 0
    coverage: list[EntryCoverage]
    commands: list[EntryCommand]
    driver_hashes: dict[str, str]
    exit: t.Literal[2] = 2


@dataclass(frozen=True, kw_only=True)
class EntryRequirement:
    journey: str
    package: str
    binary: str | None
    cases: dict[str, str]


def entry_arguments(description: str) -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=description)
    _ = parser.add_argument("--expected-head", help="Declared product commit")
    _ = parser.add_argument(
        "--output-directory",
        type=Path,
        help="New private evidence directory under system temp",
    )
    _ = parser.add_argument(
        "--list-cases",
        action="store_true",
        help="List obligations; does not claim they ran",
    )
    return parser


def inspect_entry(
    expected_head: str,
    output: Path,
    requirement: EntryRequirement,
) -> int:
    """Exit 2 for an unavailable route; no package presence can turn into a pass."""
    repository = Path(__file__).resolve().parents[2]
    parent = output.parent.resolve(strict=True)
    temp_roots = {Path(tempfile.gettempdir()).resolve(), Path("/tmp").resolve()}
    if (
        not output.is_absolute()
        or parent.is_relative_to(repository)
        or not any(
            parent != root and parent.is_relative_to(root) for root in temp_roots
        )
        or parent.stat().st_uid != os.getuid()
        or parent.stat().st_mode & 0o077
        or output.is_symlink()
    ):
        raise ValueError(
            "evidence parent must be private, owned, under system temp and outside the checkout"
        )
    output.mkdir(mode=0o700, exist_ok=False)
    commands: list[EntryCommand] = []

    def command(arguments: list[str]) -> str:
        result = subprocess.run(
            arguments,
            cwd=repository,
            text=True,
            capture_output=True,
            check=False,
            timeout=60,
        )
        commands.append(EntryCommand(arguments=arguments, exit=result.returncode))
        if result.returncode:
            raise ValueError(f"entry command failed: {arguments!r}: {result.stderr}")
        return result.stdout

    expected = command(["git", "rev-parse", f"{expected_head}^{{commit}}"]).strip()
    actual = command(["git", "rev-parse", "HEAD"]).strip()
    if expected != actual:
        raise ValueError(f"target changed: expected {expected}, observed {actual}")
    # Both authorized apparatus homes may change while the product is pinned.
    dirty = command(
        [
            "git",
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ".",
            ":(exclude)scripts/verification/**",
            ":(exclude).agents/skills/verify-codex-router/**",
        ]
    )
    if dirty:
        raise ValueError(f"product source is dirty: {dirty}")
    metadata_text = command(
        [
            "cargo",
            "metadata",
            "--offline",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
        ]
    )
    metadata = CargoWorkspace.model_validate_json(metadata_text)
    if Path(metadata.workspace_root).resolve() != repository:
        raise ValueError("Cargo metadata belongs to another workspace")
    packages = [
        package for package in metadata.packages if package.name == requirement.package
    ]
    targets = [
        target
        for package in packages
        for target in package.targets
        if target.name == requirement.binary and "bin" in target.kind
    ]
    if requirement.journey == "board-upgrade" and not targets:
        reason = f"no runnable {requirement.binary} target in {requirement.package}; no importer yet"
    elif not packages:
        reason = f"no {requirement.package} package; B's real core and fleet cutover are prerequisites"
    else:
        reason = "source entry exists; declared build/configuration and real driving route still need qualification"
    driver_paths = [
        Path(__file__),
        Path(__file__).with_name(requirement.journey.replace("-", "_") + "_journey.py"),
    ]
    receipt = EntryReceipt(
        journey=requirement.journey,
        reason=reason,
        source_head=actual,
        source_root=str(repository),
        build=UnbuiltTarget(),
        client="cargo metadata --offline --locked --no-deps",
        workspace_packages=len(metadata.packages),
        matching_packages=len(packages),
        matching_executable_targets=len(targets),
        coverage=[
            EntryCoverage(case=case, expected=expectation)
            for case, expectation in requirement.cases.items()
        ],
        commands=commands,
        driver_hashes={
            path.name: hashlib.sha256(path.read_bytes()).hexdigest()
            for path in driver_paths
        },
    )
    _ = (output / "entry.json").write_text(
        receipt.model_dump_json(indent=2) + "\n", encoding="utf-8"
    )
    print(receipt.model_dump_json())
    return 2
