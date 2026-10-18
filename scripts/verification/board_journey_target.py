"""Validate the declared product source and pin the exact runnable artifacts."""

import hashlib
import os
import typing as t
from pathlib import Path

from .board_journey_values import (
    JsonObject,
    JsonValue,
    JourneyFailure,
    file_digest,
    run_command,
)


@t.final
class JourneyTarget:
    def __init__(
        self, source_root: Path, expected_head: str, peer_qualification: Path | None
    ) -> None:
        self.source_root = source_root.resolve()
        self.expected_head = expected_head
        self.peer_qualification = peer_qualification
        self.launcher = self.source_root / "target/debug/examples/board-debug-runtime"
        self.cli = self.source_root / "target/debug/agent-collaboration"
        self.digests: JsonObject = {}

    def inspect(self, reference_repository: Path) -> JsonObject:
        resolved = run_command(
            ["git", "rev-parse", f"{self.expected_head}^{{commit}}"],
            reference_repository,
        )
        if resolved.returncode:
            raise JourneyFailure(f"cannot resolve expected source: {resolved.stderr}")
        expected = resolved.stdout.strip()
        current = run_command(["git", "rev-parse", "--show-toplevel"], self.source_root)
        if (
            current.returncode == 0
            and Path(current.stdout.strip()).resolve() == self.source_root
        ):
            actual = run_command(
                ["git", "rev-parse", "HEAD"], self.source_root
            ).stdout.strip()
            if actual != expected:
                raise JourneyFailure(
                    f"source HEAD changed: expected {expected}, observed {actual}"
                )
            owned_checkout = self.source_root == reference_repository.resolve()
            status_command = ["git", "status", "--porcelain", "--untracked-files=no"]
            if owned_checkout:
                status_command.extend(["--", ".", ":(exclude)scripts/verification/**"])
            status = run_command(status_command, self.source_root).stdout
            if status:
                raise JourneyFailure(
                    f"product source has uncommitted changes: {status}"
                )
            verifier_status = (
                run_command(
                    [
                        "git",
                        "status",
                        "--porcelain",
                        "--untracked-files=no",
                        "--",
                        "scripts/verification",
                    ],
                    self.source_root,
                ).stdout
                if owned_checkout
                else ""
            )
            untracked = run_command(
                ["git", "ls-files", "--others", "--exclude-standard", "-z"],
                self.source_root,
            ).stdout.split("\0")
            verifier_files = [name for name in untracked if name]
            verifier_root = reference_repository / "scripts/verification"
            if any(
                self.source_root != reference_repository.resolve()
                or not (self.source_root / name).resolve().is_relative_to(verifier_root)
                for name in verifier_files
            ):
                raise JourneyFailure(
                    "source has untracked files outside the owned verifier"
                )
            source_proof: JsonObject = {
                "kind": "cleanCheckout"
                if not verifier_files and not verifier_status
                else "productCleanWithOwnedVerifier",
                "head": actual,
                "trackedVerifierStatus": verifier_status,
                "untrackedVerifierFiles": list[JsonValue](verifier_files),
            }
        else:
            source_proof = self.verify_export(reference_repository, expected)
        owned_target = (self.source_root / "target").resolve()
        if not owned_target.is_relative_to(self.source_root):
            raise JourneyFailure(
                "target must belong to the declared source, not a shared symlink"
            )
        if not all(
            path.resolve().is_relative_to(owned_target)
            for path in [self.launcher, self.cli]
        ):
            raise JourneyFailure(
                "declared runtime executables escaped their owned target"
            )
        self.digests = {
            "launcherSha256": file_digest(self.launcher),
            "cliSha256": file_digest(self.cli),
        }
        identity: JsonObject = {
            "sourceRoot": str(self.source_root),
            "source": source_proof,
            "launcher": str(self.launcher),
            "cli": str(self.cli),
            "buildProfile": "debug",
            **self.digests,
        }
        if self.peer_qualification is not None:
            identity["peerQualification"] = {
                "path": str(self.peer_qualification.resolve()),
                "sha256": file_digest(self.peer_qualification),
            }
        return identity

    def verify_export(self, repository: Path, expected: str) -> JsonObject:
        result = run_command(["git", "ls-tree", "-r", "-z", expected], repository)
        if result.returncode:
            raise JourneyFailure(f"cannot inspect export reference: {result.stderr}")
        checked = 0
        for entry in result.stdout.split("\0"):
            if not entry:
                continue
            metadata, name = entry.split("\t", 1)
            mode, kind, digest = metadata.split()
            if kind != "blob":
                raise JourneyFailure(f"export includes unsupported tree entry: {entry}")
            path = self.source_root / name
            contents = (
                os.readlink(path).encode() if mode == "120000" else path.read_bytes()
            )
            actual = hashlib.sha1(
                f"blob {len(contents)}\0".encode() + contents
            ).hexdigest()
            if actual != digest:
                raise JourneyFailure(f"export differs from {expected}: {name}")
            checked += 1
        return {"kind": "verifiedGitExport", "head": expected, "checkedBlobs": checked}

    def require_unchanged_binaries(self) -> None:
        current: JsonObject = {
            "launcherSha256": file_digest(self.launcher),
            "cliSha256": file_digest(self.cli),
        }
        if current != self.digests:
            raise JourneyFailure("target binaries changed during the journey")
