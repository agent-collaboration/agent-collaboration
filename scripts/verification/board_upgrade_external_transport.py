"""Authenticated external owner HTTP and correlated machine-session readbacks.

No credential values are returned in receipts. HTTP redirects are not followed;
the WebSocket uses one preconnected socket, which also forbids redirects.
"""

import contextlib
import http.client
import http.server
import importlib
import json
import os
import re
import socket
import subprocess
import threading
import time
import typing as t
import urllib.parse
from dataclasses import dataclass

from .board_journey_values import fixture_identity
from .board_upgrade_external_plan import ExternalPlan
from .board_upgrade_views import Object, int_field, json_object
from .staging_http_identity import STAGING_VERIFICATION_USER_AGENT


@dataclass(repr=False)
class OwnerEndpoint:
    url: str
    header: str
    token: str

    @classmethod
    def open(cls, plan: ExternalPlan) -> "OwnerEndpoint":
        plan.validate_endpoint()
        if plan.authentication == "localFixture":
            if plan.fixture_assertion is None:
                raise ValueError("fixture assertion missing")
            path = plan.fixture_assertion
            metadata = path.lstat()
            if (
                not path.is_absolute()
                or path.is_symlink()
                or not path.is_file()
                or metadata.st_mode & 0o077
                or metadata.st_uid != os.getuid()
            ):
                raise ValueError("fixture assertion must be owner-private")
            return cls(
                plan.endpoint.rstrip("/"),
                "Cf-Access-Jwt-Assertion",
                path.read_text().strip(),
            )
        result = subprocess.run(
            ["cloudflared", "access", "token", f"-app={plan.endpoint}"],
            capture_output=True,
            timeout=30,
            check=False,
        )
        token = result.stdout.decode().strip()
        if result.returncode or token.count(".") != 2:
            raise ValueError(
                "owner sign-in required: cloudflared access login for the declared admin app"
            )
        return cls(plan.endpoint.rstrip("/"), "cf-access-token", token)

    def request(
        self,
        method: str,
        path: str,
        body: bytes | None = None,
        *,
        user_agent: str | None = STAGING_VERIFICATION_USER_AGENT,
    ) -> tuple[int, bytes]:
        if (
            not path.startswith("/admin/v1/")
            or path.startswith("//")
            or "?" in path.split("/", 3)[2]
        ):
            raise ValueError("owner request outside the declared admin origin")
        url = urllib.parse.urlsplit(self.url)
        connection_class = (
            http.client.HTTPSConnection
            if url.scheme == "https"
            else http.client.HTTPConnection
        )
        headers = {"Content-Type": "application/json", self.header: self.token}
        if user_agent is not None:
            headers["User-Agent"] = user_agent
        with contextlib.closing(
            connection_class(url.hostname or "", url.port, timeout=30)
        ) as connection:
            connection.request(
                method,
                path,
                body,
                headers,
            )
            response = connection.getresponse()
            contents = response.read(16 * 1024 * 1024 + 1)
            if len(contents) > 16 * 1024 * 1024:
                raise ValueError("owner response exceeds observation bound")
            return response.status, contents

    def call(
        self, path: str, body: Object | None = None, *, method: str = "POST"
    ) -> Object:
        status, contents = self.request(
            method, path, json.dumps(body).encode() if method == "POST" else None
        )
        if status != 200:
            raise ValueError(f"owner route returned HTTP {status}")
        return json_object.validate_json(contents)

    def control(self, address: Object, command: Object) -> Object:
        return self.call(
            "/admin/v1/conformance/runtime", {"address": address, "command": command}
        )


@t.final
class OwnerProxy:
    """Transport the CLI to one declared origin; keep owner authentication in memory."""

    def __init__(
        self, owner: OwnerEndpoint, *, after_publication_fault: bool = False
    ) -> None:
        self.completed: list[Object] = []
        self.responses: list[Object] = []
        self.failure: str | None = None
        proxy = self

        class Forward(http.server.BaseHTTPRequestHandler):
            @t.override
            def log_message(self, format: str, *args: object) -> None:
                return

            def do_POST(self) -> None:
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    if not 0 < length <= 4 * 1024 * 1024:
                        raise ValueError("CLI request body outside snapshot bound")
                    raw = self.rfile.read(length)
                    body = json_object.validate_json(raw)
                    complete = (
                        "/admin/v1/import/projects/" in self.path
                        and body.get("kind") == "complete"
                    )
                    project = self.path.rsplit("/", 1)[-1]
                    arm = complete and after_publication_fault and not proxy.completed
                    if arm:
                        _ = owner.control(
                            {"kind": "copy", "value": project},
                            {
                                "kind": "dropNextObjectReply",
                                "value": {"destination": {"kind": "fleet"}},
                            },
                        )
                    # This is the product's request, not a verifier-originated
                    # request: preserve its User-Agent, including absence.
                    user_agent = self.headers.get("User-Agent")
                    status, contents = owner.request(
                        "POST", self.path, raw, user_agent=user_agent
                    )
                    reply: Object | None
                    try:
                        reply = json_object.validate_json(contents)
                    except ValueError:
                        # Edge refusals can be HTML. Preserve their actual
                        # status/body instead of turning them into proxy 502s.
                        reply = None
                    cloudflare_1010 = status == 403 and (
                        b"browser_signature_banned" in contents.lower()
                        or re.search(
                            rb"\berror(?:\s+code)?\s*:?\s*1010\b", contents.lower()
                        )
                        is not None
                    )
                    observation: Object = {
                        "path": self.path,
                        "requestKind": body.get("kind"),
                        "httpStatus": status,
                        "userAgent": user_agent,
                        "edgeAdmitted": False
                        if cloudflare_1010
                        else True
                        if status == 200 and reply is not None
                        else None,
                        "cloudflare1010": cloudflare_1010,
                        "reply": reply,
                    }
                    proxy.responses.append(observation)
                    if complete:
                        observation = {**observation, "project": project}
                        if arm:
                            observation["fault"] = owner.control(
                                {"kind": "copy", "value": project},
                                {"kind": "awaitDroppedObjectReply"},
                            )
                        proxy.completed.append(observation)
                    self.send_response(status)
                    if reply is not None:
                        self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(contents)))
                    self.end_headers()
                    _ = self.wfile.write(contents)
                except (BrokenPipeError, ConnectionResetError):
                    return
                except Exception as error:  # noqa: BLE001 -- never retain credential-bearing exception text
                    proxy.failure = type(error).__name__
                    self.send_error(502, "declared owner transport failed")

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Forward)
        self.server.daemon_threads = False
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}"

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise RuntimeError("owned owner proxy did not stop")


@t.final
class MachineReadback:
    def __init__(
        self, endpoint: str, headers: dict[str, str], *, session_id: str | None = None
    ) -> None:
        self.snapshot_cut: int | None = None
        url = urllib.parse.urlsplit(endpoint)
        if url.scheme not in {"http", "https", "ws", "wss"} or not url.hostname:
            raise ValueError("invalid declared machine session endpoint")
        secure = url.scheme in {"https", "wss"}
        scheme = "wss" if secure else "ws"
        uri = urllib.parse.urlunsplit((scheme, url.netloc, "/v1/session", "", ""))
        stream = socket.create_connection(
            (url.hostname, url.port or (443 if secure else 80)), timeout=10
        )
        try:
            # Optional G4 transport dependency is loaded only by a real drive;
            # entry/help and the repository's existing CI need no new package.
            connect = t.cast(
                t.Callable[..., MachineSocket],
                importlib.import_module("websockets.sync.client").connect,
            )
            self.socket = connect(
                uri,
                sock=stream,
                additional_headers=headers,
                user_agent_header=STAGING_VERIFICATION_USER_AGENT,
                proxy=None,
                open_timeout=15,
                close_timeout=5,
                max_size=16 * 1024 * 1024,
                legacy=True,
            )
        except BaseException:
            stream.close()
            raise
        try:
            session = fixture_identity() if session_id is None else session_id
            self.socket.send(
                json.dumps(
                    {
                        "kind": "hello",
                        "value": {
                            "sessionId": session,
                            "schemaSupport": {"maxVersion": 1},
                        },
                    }
                )
            )
            welcome = self.receive()
            if (
                welcome.get("kind") != "welcome"
                or json_object.validate_python(welcome["value"]).get("sessionId")
                != session
            ):
                raise ValueError("normal machine session did not welcome")
        except BaseException:
            self.close()
            raise

    def receive(self) -> Object:
        return json_object.validate_json(self.socket.recv(timeout=30))

    def operation(self, body: Object) -> Object:
        frame = fixture_identity()
        self.socket.send(
            json.dumps(
                {"kind": "request", "value": {"frame": frame, "body": body}}
            ).encode()
        )
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            message = self.receive()
            if message.get("kind") == "notice":
                continue
            value = json_object.validate_python(message.get("value"))
            if message.get("kind") != "answer" or value.get("frame") != frame:
                raise ValueError("machine answer correlation differs")
            return json_object.validate_python(value["body"])
        raise TimeoutError("machine operation did not answer")

    def pages(self, database: str) -> list[Object]:
        pages: list[Object] = []
        cursor: Object | None = None
        for _ in range(2048):
            answer = self.operation(
                {
                    "kind": "replication",
                    "value": {
                        "kind": "snapshotPage",
                        "value": {"database": database, "cursor": cursor},
                    },
                }
            )
            response = json_object.validate_python(answer["value"])
            if (
                answer.get("kind") != "replication"
                or response.get("kind") != "snapshotPage"
            ):
                raise ValueError("copy snapshot answer family differs")
            snapshot = json_object.validate_python(response["value"])
            if snapshot.get("kind") != "page":
                raise ValueError("copy snapshot unavailable or moved")
            page = json_object.validate_python(snapshot["value"])
            pages.append(page)
            next_cursor = page.get("next")
            if next_cursor is None:
                return pages
            following = json_object.validate_python(next_cursor)
            if following == cursor:
                raise ValueError("copy snapshot cursor stalled")
            cursor = following
        raise ValueError("copy snapshot exceeds declared page bound")

    def fleet(self, snapshot: bool, *, after: int | None = None) -> list[Object]:
        records: list[Object] = []
        cursor: Object | None = (
            None if snapshot or after is None else {"kind": "fleet", "value": after}
        )
        cut: object = None
        for _ in range(2048):
            answer = self.operation(
                {
                    "kind": "consumer",
                    "value": {
                        "kind": "read",
                        "value": {
                            "scope": {
                                "kind": "fleetSnapshot" if snapshot else "fleetRecords"
                            },
                            "cursor": cursor,
                        },
                    },
                }
            )
            consumer = json_object.validate_python(answer["value"])
            page = json_object.validate_python(consumer["value"])
            if answer.get("kind") != "consumer" or consumer.get("kind") != "read":
                raise ValueError("fleet read family differs")
            if snapshot:
                if page.get("kind") != "fleetSnapshot":
                    raise ValueError("fleet snapshot family differs")
                data = json_object.validate_python(page["value"])
                if cut is not None and cut != data["snapshotAt"]:
                    raise ValueError("fleet snapshot cut changed")
                cut = data["snapshotAt"]
                self.snapshot_cut = int_field(data, "snapshotAt")
                next_cursor = data.get("next")
                next_kind = "fleetSnapshot"
            else:
                feed = json_object.validate_python(page["value"])
                if page.get("kind") != "fleetFeed" or feed.get("kind") != "page":
                    raise ValueError("fleet feed unavailable")
                data = json_object.validate_python(feed["value"])
                next_cursor = (
                    None if data["through"] == data["head"] else data["through"]
                )
                next_kind = "fleet"
            raw_records = data["records"]
            if not isinstance(raw_records, list):
                raise TypeError("fleet records not an array")
            records.extend(
                json_object.validate_python(record) for record in raw_records
            )
            if next_cursor is None:
                return records
            following: Object = {"kind": next_kind, "value": next_cursor}
            if following == cursor:
                raise ValueError("fleet cursor stalled")
            cursor = following
        raise ValueError("fleet exceeds observation page bound")

    def close(self) -> None:
        self.socket.close()


class MachineSocket(t.Protocol):
    def send(self, message: str | bytes) -> None: ...
    def recv(self, timeout: float) -> str | bytes: ...
    def close(self) -> None: ...
