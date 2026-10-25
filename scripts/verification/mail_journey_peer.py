"""Owned peer endpoint for observing real Router writes, without an agent turn."""

import hashlib
import json
import secrets
import selectors
import socket
import threading
import typing as t
from pathlib import Path

from .board_journey_peer import qualified_peer_entry
from .board_journey_runtime import BoardRuntime
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    file_digest,
    json_value,
    object_value,
    string_value,
)


@t.final
class MailPeer:
    def __init__(self, runtime: BoardRuntime) -> None:
        self.runtime = runtime
        self.path = runtime.directory / "mail-peer.sock"
        self.token = secrets.token_hex(16)
        self.messages: list[str] = []
        self.reported = 0
        self.error: Exception | None = None
        self.condition = threading.Condition()
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(str(self.path))
        self.listener.listen(32)
        self.cancel_read, self.cancel_write = socket.socketpair()
        self.thread = threading.Thread(target=self.serve, name="owned-m2c-peer")
        self.session: JsonObject = {}

    def start(self) -> None:
        qualification_path = self.runtime.target.peer_qualification
        if qualification_path is None:
            raise JourneyFailure(
                "the peer-entry stand-in requires producer qualification"
            )
        qualification = object_value(json_value(qualification_path.read_bytes()))
        if (
            qualification.get("qualified") is not True
            or qualification.get("actualProducerConstructorAndSerializer") is not True
            or qualification.get("fixtureModuleSha256")
            != file_digest(Path(__file__).with_name("board_journey_peer.py"))
        ):
            raise JourneyFailure("peer producer qualification does not match")
        repository = self.runtime.root / "mail-peer-repository"
        repository.mkdir(mode=0o700)
        directory = self.runtime.directory / "absent-peer-registry"
        directory.mkdir(mode=0o700, exist_ok=True)
        serialized, reference = qualified_peer_entry(repository, self.path)
        entry = object_value(json_value(serialized))
        record = directory / f"{entry['pid']}.json"
        _ = record.write_text(serialized, encoding="utf-8")
        record.chmod(0o600)
        digest = hashlib.sha256(str(self.path).encode()).hexdigest()
        key = directory / f"{entry['pid']}.{digest}.key"
        _ = key.write_text(json.dumps({"peerToken": self.token}), encoding="utf-8")
        key.chmod(0o600)
        self.session = {
            "endpoint": {
                "serviceId": self.runtime.manifest.get("serviceId"),
                "endpointId": "claude-local",
            },
            "sessionId": entry["sessionId"],
        }
        self.runtime.receipt.equal(
            "registered fixture bytes match the qualified producer",
            "named peer-registry stand-in",
            (reference.get("entrySha256"), file_digest(record)),
        )
        self.runtime.receipt.record(
            {
                "kind": "standIn",
                "scope": "producer-qualified peer metadata and owned peer receiver",
                "ownedSocket": str(self.path),
                "session": self.session,
                "producerReference": reference,
                "real": "Host/Router, CLI/MCP, native peer auth/user socket writes",
                "unverified": "actual Claude registry author and agent turn (M4)",
            }
        )
        self.thread.start()

    def serve(self) -> None:
        try:
            with selectors.DefaultSelector() as selector:
                _ = selector.register(self.listener, selectors.EVENT_READ)
                _ = selector.register(self.cancel_read, selectors.EVENT_READ)
                while True:
                    for key, _events in selector.select():
                        if key.fileobj is self.cancel_read:
                            return
                        connection = self.listener.accept()[0]
                        with connection:
                            self.receive(connection)
        # Publish every worker failure so the waiting caller records its cause.
        except Exception as error:  # noqa: BLE001
            with self.condition:
                self.error = error
                self.condition.notify_all()

    def receive(self, connection: socket.socket) -> None:
        connection.settimeout(10)
        with connection.makefile("rb") as reader:
            first = object_value(json_value(reader.readline(1_048_576)))
            if first.get("type") == "journeyBarrier":
                with self.condition:
                    count = len(self.messages)
                connection.sendall(json.dumps({"count": count}).encode() + b"\n")
                return
            if first.get("type") != "auth" or first.get("token") != self.token:
                raise JourneyFailure("owned peer received invalid auth framing")
            frame = object_value(json_value(reader.readline(1_048_576)))
            message = object_value(frame.get("message"))
            if frame.get("type") != "user" or message.get("role") != "user":
                raise JourneyFailure("owned peer received invalid user framing")
            text = string_value(message.get("content"))
            with self.condition:
                self.messages.append(text)
                self.condition.notify_all()

    def wait_for_count(self, expected: int) -> None:
        with self.condition:
            reached = self.condition.wait_for(
                lambda: self.error is not None or len(self.messages) >= expected,
                timeout=15,
            )
            if self.error is not None:
                raise JourneyFailure(f"owned peer failed: {self.error}")
            if not reached:
                raise JourneyFailure(
                    "owned peer did not receive the expected user frame"
                )

    def count_after_barrier(self) -> int:
        # The sequential receiver drains earlier accepted connections before
        # answering this one. A replay cannot hide a second buffered write.
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(10)
            client.connect(str(self.path))
            client.sendall(b'{"type":"journeyBarrier"}\n')
            with client.makefile("rb") as reader:
                answer = object_value(json_value(reader.readline(4096)))
        with self.condition:
            received = list(self.messages[self.reported :])
            first_index = self.reported + 1
            self.reported = len(self.messages)
        for index, text in enumerate(received, start=first_index):
            self.runtime.receipt.record(
                {"kind": "peerWrite", "index": index, "text": text}
            )
        count = answer.get("count")
        if not isinstance(count, int) or isinstance(count, bool):
            raise JourneyFailure("invalid owned peer barrier answer")
        if self.error is not None:
            raise JourneyFailure(f"owned peer failed: {self.error}")
        return count

    def stop(self) -> None:
        if self.thread.ident is not None:
            _ = self.cancel_write.send(b"x")
            self.thread.join(timeout=15)
            if self.thread.is_alive():
                raise JourneyFailure("owned peer did not join")
        self.listener.close()
        self.cancel_read.close()
        self.cancel_write.close()
        self.path.unlink(missing_ok=True)
        if self.error is not None:
            raise JourneyFailure(f"owned peer failed: {self.error}")
