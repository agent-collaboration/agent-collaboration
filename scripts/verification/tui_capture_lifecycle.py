"""Finite lifecycle for the declared compact capture and exact libtest discovery.

Rust retains ownership of PTY and browser cleanup. This module requests its fixed
private stop and records failure; it never signals those product resources.
"""

import base64
import datetime as dt
import json
import os
import stat
import subprocess
import tempfile
import typing as t
from pathlib import Path

from pydantic import BaseModel, PositiveFloat

from scripts.verification.tui_verification import (
    QUOTA_CHECKPOINT_STATES,
    SESSION_DETAILS_POSES,
    VIEWPORTS,
    session_checkpoint_states,
)


def static_capture_budget_seconds(
    surfaces: tuple[t.Literal["sessions", "quota"], ...],
) -> int:
    # Existing owners allow 90s/VHS and 30s/image. Sessions has three and Quota four
    # negative image checks per grid. Cleanup has 35s of existing PID/browser waits;
    # allow 40s per grid and retain a separate 90s whole-driver reserve.
    # This is the current six-grid source budget. Prior run packets retain their
    # original caps; a new target still needs its own finite execution packet.
    return 90 + sum(
        90
        + 30
        * (
            len(session_checkpoint_states(columns, rows))
            + 3
            + len(SESSION_DETAILS_POSES[(columns, rows)])
            + (1 if (columns, rows) == (48, 12) else 0)
            if surface == "sessions"
            else len(QUOTA_CHECKPOINT_STATES) + 4
        )
        + 40
        for surface in surfaces
        for columns, rows in VIEWPORTS
    )


class StaticCaptureBudget(BaseModel):
    work_seconds: PositiveFloat
    cleanup_seconds: PositiveFloat


def request_static_stop(output: Path) -> None:
    marker = output / "capture-stop"
    try:
        descriptor = os.open(marker, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        metadata = marker.lstat()
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or marker.read_bytes() != b"stop\n"
        ):
            raise ValueError("invalid private capture stop marker") from None
    else:
        with os.fdopen(descriptor, "wb") as stream:
            _ = stream.write(b"stop\n")


def wait_static_capture(
    child: subprocess.Popen[bytes], output: Path, budget: StaticCaptureBudget
) -> int:
    """Rust owns PTY/browser cleanup; Python only requests its private stop."""
    started = dt.datetime.now(dt.UTC)
    timed_out = False
    stop_error: str | None = None
    try:
        result = child.wait(timeout=budget.work_seconds)
    except subprocess.TimeoutExpired:
        timed_out = True
        try:
            request_static_stop(output)
        except (OSError, ValueError) as error:
            stop_error = str(error)
        try:
            result = child.wait(timeout=budget.cleanup_seconds)
        except subprocess.TimeoutExpired:
            result = None
    _ = (output / "driver-exit.json").write_text(
        json.dumps(
            {
                "exit": result,
                "pid": child.pid,
                "started_at": started.isoformat(),
                "observed_at": dt.datetime.now(dt.UTC).isoformat(),
                "work_seconds": budget.work_seconds,
                "cleanup_seconds": budget.cleanup_seconds,
                "work_deadline": (
                    started + dt.timedelta(seconds=budget.work_seconds)
                ).isoformat(),
                "whole_deadline": (
                    started
                    + dt.timedelta(seconds=budget.work_seconds + budget.cleanup_seconds)
                ).isoformat(),
                "deadline_reached": timed_out,
                "driver_settled": result is not None,
                "stop_error": stop_error,
            },
            indent=2,
        )
    )
    if timed_out:
        settlement = "driver exited" if result is not None else "cleanup unsettled"
        raise TimeoutError(f"static capture deadline; {settlement}; {stop_error=}")
    assert result is not None
    return result


def discover_capture_test(
    command: list[str], repository: Path, timeout: float = 10
) -> str:
    """Bound only the exact libtest --list child, retaining failure streams."""
    with tempfile.TemporaryDirectory(prefix="tui-discovery-") as directory:
        stdout_path, stderr_path = (
            Path(directory) / "stdout",
            Path(directory) / "stderr",
        )
        with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
            child = subprocess.Popen(
                command,
                cwd=repository,
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
            )
            timed_out = False
            try:
                result = child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                child.kill()
                try:
                    result = child.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    result = None
        output = stdout_path.read_text(errors="replace")
        if timed_out or result != 0:
            raise ValueError(
                "capture discovery failed: "
                + json.dumps(
                    {
                        "command": command,
                        "pid": child.pid,
                        "exit": result,
                        "timeout": timed_out,
                        "stdout": output,
                        "stderr": stderr_path.read_text(errors="replace"),
                        "stdout_base64": base64.b64encode(
                            stdout_path.read_bytes()
                        ).decode("ascii"),
                        "stderr_base64": base64.b64encode(
                            stderr_path.read_bytes()
                        ).decode("ascii"),
                    }
                )
            )
        return output
