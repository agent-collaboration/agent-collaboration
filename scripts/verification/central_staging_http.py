"""Bounded HTTP observations with redirects disabled and no credential logging."""

import contextlib
import http.client
import json
import os
import urllib.parse
from dataclasses import dataclass

from .board_journey_values import JsonObject, json_value, object_value
from .staging_http_identity import STAGING_VERIFICATION_USER_AGENT


@dataclass(frozen=True)
class HttpReply:
    status: int
    body: bytes
    auth_reasons: tuple[str, ...]

    def object(self) -> JsonObject:
        return object_value(json_value(self.body))


def environment_value(name: str) -> str:
    value = os.environ.get(name, "")
    if not value or "\r" in value or "\n" in value:
        raise ValueError(
            "a declared credential environment is unavailable or malformed"
        )
    return value


@dataclass(frozen=True, repr=False)
class HttpEndpoint:
    origin: str
    headers: dict[str, str]

    def request(
        self, method: str, path: str, body: JsonObject | None = None
    ) -> HttpReply:
        if not path.startswith("/") or path.startswith("//"):
            raise ValueError("request must remain at the declared origin")
        url = urllib.parse.urlsplit(self.origin)
        connection_type = (
            http.client.HTTPSConnection
            if url.scheme == "https"
            else http.client.HTTPConnection
        )
        with contextlib.closing(
            connection_type(url.hostname or "", url.port, timeout=45)
        ) as connection:
            connection.request(
                method,
                path,
                json.dumps(body).encode() if method == "POST" else None,
                {
                    "Content-Type": "application/json",
                    **self.headers,
                    "User-Agent": STAGING_VERIFICATION_USER_AGENT,
                },
            )
            response = connection.getresponse()
            contents = response.read(16 * 1024 * 1024 + 1)
            if len(contents) > 16 * 1024 * 1024:
                raise ValueError("HTTP observation exceeded the read bound")
            return HttpReply(
                response.status,
                contents,
                tuple(response.headers.get_all("fleet-sync-auth-refusal", [])),
            )

    def post(self, route: str, body: JsonObject | None) -> JsonObject:
        reply = self.request("POST", route, body)
        if reply.status != 200:
            raise ValueError("owner request did not return HTTP 200")
        return reply.object()

    def get(self, route: str) -> JsonObject:
        reply = self.request("GET", route)
        if reply.status != 200:
            raise ValueError("owner read did not return HTTP 200")
        return reply.object()

    def control(self, address: JsonObject, command: JsonObject) -> JsonObject:
        return self.post(
            "/admin/v1/conformance/runtime", {"address": address, "command": command}
        )
