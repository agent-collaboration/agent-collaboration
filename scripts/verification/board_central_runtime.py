"""Own a real local central and fixture Access edges for model-free board Hosts.

The edge changes only the HTTP handshake: it supplies the repository's signed
Access test assertion, then forwards every WebSocket byte unchanged. Registry,
Copy, admission, origin verification and board policy remain the real product.
No owner credentials, provider process or normal Codex state are read.
"""

import base64
import hashlib
import http.client
import json
import os
import selectors
import socket
import socketserver
import subprocess
import threading
import time
import typing as t
import urllib.request
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field

from .board_journey_runtime import BoardClient, BoardRuntime, JourneyReceipt
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    field,
    file_digest,
    fixture_identity,
    json_value,
    object_value,
    string_value,
)


class CentralPlan(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    repository: Path
    source_root: Path | None = None
    expected_head: str = Field(pattern=r"^[0-9a-f]{40}$")
    artifacts: Path
    central_binary: Path
    runtime_parent: Path
    output_directory: Path
    known_unrelated_changes: tuple[str, ...] = ()
    record_cases: list[str] | None = None
    scenario: t.Literal[
        "fresh-create",
        "records",
        "replication",
        "holder-change",
        "archive",
        "verified-origin",
        "i1",
        "joint",
    ]


class CentralReady(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    kind: t.Literal["ready"]
    url: str
    pid: int


class InstallationIdentity(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    serviceId: str
    machineLabel: str
    peerId: str


def private_json(path: Path, value: JsonObject) -> None:
    with path.open("x", encoding="utf-8") as stream:
        os.chmod(path, 0o600)
        _ = stream.write(json.dumps(value) + "\n")


@t.final
class RealCentral:
    def __init__(
        self, plan: CentralPlan, receipt: JourneyReceipt, projects: list[str]
    ) -> None:
        self.plan = plan
        self.receipt = receipt
        self.root = receipt.directory / "central"
        self.root.mkdir(mode=0o700)
        self.assertion = receipt.directory / "owner-assertion.txt"
        request = receipt.directory / "central-request.json"
        private_json(
            request,
            {
                "root": str(self.root),
                "assertionFile": str(self.assertion),
                "projects": list(projects),
            },
        )
        self.log = (receipt.directory / "central.log").open("xb")
        self.child: subprocess.Popen[bytes] = subprocess.Popen(
            [str(plan.central_binary), str(request)],
            cwd=plan.repository,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.log,
        )
        try:
            self.ready = CentralReady.model_validate(self.reply())
        except Exception:
            self.close()
            raise
        if self.ready.pid != self.child.pid or not self.ready.url.startswith(
            "http://127.0.0.1:"
        ):
            self.close()
            raise JourneyFailure(
                "central readiness identifies a different owned process"
            )
        receipt.record(
            {
                "kind": "centralReady",
                "pid": self.child.pid,
                "url": self.ready.url,
                "binarySha256": file_digest(plan.central_binary),
            }
        )

    def reply(self) -> JsonObject:
        if self.child.stdout is None:
            raise JourneyFailure("central stdout unavailable")
        with selectors.DefaultSelector() as selector:
            _ = selector.register(self.child.stdout, selectors.EVENT_READ)
            if not selector.select(30):
                raise JourneyFailure("central reply exceeded 30 seconds")
            line = t.cast(t.BinaryIO, self.child.stdout).readline()
            return object_value(json_value(line))

    def post(self, route: str, body: JsonObject | None) -> JsonObject:
        request = urllib.request.Request(
            self.ready.url + route,
            data=json.dumps(body).encode(),
            headers={
                "Content-Type": "application/json",
                "Cf-Access-Jwt-Assertion": self.assertion.read_text().strip(),
            },
        )
        response = t.cast(
            http.client.HTTPResponse, urllib.request.urlopen(request, timeout=30)
        )
        with response:
            value = object_value(json_value(response.read()))
        # Fixture credentials and signed assertions never enter receipts.
        self.receipt.record(
            {"kind": "centralCall", "route": route, "request": body, "reply": value}
        )
        return value

    def get(self, route: str) -> JsonObject:
        request = urllib.request.Request(
            self.ready.url + route,
            headers={"Cf-Access-Jwt-Assertion": self.assertion.read_text().strip()},
        )
        response = t.cast(
            http.client.HTTPResponse, urllib.request.urlopen(request, timeout=30)
        )
        with response:
            value = object_value(json_value(response.read()))
        self.receipt.record({"kind": "centralRead", "route": route, "reply": value})
        return value

    def control(self, address: JsonObject, command: JsonObject) -> JsonObject:
        return self.post(
            "/admin/v1/conformance/runtime", {"address": address, "command": command}
        )

    def close(self) -> None:
        child = self.child
        if child.poll() is None:
            if child.stdin is not None:
                child.stdin.close()
            try:
                _ = child.wait(timeout=20)
            except subprocess.TimeoutExpired:
                child.terminate()
                _ = child.wait(timeout=10)
        if child.stdout is not None:
            child.stdout.close()
        self.log.close()
        self.receipt.record(
            {"kind": "centralStopped", "pid": child.pid, "exit": child.returncode}
        )
        if child.returncode != 0:
            raise JourneyFailure("central did not drain successfully")


def machine_assertion(repository: Path, credential: str) -> str:
    """Mirror access_fixture.rs using its checked-in test-only RSA key."""

    def encoded(value: bytes) -> str:
        return base64.urlsafe_b64encode(value).decode().rstrip("=")

    issued = int(time.time())
    header = {"alg": "RS256", "kid": "access-test-key", "typ": "JWT"}
    claims = {
        "iss": "https://conformance-test.cloudflareaccess.com",
        "aud": "test-session-aud",
        "exp": issued + 86400 * 60,
        "iat": issued,
        "type": "app",
        "sub": "",
        "common_name": f"test-client-{hashlib.sha256(credential.encode()).hexdigest()}.access",
    }
    signed = (
        encoded(json.dumps(header).encode())
        + "."
        + encoded(json.dumps(claims).encode())
    )
    key = (
        repository
        / "crates/collaboration-central-conformance/fixtures/access-test-only-rsa.pk8"
    )
    signature = subprocess.run(
        ["openssl", "dgst", "-sha256", "-keyform", "DER", "-sign", str(key)],
        input=signed.encode(),
        capture_output=True,
        check=True,
    ).stdout
    return signed + "." + encoded(signature)


@t.final
class AccessServer(socketserver.ThreadingTCPServer):
    daemon_threads = False
    block_on_close = True


@t.final
class FixtureAccessEdge:
    def __init__(self, repository: Path, upstream_url: str, credential: str) -> None:
        self.sockets: set[socket.socket] = set()
        self.lock = threading.Lock()
        self.failures: list[str] = []
        self.paused = threading.Event()
        assertion = machine_assertion(repository, credential)
        upstream_host, upstream_port = upstream_url.removeprefix("http://").split(":")
        edge = self

        class ForwardHandshake(socketserver.BaseRequestHandler):
            @t.override
            def handle(self) -> None:
                downstream = t.cast(socket.socket, self.request)
                if edge.paused.is_set():
                    return
                upstream: socket.socket | None = None
                with edge.lock:
                    edge.sockets.add(downstream)
                try:
                    downstream.settimeout(10)
                    header = b""
                    while b"\r\n\r\n" not in header:
                        chunk = downstream.recv(4096)
                        if not chunk:
                            return
                        header += chunk
                        if len(header) > 65536:
                            raise JourneyFailure("invalid fixture Access handshake")
                    request_headers, tail = header.split(b"\r\n\r\n", 1)
                    headers = {
                        name.lower(): value.strip()
                        for name, value in (
                            line.split(b":", 1)
                            for line in request_headers.split(b"\r\n")[1:]
                        )
                    }
                    if (
                        headers.get(b"authorization") != f"Bearer {credential}".encode()
                        or headers.get(b"cf-access-client-id")
                        != b"board-fixture-client"
                        or headers.get(b"cf-access-client-secret")
                        != b"board-fixture-secret"
                    ):
                        raise JourneyFailure(
                            "fixture Access edge received a different machine credential"
                        )
                    upstream = socket.create_connection(
                        (upstream_host, int(upstream_port)), timeout=10
                    )
                    with edge.lock:
                        edge.sockets.update([downstream, upstream])
                    upstream.sendall(
                        request_headers
                        + b"\r\nCf-Access-Jwt-Assertion: "
                        + assertion.encode()
                        + b"\r\n\r\n"
                        + tail
                    )
                    downstream.settimeout(None)
                    upstream.settimeout(None)
                    with selectors.DefaultSelector() as selector:
                        _ = selector.register(
                            downstream, selectors.EVENT_READ, upstream
                        )
                        _ = selector.register(
                            upstream, selectors.EVENT_READ, downstream
                        )
                        while True:
                            for selected, _mask in selector.select(30):
                                source = t.cast(socket.socket, selected.fileobj)
                                destination = t.cast(socket.socket, selected.data)
                                data = source.recv(65536)
                                if not data:
                                    return
                                destination.sendall(data)
                except OSError:
                    # Host and edge shutdown close these exact sockets.
                    pass
                except Exception as error:  # noqa: BLE001 -- retain fixture failure without auth bytes
                    edge.failures.append(f"{type(error).__name__}: {error}")
                finally:
                    with edge.lock:
                        edge.sockets.discard(downstream)
                        if upstream is not None:
                            edge.sockets.discard(upstream)
                    if upstream is not None:
                        upstream.close()

        self.server = AccessServer(("127.0.0.1", 0), ForwardHandshake)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.endpoint = f"http://127.0.0.1:{self.server.server_address[1]}/v1/session"

    def pause(self) -> None:
        """Disconnect only this enrolled session and refuse its reconnects."""
        self.paused.set()
        with self.lock:
            for connection in list(self.sockets):
                try:
                    connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

    def resume(self) -> None:
        self.paused.clear()

    def close(self) -> None:
        self.server.shutdown()
        with self.lock:
            for connection in list(self.sockets):
                try:
                    connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                connection.close()
        self.server.server_close()
        self.thread.join(timeout=5)
        if self.thread.is_alive() or self.failures:
            raise JourneyFailure(f"fixture Access edge did not settle: {self.failures}")


def enroll_host(central: RealCentral, runtime: BoardRuntime) -> FixtureAccessEdge:
    runtime.start()
    identity = InstallationIdentity.model_validate(
        BoardClient(runtime).read("installation_show", {})
    )
    runtime.stop()
    credential = "board-fixture-" + fixture_identity()
    credential_hash = hashlib.sha256(credential.encode()).hexdigest()
    # Enrollment is a real owner route. Its test credential remains private.
    request: JsonObject = {
        "kind": "enrollMachine",
        "value": {
            "label": identity.machineLabel,
            "installation": identity.serviceId,
            "peer": identity.peerId,
            "access_client_id": f"test-client-{credential_hash}.access",
            "credential_hash": credential_hash,
            "expires_at": "2033-05-18T03:33:20Z",
        },
    }
    enrolled = central.post("/admin/v1/machines/enroll", request)
    if enrolled.get("kind") != "machineEnrolled":
        raise JourneyFailure("real central refused machine enrollment")
    edge = FixtureAccessEdge(central.plan.repository, central.ready.url, credential)
    session = runtime.directory / "fixture-session.json"
    private_json(
        session,
        {
            "endpoint": edge.endpoint,
            "access": {
                "clientId": "board-fixture-client",
                "clientSecret": "board-fixture-secret",
            },
            "credential": "Bearer " + credential,
            "peer": identity.peerId,
            "receiver": string_value(field(enrolled, "value", "authoritativeReceiver")),
            "relay": string_value(field(enrolled, "value", "relayPeer")),
        },
    )
    runtime.fixture_session = session
    try:
        runtime.start()
    except Exception:
        edge.close()
        raise
    return edge
