"""M2c mail/wake journey against frozen debug Host/Router artifacts."""

import argparse
import http.client
import sqlite3
import subprocess
import sys
import traceback
import typing as t
from pathlib import Path

from .board_journey_runtime import BoardRuntime, JourneyReceipt
from .board_journey_target import JourneyTarget
from .board_journey_values import JourneyFailure, JsonObject, JsonValue, file_digest
from .mail_journey_messages import MailMessages
from .mail_journey_migration import observe_migration
from .mail_journey_peer import MailPeer
from .mail_journey_wakes import MailWakes


class JourneyOptions(argparse.Namespace):
    def __init__(self, repository: Path) -> None:
        super().__init__()
        self.source_root: Path = repository
        self.expected_head: str = ""
        self.output_directory: Path = repository / "tmp/m2/m2c-journey"
        self.peer_qualification: Path | None = None


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--source-root", type=Path, default=repository)
    _ = parser.add_argument("--expected-head", required=True)
    _ = parser.add_argument("--output-directory", type=Path, required=True)
    _ = parser.add_argument("--peer-qualification", type=Path, required=True)
    options = parser.parse_args(namespace=JourneyOptions(repository))
    receipt = JourneyReceipt(options.output_directory.resolve())
    runtime: BoardRuntime | None = None
    peer: MailPeer | None = None
    frozen: dict[Path, str] = {}
    failed = False
    try:
        target = JourneyTarget(
            options.source_root, options.expected_head, options.peer_qualification
        )
        receipt.target = target.inspect(repository)
        artifacts = [
            target.launcher,
            target.cli,
            target.source_root / "target/debug/codex-router",
            target.source_root / "target/debug/agent-sessions",
        ]
        for artifact in artifacts:
            if not artifact.resolve().is_relative_to(
                (target.source_root / "target").resolve()
            ):
                raise JourneyFailure("frozen artifact escaped its owned target")
            frozen[artifact] = file_digest(artifact)
        driver_files: JsonObject = {
            str(path.relative_to(repository)): file_digest(path)
            for path in sorted(Path(__file__).parent.glob("*.py"))
        }
        receipt.target.update(
            {
                "frozenArtifacts": {
                    str(path): digest for path, digest in frozen.items()
                },
                "driverFiles": driver_files,
                "invocation": list[JsonValue](sys.argv),
                "declaredLimits": [
                    "producer-qualified peer registry entry and owned peer receiver; actual native producer and agent turns remain M4",
                    "legacy migration is a stopped copy of a fresh debug store with only the unreleased schema changes reversed",
                    "no production process, profile, installation or central service action",
                ],
            }
        )
        receipt.record({"kind": "target", "identity": receipt.target})
        runtime = BoardRuntime(target, receipt)
        runtime.start()
        runtime.stop()
        receipt.record({"kind": "case", "case": "legacy-migration", "stage": "start"})
        observe_migration(runtime)
        receipt.record(
            {
                "kind": "case",
                "case": "legacy-migration",
                "stage": "complete",
                "passed": True,
            }
        )
        runtime.start()
        peer = MailPeer(runtime)
        peer.start()
        cases: list[tuple[str, t.Callable[[], None]]] = [
            ("local-replay", MailMessages(peer).replay),
            ("lost-local-answers", MailMessages(peer).lost_answers),
            ("wake-slots-and-limits", MailWakes(peer).slots_and_limits),
            ("wake-due-relative-end", MailWakes(peer).due_relative_end),
        ]
        for name, action in cases:
            receipt.record({"kind": "case", "case": name, "stage": "start"})
            action()
            receipt.record(
                {"kind": "case", "case": name, "stage": "complete", "passed": True}
            )
    except (
        JourneyFailure,
        OSError,
        ValueError,
        sqlite3.Error,
        subprocess.SubprocessError,
        http.client.HTTPException,
    ) as error:
        failed = True
        receipt.record(
            {
                "kind": "failure",
                "errorType": type(error).__name__,
                "error": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
    finally:
        try:
            if runtime is not None:
                runtime.stop()
            if peer is not None:
                peer.stop()
            for artifact, digest in frozen.items():
                receipt.equal(
                    f"frozen {artifact.name} did not change",
                    "artifact integrity",
                    (digest, file_digest(artifact)),
                )
        except (
            JourneyFailure,
            OSError,
            ValueError,
            subprocess.SubprocessError,
        ) as error:
            failed = True
            receipt.record(
                {
                    "kind": "cleanupFailure",
                    "error": str(error),
                    "raw": {"traceback": traceback.format_exc()},
                }
            )
        receipt.record(
            {
                "kind": "result",
                "failed": failed,
                "receipt": str(receipt.directory / "journey.json"),
            }
        )
        receipt.close()
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
