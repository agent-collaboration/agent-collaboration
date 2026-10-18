"""Checked JSON values and small command/file helpers shared by the verifier."""

import hashlib
import json
import secrets
import subprocess
import time
import typing as t
import uuid
from pathlib import Path

type JsonValue = (
    None | bool | int | float | str | list[JsonValue] | dict[str, JsonValue]
)
type JsonObject = dict[str, JsonValue]


@t.final
class JourneyFailure(Exception):
    """An observed outcome differs from its declared reference."""


def object_value(value: JsonValue) -> JsonObject:
    if not isinstance(value, dict):
        raise JourneyFailure(f"expected an object, observed {value!r}")
    return value


def array_value(value: JsonValue) -> list[JsonValue]:
    if not isinstance(value, list):
        raise JourneyFailure(f"expected an array, observed {value!r}")
    return value


def string_value(value: JsonValue) -> str:
    if not isinstance(value, str):
        raise JourneyFailure(f"expected a string, observed {value!r}")
    return value


def integer_value(value: JsonValue) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise JourneyFailure(f"expected an integer, observed {value!r}")
    return value


def field(value: JsonValue, *names: str) -> JsonValue:
    for name in names:
        value = object_value(value).get(name)
    return value


def fixture_identity() -> str:
    timestamp = int(time.time() * 1_000)
    bits = timestamp << 80 | 7 << 76 | secrets.randbits(12) << 64
    bits |= 2 << 62 | secrets.randbits(62)
    return str(uuid.UUID(int=bits))


def file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def json_value(contents: str | bytes) -> JsonValue:
    # The JSON library supplies dynamic values; validate their shape at use sites.
    return t.cast(JsonValue, json.loads(contents))


def run_command(
    arguments: list[str], directory: Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        arguments,
        cwd=directory,
        text=True,
        capture_output=True,
        check=False,
        timeout=60,
    )
