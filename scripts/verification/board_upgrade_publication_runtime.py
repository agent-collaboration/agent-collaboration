"""Own the isolated Tokio child and a Complete-wire interruption gate (no product shim)."""

import contextlib
import http.client
import http.server
import json
import selectors
import socket
import subprocess
import threading
import typing as t
import urllib.parse
from dataclasses import dataclass, field
from pathlib import Path

from .board_upgrade_publication_views import (
    CentralReadback,
    CentralSettlement,
    Inspected,
    PublicationPlan,
    Ready,
    Stopped,
    central_reply,
)
from .board_upgrade_views import Object, json_object, text_field


def post_json(url: str, assertion_file: Path, body: Object) -> Object:
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "http" or parsed.hostname != "127.0.0.1" or not parsed.port:
        raise ValueError("owner readback must address the isolated loopback central")
    with contextlib.closing(
        http.client.HTTPConnection("127.0.0.1", parsed.port, timeout=15)
    ) as connection:
        connection.request(
            "POST",
            parsed.path,
            json.dumps(body).encode(),
            {
                "Content-Type": "application/json",
                "Cf-Access-Jwt-Assertion": assertion_file.read_text(),
            },
        )
        response = connection.getresponse()
        contents = response.read()
        if response.status != 200:
            raise AssertionError(
                f"read-only owner status returned HTTP {response.status}"
            )
        return json_object.validate_json(contents)


@t.final
class OwnedCentral:
    """The child serves real central and writes readbacks from its own process."""

    def __init__(
        self, plan: PublicationPlan, case: Path, label: str, projects: list[str]
    ) -> None:
        self.case = case
        self.label = label
        root = case / "central"
        root.mkdir(mode=0o700, exist_ok=True)
        self.assertion_file = case / f"{label}-assertion"
        request = case / f"{label}-central-request.json"
        _ = request.write_text(
            json.dumps(
                {
                    "root": str(root),
                    "assertionFile": str(self.assertion_file),
                    "projects": projects,
                }
            )
        )
        self.stderr = (case / f"{label}-central.stderr").open("w")
        self.arguments = [str(plan.central_binary), str(request)]
        self.process: subprocess.Popen[str] = subprocess.Popen(
            self.arguments,
            cwd=plan.repository,
            text=True,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr,
        )
        self.stopped = False
        try:
            ready = self.read_reply()
            if not isinstance(ready, Ready) or ready.pid != self.process.pid:
                raise AssertionError(
                    "central did not identify its actual ready process"
                )
            parsed = urllib.parse.urlsplit(ready.url)
            if (
                parsed.scheme != "http"
                or parsed.hostname != "127.0.0.1"
                or not parsed.port
            ):
                raise ValueError(
                    "central must bind an ephemeral loopback HTTP endpoint"
                )
            self.url = ready.url
            _ = (case / f"{label}-ready.json").write_text(
                ready.model_dump_json(indent=2)
            )
        except BaseException:
            self.close()
            raise

    def read_reply(self) -> Ready | Inspected | Stopped:
        if self.process.stdout is None:
            raise ValueError("central stdout missing")
        stdout: t.IO[str] = self.process.stdout
        with selectors.DefaultSelector() as selector:
            _ = selector.register(stdout, selectors.EVENT_READ)
            if not selector.select(30):
                raise TimeoutError(
                    "central operation did not produce its readiness receipt"
                )
            line = stdout.readline()
        if not line:
            raise AssertionError(
                f"central ended before reply (exit {self.process.poll()})"
            )
        return central_reply.validate_json(line)

    def send(self, body: Object) -> Ready | Inspected | Stopped:
        if self.process.stdin is None:
            raise ValueError("central stdin missing")
        _ = self.process.stdin.write(json.dumps(body) + "\n")
        self.process.stdin.flush()
        return self.read_reply()

    def inspect(self, label: str) -> CentralReadback:
        output = self.case / f"{self.label}-{label}.json"
        response = self.send({"kind": "inspect", "output": str(output)})
        if not isinstance(response, Inspected) or response.output != output:
            raise AssertionError("owning central readback receipt differs")
        return CentralReadback.model_validate_json(output.read_bytes())

    def stop(self, label: str) -> CentralReadback:
        output = self.case / f"{self.label}-{label}.json"
        response = self.send({"kind": "stop", "output": str(output)})
        if not isinstance(response, Stopped) or response.output != output:
            raise AssertionError("central stop/readback receipt differs")
        if self.process.stdin is not None:
            self.process.stdin.close()
        code = self.process.wait(timeout=20)
        if code != 0:
            raise AssertionError(f"central shutdown exit {code}")
        self.stopped = True
        self.close()
        return CentralReadback.model_validate_json(output.read_bytes())

    def close(self) -> None:
        if self.process.poll() is None:
            if self.process.stdin is not None and not self.process.stdin.closed:
                self.process.stdin.close()
            try:
                _ = self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                try:
                    _ = self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    _ = self.process.wait(timeout=5)
        for pipe in [self.process.stdout, self.process.stdin]:
            if pipe is not None and not pipe.closed:
                pipe.close()
        self.stderr.close()
        if self.process.returncode is None:
            raise AssertionError("central process remains unsettled")
        _ = (self.case / f"{self.label}-settlement.json").write_text(
            CentralSettlement(
                arguments=self.arguments,
                pid=self.process.pid,
                exit=self.process.returncode,
                stop_receipt_observed=self.stopped,
            ).model_dump_json(indent=2)
        )


@dataclass
class GateState:
    phase: t.Literal["before", "after"]
    reached: threading.Event = field(default_factory=threading.Event)
    release: threading.Event = field(default_factory=threading.Event)
    begins: dict[str, Object] = field(default_factory=dict)
    checkpoint: Object | None = None
    failure: str | None = None


@t.final
class CompleteGate:
    """Hold the actual first Complete before forwarding, or hold its actual reply."""

    def __init__(self, upstream: str, phase: t.Literal["before", "after"]) -> None:
        parsed = urllib.parse.urlsplit(upstream)
        if parsed.hostname != "127.0.0.1" or parsed.scheme != "http" or not parsed.port:
            raise ValueError("gate upstream must be the owned loopback central")
        port = parsed.port
        self.state = state = GateState(phase)

        class Forwarder(http.server.BaseHTTPRequestHandler):
            connection: socket.socket

            @t.override
            def log_message(self, format: str, *args: object) -> None:
                return

            def do_POST(self) -> None:
                self.connection.settimeout(15)
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    if not 0 < length <= 4 * 1024 * 1024:
                        raise ValueError("request body outside the snapshot-unit bound")
                    body = self.rfile.read(length)
                    request = json_object.validate_json(body)
                    project_route = "/admin/v1/import/projects/" in self.path
                    kind = request["kind"] if project_route else None
                    if kind == "begin":
                        value = json_object.validate_python(request["value"])
                        header = json_object.validate_python(value["header"])
                        manifest = json_object.validate_python(header["manifest"])
                        state.begins[text_field(manifest, "database")] = header
                    complete = kind == "complete" and project_route
                    held = complete and state.checkpoint is None
                    if held:
                        state.checkpoint = {
                            "phase": phase,
                            "path": self.path,
                            "request": request,
                            "begins": json_object.validate_python(state.begins),
                        }
                    if held and phase == "before":
                        state.reached.set()
                        if not state.release.wait(60):
                            raise TimeoutError(
                                "before-publication gate was not released"
                            )
                        return
                    with contextlib.closing(
                        http.client.HTTPConnection("127.0.0.1", port, timeout=20)
                    ) as connection:
                        headers = {
                            name: value
                            for name, value in self.headers.items()
                            if name.lower()
                            not in {"host", "connection", "transfer-encoding"}
                        }
                        connection.request("POST", self.path, body, headers)
                        response = connection.getresponse()
                        contents = response.read()
                        response_headers = response.getheaders()
                        status = response.status
                    if held and phase == "after":
                        if state.checkpoint is None:
                            raise AssertionError("gate checkpoint missing")
                        state.checkpoint["status"] = status
                        state.checkpoint["response"] = json_object.validate_json(
                            contents
                        )
                        state.reached.set()
                        if not state.release.wait(60):
                            raise TimeoutError(
                                "after-publication gate was not released"
                            )
                        return
                    self.send_response(status)
                    for name, value in response_headers:
                        if name.lower() not in {
                            "content-length",
                            "transfer-encoding",
                            "connection",
                        }:
                            self.send_header(name, value)
                    self.send_header("Content-Length", str(len(contents)))
                    self.end_headers()
                    _ = self.wfile.write(contents)
                except (BrokenPipeError, ConnectionResetError):
                    pass
                except (
                    OSError,
                    ValueError,
                    KeyError,
                    TypeError,
                    AssertionError,
                    http.client.HTTPException,
                ) as error:
                    state.failure = f"{type(error).__name__}: {error}"
                    state.reached.set()
                    with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                        self.send_error(502)

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Forwarder)
        self.thread = threading.Thread(
            target=self.server.serve_forever, kwargs={"poll_interval": 0.05}
        )
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}"

    def close(self) -> None:
        self.state.release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=10)
        if self.thread.is_alive():
            raise AssertionError("owned HTTP gate failed to settle")
