"""Identify verifier HTTP requests while preserving native product headers."""

import contextlib
import http.client
import http.server
import json
import threading
import typing as t
from collections.abc import Generator
from dataclasses import dataclass

import pytest
from websockets.sync.server import ServerConnection, serve

from scripts.verification.board_upgrade_external_transport import (
    MachineReadback,
    OwnerEndpoint,
    OwnerProxy,
)
from scripts.verification.board_upgrade_views import json_object
from scripts.verification.central_staging_http import HttpEndpoint

EXPECTED_USER_AGENT = "codex-router-staging-verification/0.1.102"


@dataclass(frozen=True)
class RequestSeen:
    user_agent: str | None
    authorization: str | None
    body: bytes


@contextlib.contextmanager
def http_capture(
    status: int = 200, body: bytes = b'{"kind":"observed"}'
) -> Generator[tuple[str, list[RequestSeen]], None, None]:
    observed: list[RequestSeen] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        @t.override
        def log_message(self, format: str, *args: object) -> None:
            return

        def do_GET(self) -> None:
            self.respond()

        def do_POST(self) -> None:
            self.respond()

        def respond(self) -> None:
            observed.append(
                RequestSeen(
                    self.headers.get("User-Agent"),
                    self.headers.get("Fixture-Authorization"),
                    self.rfile.read(int(self.headers.get("Content-Length", "0"))),
                )
            )
            self.send_response(status)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            _ = self.wfile.write(body)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever)
    worker.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", observed
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)
        assert not worker.is_alive()


@pytest.mark.parametrize("client", ["owner", "ob7"])
@pytest.mark.parametrize("method", ["GET", "POST"])
def test_verification_http_sends_explicit_user_agent(client: str, method: str) -> None:
    with http_capture() as (origin, observed):
        if client == "owner":
            status, _body = OwnerEndpoint(
                origin, "Fixture-Authorization", "fixture-only"
            ).request(method, "/admin/v1/copies", b"{}" if method == "POST" else None)
        else:
            status = (
                HttpEndpoint(origin, {"Fixture-Authorization": "fixture-only"})
                .request(method, "/admin/v1/copies", {} if method == "POST" else None)
                .status
            )
        assert status == 200
        assert len(observed) == 1
        assert observed[0].user_agent == EXPECTED_USER_AGENT
        assert observed[0].authorization == "fixture-only"
        assert observed[0].body == (b"{}" if method == "POST" else b"")


@pytest.mark.parametrize("product_user_agent", [None, "native-importer/declared", ""])
def test_import_proxy_preserves_product_user_agent_or_absence(
    product_user_agent: str | None,
) -> None:
    with http_capture() as (origin, observed):
        proxy = OwnerProxy(OwnerEndpoint(origin, "Fixture-Authorization", "fixture"))
        try:
            with contextlib.closing(
                http.client.HTTPConnection(
                    "127.0.0.1", proxy.server.server_port, timeout=5
                )
            ) as client:
                headers = (
                    {}
                    if product_user_agent is None
                    else {"User-Agent": product_user_agent}
                )
                client.request(
                    "POST",
                    "/admin/v1/import/projects/fixture",
                    b'{"kind":"page"}',
                    headers,
                )
                reply = client.getresponse()
                assert reply.status == 200
                _ = reply.read()
            assert observed[0].user_agent == product_user_agent
            assert proxy.responses[0]["userAgent"] == product_user_agent
            assert proxy.responses[0]["edgeAdmitted"] is True
        finally:
            proxy.close()


def test_import_proxy_preserves_and_records_product_1010() -> None:
    error_body = b"<h1>Error 1010</h1><p>browser_signature_banned</p>"
    with http_capture(403, error_body) as (origin, observed):
        proxy = OwnerProxy(OwnerEndpoint(origin, "Fixture-Authorization", "fixture"))
        try:
            with contextlib.closing(
                http.client.HTTPConnection(
                    "127.0.0.1", proxy.server.server_port, timeout=5
                )
            ) as client:
                client.request(
                    "POST", "/admin/v1/import/projects/fixture", b'{"kind":"page"}'
                )
                reply = client.getresponse()
                assert reply.status == 403
                assert reply.read() == error_body
            assert observed[0].user_agent is None
            assert proxy.responses[0]["userAgent"] is None
            assert proxy.responses[0]["httpStatus"] == 403
            assert proxy.responses[0]["edgeAdmitted"] is False
            assert proxy.responses[0]["cloudflare1010"] is True
        finally:
            proxy.close()


def test_diagnostic_websocket_sends_verification_user_agent() -> None:
    observed: list[str | None] = []

    def accept(connection: ServerConnection) -> None:
        assert connection.request is not None
        observed.append(connection.request.headers.get("User-Agent"))
        hello = json_object.validate_json(connection.recv(timeout=5))
        value = json_object.validate_python(hello["value"])
        connection.send(
            json.dumps({"kind": "welcome", "value": {"sessionId": value["sessionId"]}})
        )
        connection.close()

    server = serve(accept, "127.0.0.1", 0)
    worker = threading.Thread(target=server.serve_forever)
    worker.start()
    port = t.cast(tuple[str, int], server.socket.getsockname())[1]
    try:
        client = MachineReadback(f"http://127.0.0.1:{port}/v1/session", {})
        try:
            assert observed == [EXPECTED_USER_AGENT]
        finally:
            client.close()
    finally:
        server.shutdown()
        worker.join(timeout=5)
        assert not worker.is_alive()
