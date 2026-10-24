"""Bind OB4 to the same explicit owner HTTP edge used by the OB5 apparatus."""

import json
import os
import typing as t
from pathlib import Path

from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    json_value,
    object_value,
)
from .board_upgrade_external_transport import OwnerEndpoint
from .cross_machine_plan import CentralTarget, FixtureOwnerAccess


def private_fixture_assertion(path: Path) -> str:
    metadata = path.lstat()
    if (
        not path.is_absolute()
        or path.resolve(strict=True) != path
        or not path.is_file()
        or metadata.st_uid != os.getuid()
        or metadata.st_mode & 0o077
        or metadata.st_size > 32768
    ):
        raise JourneyFailure(
            "fixture assertion must be a bounded canonical owner-private file"
        )
    token = path.read_text().strip()
    if not token or "\r" in token or "\n" in token:
        raise JourneyFailure("fixture assertion is unavailable")
    return token


@t.final
class MatrixCentral:
    def __init__(self, declaration: CentralTarget) -> None:
        self.declaration = declaration
        if isinstance(declaration.authentication, FixtureOwnerAccess):
            token = private_fixture_assertion(declaration.authentication.assertion_file)
            header = "Cf-Access-Jwt-Assertion"
        else:
            token = os.environ.get(declaration.authentication.token_environment, "")
            if not token or "\r" in token or "\n" in token:
                raise JourneyFailure("declared owner-token environment is unavailable")
            header = "cf-access-token"
        self.owner = OwnerEndpoint(declaration.endpoint.rstrip("/"), header, token)

    def post(self, path: str, body: JsonObject | None) -> JsonObject:
        return self.owner.call(path, body)

    def get(self, path: str) -> JsonObject:
        return self.owner.call(path, method="GET")

    def post_json(self, path: str, body: JsonValue) -> JsonObject:
        status, contents = self.owner.request("POST", path, json.dumps(body).encode())
        if status != 200:
            raise JourneyFailure(f"declared owner JSON route returned HTTP {status}")
        return object_value(json_value(contents))
