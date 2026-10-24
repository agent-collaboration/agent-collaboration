"""Observe Session transport metadata without changing or retaining its payloads.

This module stays standard-library-only because the same agent runs over SSH.
The controlling process validates exported snapshots at its Pydantic boundary.
"""

import collections
import threading
import typing as t
from dataclasses import dataclass, field

from .board_journey_values import JsonObject, JsonValue, json_value, object_value

_FRAME_BOUND = 16 * 1024 * 1024
_EVENT_BOUND = 1024


@dataclass
class FrameBuffer:
    pending: bytearray = field(default_factory=bytearray)
    fragments: bytearray = field(default_factory=bytearray)
    upgrade_pending: bool = True
    failed: bool = False


@t.final
class SessionAudit:
    def __init__(self) -> None:
        self.lock = threading.RLock()
        self.connections = 0
        self.active: dict[int, FrameBuffer] = {}
        self.welcomes = 0
        self.closed = 0
        self.frames: collections.Counter[str] = collections.Counter()
        self.errors: set[str] = set()
        self.events: collections.deque[JsonObject] = collections.deque(
            maxlen=_EVENT_BOUND
        )
        self.events_dropped = 0

    def record(self, event: JsonObject) -> None:
        if len(self.events) == _EVENT_BOUND:
            self.events_dropped += 1
        self.events.append(event)

    def opened(self) -> int:
        with self.lock:
            self.connections += 1
            connection = self.connections
            self.active[connection] = FrameBuffer()
            self.record({"kind": "connectionOpened", "connection": connection})
            return connection

    def ended(self, connection: int) -> None:
        with self.lock:
            if self.active.pop(connection, None) is not None:
                self.closed += 1
                self.record({"kind": "connectionClosed", "connection": connection})

    def receive(self, connection: int, contents: bytes) -> None:
        with self.lock:
            state = self.active.get(connection)
            if state is not None and state.failed:
                return
            try:
                self.parse(connection, contents)
            except Exception as error:  # noqa: BLE001 -- passive observation must never interrupt the forwarded stream
                self.errors.add(type(error).__name__)
                if state is not None:
                    state.failed = True
                    state.pending.clear()
                    state.fragments.clear()

    def parse(self, connection: int, contents: bytes) -> None:
        state = self.active[connection]
        state.pending.extend(contents)
        if len(state.pending) > _FRAME_BOUND + 65536:
            raise ValueError("audit frame bound exceeded")
        if state.upgrade_pending:
            if b"\r\n\r\n" not in state.pending:
                if len(state.pending) > 65536:
                    raise ValueError("audit upgrade bound exceeded")
                return
            header, tail = bytes(state.pending).split(b"\r\n\r\n", 1)
            if not header.startswith(b"HTTP/1.1 101 "):
                raise ValueError("audit did not observe an upgrade")
            state.upgrade_pending = False
            state.pending.clear()
            state.pending.extend(tail)
        while len(state.pending) >= 2:
            first, second = state.pending[:2]
            if first & 0x70 or second & 0x80:
                raise ValueError("unexpected server frame flags")
            opcode, finished = first & 15, bool(first & 128)
            length, offset = second & 127, 2
            if length in {126, 127}:
                width = 2 if length == 126 else 8
                if len(state.pending) < offset + width:
                    return
                length = int.from_bytes(state.pending[offset : offset + width], "big")
                offset += width
            if length > _FRAME_BOUND:
                raise ValueError("audit frame bound exceeded")
            if len(state.pending) < offset + length:
                return
            payload = bytes(state.pending[offset : offset + length])
            del state.pending[: offset + length]
            if opcode in {8, 9, 10}:
                continue
            if opcode not in {0, 2}:
                raise ValueError("unexpected nonbinary Session frame")
            state.fragments.extend(payload)
            if len(state.fragments) > _FRAME_BOUND:
                raise ValueError("audit fragmented message bound exceeded")
            if not finished:
                continue
            message = object_value(json_value(bytes(state.fragments)))
            state.fragments.clear()
            self.observe_message(connection, message)

    def observe_message(self, connection: int, message: JsonObject) -> None:
        kind = message.get("kind")
        if not isinstance(kind, str):
            raise TypeError("invalid Session frame tag")
        if kind not in {"welcome", "answer", "request", "notice", "closed"}:
            raise ValueError("unknown Session server frame")
        self.frames[kind] += 1
        if kind == "welcome":
            self.welcomes += 1
            self.record({"kind": "welcome", "connection": connection})
        elif kind == "closed":
            self.record({"kind": "serverClosed", "connection": connection})
        elif kind == "notice":
            stream = mail_waiting_stream(message.get("value"), depth=0)
            if stream is not None:
                self.record(
                    {"kind": "mailWaiting", "connection": connection, "stream": stream}
                )

    def snapshot(self) -> JsonObject:
        with self.lock:
            return {
                "connections": self.connections,
                "activeConnections": list[JsonValue](sorted(self.active)),
                "welcomes": self.welcomes,
                "closed": self.closed,
                "frames": dict[str, JsonValue](self.frames),
                "errors": list[JsonValue](sorted(self.errors)),
                "events": list[JsonValue](self.events),
                "eventsDropped": self.events_dropped,
            }


def mail_waiting_stream(value: JsonValue, *, depth: int) -> str | None:
    if depth > 8 or not isinstance(value, dict):
        return None
    nested = value.get("value")
    if value.get("kind") == "mailWaiting" and isinstance(nested, dict):
        stream = nested.get("stream")
        if isinstance(stream, str) and stream in {"items", "controls"}:
            return stream
    for child in value.values():
        found = mail_waiting_stream(child, depth=depth + 1)
        if found is not None:
            return found
    return None
