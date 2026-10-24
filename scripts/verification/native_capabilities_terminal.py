"""Drive one installed Codex TUI interactively in a private pseudo-terminal.

The journey needs real keystrokes: the `$` picker, Tab selection and a typed task,
which a `script` capture of a prompt argument cannot produce (Codex 0.163 builds a
plugin Mention only from a picker binding). Raw output bytes are appended to the
capture file as they arrive. Waits are bounded and look for screen text; a timeout
raises instead of passing. Only this driver's own child is ever signalled.
"""

import fcntl
import os
import pty
import re
import select
import signal
import struct
import termios
import time
import typing as t
from pathlib import Path

from scripts.verification.acp_metadata_observation import terminal_text

TAB = b"\t"
ENTER = b"\r"
CTRL_C = b"\x03"
CURSOR_POSITION_QUERY = b"\x1b[6n"


def bracketed_paste(text: str) -> bytes:
    # Multi-line task text arrives as one paste, as a user pasting it would send it.
    return b"\x1b[200~" + text.encode() + b"\x1b[201~"


class TerminalTimeout(TimeoutError):
    pass


class InstalledCodexTerminal:
    def __init__(
        self,
        argv: list[str],
        capture: Path,
        *,
        rows: int = 50,
        columns: int = 160,
        environment: dict[str, str] | None = None,
    ) -> None:
        self.argv: list[str] = argv
        self.capture: Path = capture
        self.rows: int = rows
        self.columns: int = columns
        self.environment: dict[str, str] = environment or dict(os.environ)
        self.raw: bytearray = bytearray()
        self.child_pid: int = 0
        self.master_fd: int = -1
        self.exit_status: int | None = None
        self.forced_stop: bool = False

    def __enter__(self) -> t.Self:
        self.capture.touch(mode=0o600, exist_ok=False)
        pid, master_fd = pty.fork()
        if pid == 0:
            os.execve(self.argv[0], self.argv, self.environment)
        self.child_pid, self.master_fd = pid, master_fd
        _ = fcntl.ioctl(
            master_fd,
            termios.TIOCSWINSZ,
            struct.pack("HHHH", self.rows, self.columns, 0, 0),
        )
        return self

    def __exit__(self, *_: object) -> None:
        self.shutdown()

    def mark(self) -> int:
        return len(self.raw)

    def screen_since(self, mark: int) -> str:
        return terminal_text(bytes(self.raw[mark:]))

    def send(self, data: bytes) -> None:
        _ = os.write(self.master_fd, data)

    def type_text(self, text: str) -> None:
        if "\n" in text:
            self.send(bracketed_paste(text))
        else:
            self.send(text.encode())

    def pump(self, timeout: float) -> bool:
        """Read available output; returns False once the child closed the terminal."""
        ready, _, _ = select.select([self.master_fd], [], [], timeout)
        if not ready:
            return True
        try:
            chunk = os.read(self.master_fd, 65536)
        except OSError:
            return False
        if not chunk:
            return False
        self.raw.extend(chunk)
        with self.capture.open("ab") as capture:
            _ = capture.write(chunk)
        if CURSOR_POSITION_QUERY in chunk:
            # No terminal emulator answers here; reply as a top-left cursor would.
            self.send(b"\x1b[1;1R")
        return True

    def wait_for(self, pattern: str, mark: int, timeout: float) -> str:
        """Wait for a regular expression in the normalized screen text since `mark`."""
        deadline = time.monotonic() + timeout
        expression = re.compile(pattern)
        while time.monotonic() < deadline:
            screen = self.screen_since(mark)
            if expression.search(screen):
                return screen
            if not self.pump(min(0.2, max(0.0, deadline - time.monotonic()))):
                break
        raise TerminalTimeout(f"screen never showed {pattern!r}")

    def wait_for_any(
        self, patterns: dict[str, str], mark: int, timeout: float
    ) -> str | None:
        """Return the first matching label, or None when the bound expires."""
        deadline = time.monotonic() + timeout
        compiled = {label: re.compile(pattern) for label, pattern in patterns.items()}
        while time.monotonic() < deadline:
            screen = self.screen_since(mark)
            for label, expression in compiled.items():
                if expression.search(screen):
                    return label
            if not self.pump(min(0.2, max(0.0, deadline - time.monotonic()))):
                return None
        return None

    def shutdown(self, timeout: float = 15.0) -> None:
        """Quit through the TUI's own Ctrl-C shutdown; force-stop only this child."""
        if self.child_pid == 0 or self.exit_status is not None:
            return
        for _ in range(2):
            if self._reap():
                break
            self.send(CTRL_C)
            deadline = time.monotonic() + timeout / 2
            while time.monotonic() < deadline and not self._reap():
                _ = self.pump(0.2)
        if not self._reap():
            self.forced_stop = True
            os.kill(self.child_pid, signal.SIGTERM)
            _, status = os.waitpid(self.child_pid, 0)
            self.exit_status = status
        os.close(self.master_fd)

    def _reap(self) -> bool:
        if self.exit_status is not None:
            return True
        pid, status = os.waitpid(self.child_pid, os.WNOHANG)
        if pid == 0:
            return False
        self.exit_status = status
        return True
