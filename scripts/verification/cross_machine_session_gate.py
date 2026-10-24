"""Own one byte-preserving machine-session edge, including its outage control.

The native Host still sends its real handshake and protocol frames. This edge
changes only Host routing to the declared upstream origin and adds no identity
or authentication. Fixture Access and staging therefore use the same gate.
"""

import selectors
import socket
import socketserver
import ssl
import threading
import typing as t
import urllib.parse

from .cross_machine_session_audit import SessionAudit


@t.final
class GateServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = False


@t.final
class SessionGate:
    def __init__(self, endpoint: str, *, paused: bool = False) -> None:
        upstream_url = urllib.parse.urlsplit(endpoint)
        if (
            upstream_url.scheme not in {"http", "https", "ws", "wss"}
            or not upstream_url.hostname
            or upstream_url.username
            or upstream_url.password
            or upstream_url.query
            or upstream_url.fragment
            or upstream_url.path != "/v1/session"
            or upstream_url.port in {8787, 8788, 43127}
        ):
            raise ValueError(
                "declare one machine-session origin without credentials or query"
            )
        secure = upstream_url.scheme in {"https", "wss"}
        host, port = upstream_url.hostname, upstream_url.port or (443 if secure else 80)
        self.paused, self.stopping = threading.Event(), threading.Event()
        if paused:
            self.paused.set()
        self.lock = threading.Lock()
        self.sockets: set[socket.socket] = set()
        self.closed = False
        self.failures: list[str] = []
        self.audit = SessionAudit()
        gate = self

        class Forward(socketserver.BaseRequestHandler):
            @t.override
            def handle(self) -> None:
                downstream = t.cast(socket.socket, self.request)
                upstream: socket.socket | None = None
                if gate.paused.is_set() or gate.stopping.is_set():
                    return
                connection_id = gate.audit.opened()
                try:
                    with gate.lock:
                        gate.sockets.add(downstream)
                    downstream.settimeout(10)
                    raw = b""
                    while b"\r\n\r\n" not in raw:
                        chunk = downstream.recv(4096)
                        if not chunk:
                            return
                        raw += chunk
                        if len(raw) > 65536:
                            raise ValueError("session handshake exceeds its bound")
                    header, tail = raw.split(b"\r\n\r\n", 1)
                    lines = header.split(b"\r\n")
                    if not lines[0].startswith(b"GET /v1/session HTTP/1.1"):
                        raise ValueError(
                            "session edge accepts only the declared upgrade route"
                        )
                    rewritten = [
                        lines[0],
                        b"Host: " + upstream_url.netloc.encode("ascii"),
                    ]
                    rewritten.extend(
                        line
                        for line in lines[1:]
                        if not line.lower().startswith(b"host:")
                    )
                    upstream = socket.create_connection((host, port), timeout=10)
                    if secure:
                        upstream = ssl.create_default_context().wrap_socket(
                            upstream, server_hostname=host
                        )
                    with gate.lock:
                        gate.sockets.add(upstream)
                    if gate.paused.is_set() or gate.stopping.is_set():
                        return
                    upstream.sendall(b"\r\n".join(rewritten) + b"\r\n\r\n" + tail)
                    downstream.settimeout(None)
                    upstream.settimeout(None)
                    with selectors.DefaultSelector() as selector:
                        _ = selector.register(
                            downstream, selectors.EVENT_READ, upstream
                        )
                        _ = selector.register(
                            upstream, selectors.EVENT_READ, downstream
                        )
                        while not gate.stopping.is_set():
                            for selected, _mask in selector.select(1):
                                source = t.cast(socket.socket, selected.fileobj)
                                destination = t.cast(socket.socket, selected.data)
                                data = source.recv(65536)
                                if not data:
                                    return
                                if source is upstream:
                                    gate.audit.receive(connection_id, data)
                                destination.sendall(data)
                except OSError:
                    # Intentional pause and shutdown close these exact sockets.
                    return
                except Exception as error:  # noqa: BLE001 -- retain no credential-bearing handshake or exception text
                    gate.failures.append(type(error).__name__)
                finally:
                    gate.audit.ended(connection_id)
                    with gate.lock:
                        gate.sockets.discard(downstream)
                        if upstream is not None:
                            gate.sockets.discard(upstream)
                    if upstream is not None:
                        upstream.close()

        self.server = GateServer(("127.0.0.1", 0), Forward)
        if self.server.server_address[1] in {8787, 8788, 43127}:
            self.server.server_close()
            raise ValueError("session gate selected a protected port")
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.endpoint = f"http://127.0.0.1:{self.server.server_address[1]}/v1/session"

    def pause(self) -> None:
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
        self.stopping.set()
        self.pause()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise RuntimeError("owned session gate did not stop")
        self.closed = True
