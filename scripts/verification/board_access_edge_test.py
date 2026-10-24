"""A normal empty poll must not end the byte-preserving Access fixture stream."""

import selectors
import socket
import threading
import typing as t
from pathlib import Path

import pytest

from scripts.verification.board_central_runtime import FixtureAccessEdge


def test_empty_poll_keeps_access_fixture_connection_alive(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    original_selector = selectors.DefaultSelector

    @t.final
    class EmptyOnceSelector:
        def __init__(self) -> None:
            self.delegate = original_selector()
            self.first = True

        def __enter__(self) -> t.Self:
            return self

        def __exit__(self, *_arguments: object) -> None:
            self.delegate.close()

        def register(
            self, fileobj: socket.socket, events: int, data: socket.socket
        ) -> selectors.SelectorKey:
            return self.delegate.register(fileobj, events, data)

        def select(
            self, timeout: float | None = None
        ) -> list[tuple[selectors.SelectorKey, int]]:
            if self.first:
                self.first = False
                return []
            return self.delegate.select(timeout)

    monkeypatch.setattr(selectors, "DefaultSelector", EmptyOnceSelector)
    stopped = threading.Event()
    failures: list[str] = []
    reply = b"HTTP/1.1 101 Switching Protocols\r\n\r\nretained-byte-stream"
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
                    connection.sendall(reply)
                    _ = stopped.wait(5)
            except OSError as error:
                failures.append(type(error).__name__)

        worker = threading.Thread(target=upstream)
        worker.start()
        edge = FixtureAccessEdge(Path.cwd(), f"http://127.0.0.1:{port}", "fixture-only")
        try:
            edge_port = int(edge.endpoint.split(":")[2].split("/")[0])
            with socket.create_connection(
                ("127.0.0.1", edge_port), timeout=5
            ) as client:
                client.sendall(
                    b"".join(
                        [
                            b"GET /v1/session HTTP/1.1\r\nHost: fixture\r\n",
                            b"Authorization: Bearer fixture-only\r\n",
                            b"CF-Access-Client-Id: board-fixture-client\r\n",
                            b"CF-Access-Client-Secret: board-fixture-secret\r\n\r\n",
                        ]
                    )
                )
                observed = b""
                while len(observed) < len(reply):
                    chunk = client.recv(4096)
                    if not chunk:
                        break
                    observed += chunk
                assert observed == reply
        finally:
            stopped.set()
            edge.close()
            worker.join(timeout=5)
            assert not worker.is_alive()
        assert failures == []
        assert edge.failures == []
