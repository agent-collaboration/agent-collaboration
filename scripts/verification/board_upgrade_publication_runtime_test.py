"""Exercise the interruption gate's actual HTTP ordering and owned teardown."""

import collections.abc
import contextlib
import http.client
import http.server
import json
import threading
import typing as t
from concurrent.futures import ThreadPoolExecutor

import pytest

from scripts.verification.board_upgrade_publication_runtime import CompleteGate
from scripts.verification.board_upgrade_views import Object, json_object


@pytest.fixture
def upstream() -> collections.abc.Iterator[tuple[str, list[Object]]]:
    received: list[Object] = []

    class Sink(http.server.BaseHTTPRequestHandler):
        @t.override
        def log_message(self, format: str, *args: object) -> None:
            return

        def do_POST(self) -> None:
            body = json_object.validate_json(
                self.rfile.read(int(self.headers["Content-Length"]))
            )
            received.append(body)
            encoded = json.dumps(
                {"kind": "imported", "value": {"head": {"epoch": 1, "sequence": 7}}}
            ).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            _ = self.wfile.write(encoded)

    server = http.server.HTTPServer(("127.0.0.1", 0), Sink)
    thread = threading.Thread(
        target=server.serve_forever, kwargs={"poll_interval": 0.01}
    )
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", received
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        assert not thread.is_alive()


def request(port: int, path: str, body: Object) -> Object:
    with contextlib.closing(
        http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    ) as connection:
        connection.request(
            "POST", path, json.dumps(body), {"Content-Type": "application/json"}
        )
        return json_object.validate_json(connection.getresponse().read())


@pytest.mark.parametrize("phase,forwarded", [("before", 0), ("after", 1)])
def test_gate_holds_the_intended_side_of_the_real_request(
    upstream: tuple[str, list[Object]],
    phase: t.Literal["before", "after"],
    forwarded: int,
) -> None:
    # Arrange: an actual HTTP receiver, with a known response independent of the gate.
    url, received = upstream
    gate = CompleteGate(url, phase)
    body: Object = {"kind": "complete", "value": {"digest": "a" * 64}}
    try:
        with ThreadPoolExecutor(max_workers=1) as executor:
            pending = executor.submit(
                request,
                gate.server.server_port,
                "/admin/v1/import/projects/project",
                body,
            )
            try:
                # Act: stop at the operation-owned event, never a timing sleep.
                assert gate.state.reached.wait(3)
                # Assert: before forwards nothing; after has obtained the actual reply.
                assert gate.state.failure is None
                assert len(received) == forwarded
                if phase == "after":
                    assert gate.state.checkpoint is not None
                    assert gate.state.checkpoint["response"] == {
                        "kind": "imported",
                        "value": {"head": {"epoch": 1, "sequence": 7}},
                    }
            finally:
                gate.state.release.set()
            with pytest.raises(http.client.RemoteDisconnected):
                _ = pending.result(timeout=5)
    finally:
        gate.close()


def test_fleet_bundle_without_a_kind_is_forwarded(
    upstream: tuple[str, list[Object]],
) -> None:
    url, received = upstream
    gate = CompleteGate(url, "before")
    bundle: Object = {"provenance": {"machine": "fixture"}, "records": []}
    try:
        response = request(gate.server.server_port, "/admin/v1/import/fleet", bundle)
        assert response["kind"] == "imported"
        assert received == [bundle]
        assert not gate.state.reached.is_set()
    finally:
        gate.close()


def test_gate_refuses_a_non_owned_upstream() -> None:
    with pytest.raises(ValueError, match="owned loopback central"):
        _ = CompleteGate("https://example.test", "before")
