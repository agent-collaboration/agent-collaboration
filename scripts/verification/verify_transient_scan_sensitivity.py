"""Run the one commissioned Database-quarantine mutation in an isolated export."""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import typing as t
from pathlib import Path

from .board_journey_target import JourneyTarget
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    run_command,
)

TEST_NAME = (
    "service_identity::subscription_delivery::tests::transient_scan_proofs::"
    "maintenance_timeout_retries_without_quarantining_the_project"
)
SOURCE_PATH = Path(
    "crates/collaboration-service/src/subscription_delivery/subscription_window_error.rs"
)


def run_case(export: Path, log: Path) -> tuple[int, str]:
    command = [
        "cargo",
        "test",
        "--locked",
        "-p",
        "collaboration-service",
        "--lib",
        TEST_NAME,
        "--",
        "--exact",
        "--nocapture",
    ]
    environment = {
        **os.environ,
        "CC": "/usr/bin/clang",
        "SQLX_OFFLINE": "true",
        "CARGO_INCREMENTAL": "0",
        "CARGO_TARGET_DIR": str(export / "target"),
    }
    with log.open("x", encoding="utf-8") as stream:
        _ = stream.write(
            json.dumps(
                {
                    "command": command,
                    "cwd": str(export),
                    "target": environment["CARGO_TARGET_DIR"],
                }
            )
            + "\n"
        )
        stream.flush()
        result = subprocess.run(
            command,
            cwd=export,
            env=environment,
            stdout=stream,
            stderr=subprocess.STDOUT,
            check=False,
        )
    return result.returncode, log.read_text(encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--source-export", type=Path, required=True)
    _ = parser.add_argument("--expected-head", required=True)
    _ = parser.add_argument("--output-directory", type=Path, required=True)
    options = parser.parse_args()
    export = t.cast(Path, options.source_export).resolve()
    output = t.cast(Path, options.output_directory).resolve()
    expected = t.cast(str, options.expected_head)
    repository = Path(__file__).resolve().parents[2]
    if not export.is_relative_to(repository / "tmp/verify-m2") or export == repository:
        raise JourneyFailure("mutation may modify only this task's isolated export")
    if (export / ".git").exists() or not (export / "target").resolve().is_relative_to(
        export
    ):
        raise JourneyFailure(
            "use an archive export with its own target, never a worktree or shared target"
        )
    revision = run_command(["git", "rev-parse", f"{expected}^{{commit}}"], repository)
    if revision.returncode:
        raise JourneyFailure("cannot resolve the commissioned final source revision")
    expected = revision.stdout.strip()
    output.mkdir(parents=True, exist_ok=False)
    target = JourneyTarget(export, expected, None)
    source_proof = target.verify_export(repository, expected)
    source = export / SOURCE_PATH
    original = source.read_bytes()
    needle = b"| ProjectStorageError::InvalidSchema\n        | ProjectStorageError::SchemaNotReady(_))"
    if original.count(needle) != 1:
        raise JourneyFailure(
            "the commissioned mutation owner changed; inspect instead of guessing"
        )
    mutated = original.replace(
        needle,
        b"| ProjectStorageError::InvalidSchema\n        | ProjectStorageError::Database(_)\n        | ProjectStorageError::SchemaNotReady(_))",
        1,
    )
    try:
        _ = source.write_bytes(mutated)
        red_exit, red = run_case(export, output / "quarantine-red.log")
    finally:
        _ = source.write_bytes(original)
    restored = source.read_bytes() == original
    _ = target.verify_export(repository, expected)
    green_exit, green = run_case(export, output / "retry-green.log")
    red_is_behavior = (
        red_exit == 101
        and "a transient maintenance timeout permanently quarantined this project"
        in red
        and "test result: FAILED. 0 passed; 1 failed" in red
    )
    green_is_exact = green_exit == 0 and "test result: ok. 1 passed; 0 failed" in green
    receipt: JsonObject = {
        "source": source_proof,
        "test": TEST_NAME,
        "mutationFile": str(SOURCE_PATH),
        "originalSha256": hashlib.sha256(original).hexdigest(),
        "mutationSha256": hashlib.sha256(mutated).hexdigest(),
        "sourceRestored": restored,
        "redExit": red_exit,
        "redCaughtQuarantine": red_is_behavior,
        "greenExit": green_exit,
        "greenExecutedExactTest": green_is_exact,
        "realAppObservation": "not observable through the app; real-store integration and mutation only",
    }
    _ = (output / "sensitivity.json").write_text(
        json.dumps(receipt, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(receipt), flush=True)
    return 0 if restored and red_is_behavior and green_is_exact else 1


if __name__ == "__main__":
    sys.exit(main())
