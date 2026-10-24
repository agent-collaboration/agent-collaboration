"""Audit transport/export controls; no Host, provider or staging credential needed."""

import contextlib
import json
import selectors
import socket
import subprocess
import sys
import threading
import time
import typing as t
from pathlib import Path

from scripts.verification.board_journey_values import (
    JsonObject,
    field,
    json_value,
    object_value,
)
from scripts.verification.cross_machine_lifecycle import SessionAuditSnapshot
from scripts.verification.cross_machine_session_audit import SessionAudit

CHILD = """
import json,sys
from scripts.verification.cross_machine_agent import session_gate_control
from scripts.verification.cross_machine_session_gate import SessionGate
with_gate = SessionGate(sys.argv[1])
try:
 print(json.dumps({'endpoint':with_gate.endpoint}),flush=True)
 for line in sys.stdin:
  request=json.loads(line)
  if request.get('action')=='stop':break
  print(json.dumps(session_gate_control(with_gate,request.get('action'))),flush=True)
finally:
 with_gate.close()
 print(json.dumps(session_gate_control(with_gate,'observe')),flush=True)
"""


def welcome_frame() -> bytes:
    payload = json.dumps(
        {
            "kind": "welcome",
            "value": {"sessionId": "literal-control-session", "databases": []},
        }
    ).encode()
    assert len(payload) < 126
    return bytes([130, len(payload)]) + payload


def child_reply(child: subprocess.Popen[str]) -> JsonObject:
    assert child.stdout is not None
    with selectors.DefaultSelector() as selector:
        _ = selector.register(child.stdout, selectors.EVENT_READ)
        if not selector.select(5):
            raise RuntimeError("audit child did not reply")
        return object_value(json_value(t.cast(str, child.stdout.readline())))


def exported_audit(child: subprocess.Popen[str], welcomes: int) -> JsonObject:
    assert child.stdin is not None
    deadline = time.monotonic() + 5
    while True:
        _ = child.stdin.write('{"action":"observe"}\n')
        child.stdin.flush()
        reply = child_reply(child)
        _ = SessionAuditSnapshot.model_validate(reply)
        if reply["welcomes"] == welcomes:
            return reply
        if time.monotonic() >= deadline:
            raise RuntimeError("exported audit missed the injected Welcome")
        time.sleep(0.01)


def test_fragmented_server_bytes_count_welcome_without_recording_payload() -> None:
    audit = SessionAudit()
    connection = audit.opened()
    wire = b"HTTP/1.1 101 Switching Protocols\r\n\r\n" + welcome_frame()
    for part in [wire[:9], wire[9:43], wire[43:47], wire[47:]]:
        audit.receive(connection, part)
    observed = audit.snapshot()
    assert observed["connections"] == observed["welcomes"] == 1
    assert observed["errors"] == []
    assert "literal-control-session" not in json.dumps(observed)


def test_child_export_detects_extra_welcome_without_a_reconnect() -> None:
    extra, stopped = threading.Event(), threading.Event()
    failures: list[str] = []
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(5)
        port = t.cast(tuple[str, int], listener.getsockname())[1]
        assert port not in {8787, 8788, 43127}

        def upstream() -> None:
            try:
                connection = listener.accept()[0]
                with connection:
                    connection.settimeout(5)
                    request = b""
                    while b"\r\n\r\n" not in request:
                        request += connection.recv(4096)
                    connection.sendall(
                        b"HTTP/1.1 101 Switching Protocols\r\n\r\n" + welcome_frame()
                    )
                    if not extra.wait(5):
                        raise RuntimeError("extra-Welcome control never armed")
                    connection.sendall(welcome_frame())
                    _ = stopped.wait(5)
            except (OSError, RuntimeError) as error:
                failures.append(type(error).__name__)

        server = threading.Thread(target=upstream)
        server.start()
        child = subprocess.Popen(
            [sys.executable, "-u", "-c", CHILD, f"http://127.0.0.1:{port}/v1/session"],
            cwd=Path(__file__).resolve().parents[2],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            ready = child_reply(child)
            gate_port = int(str(field(ready, "endpoint")).split(":")[2].split("/")[0])
            with socket.create_connection(
                ("127.0.0.1", gate_port), timeout=5
            ) as client:
                client.sendall(b"GET /v1/session HTTP/1.1\r\nHost: fixture\r\n\r\n")
                correct = exported_audit(child, 1)
                assert correct["connections"] == 1
                assert correct["activeConnections"] == [1]
                assert correct["errors"] == []
                extra.set()
                incorrect = exported_audit(child, 2)
                assert incorrect["connections"] == correct["connections"] == 1
                assert incorrect["activeConnections"] == [1]
                assert incorrect["errors"] == []
                assert incorrect["welcomes"] != correct["welcomes"]
        finally:
            extra.set()
            stopped.set()
            if child.stdin is not None:
                with contextlib.suppress(BrokenPipeError):
                    _ = child.stdin.write('{"action":"stop"}\n')
                    child.stdin.flush()
                child.stdin.close()
                child.stdin = None
            final_stdout, stderr = child.communicate(timeout=10)
            final_audit = SessionAuditSnapshot.model_validate_json(final_stdout.strip())
            assert final_audit.connections == 1
            assert final_audit.welcomes == 2
            assert final_audit.activeConnections == []
            assert final_audit.closed == 1
            assert final_audit.errors == []
            server.join(timeout=5)
            assert not server.is_alive()
            assert child.returncode == 0, stderr
        assert failures == []


def test_malformed_audit_records_failure_without_interrupting_forwarding() -> None:
    audit = SessionAudit()
    connection = audit.opened()
    audit.receive(connection, b"HTTP/1.1 101 Switching Protocols\r\n\r\n\x82\x02[]")
    snapshot = audit.snapshot()
    assert snapshot["errors"] != []
    assert snapshot["welcomes"] == 0
    # Once decoding failed, subsequent traffic must not grow a retained buffer.
    audit.receive(connection, b"x" * 65536)
    assert audit.active[connection].pending == bytearray()
    assert audit.active[connection].fragments == bytearray()
