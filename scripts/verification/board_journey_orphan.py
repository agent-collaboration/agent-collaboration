"""Damage one excluded orphan offline and observe scope and direct-read boundaries."""

import contextlib
import sqlite3
from pathlib import Path

from .board_journey_fixture import BoardFixture
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    file_digest,
    object_value,
)


def service_file_digests(directory: Path) -> dict[str, str]:
    return {
        str(path.relative_to(directory)): file_digest(path)
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    }


def damage_rejected_orphan(fixture: BoardFixture, project: str) -> None:
    if fixture.runtime.child is not None:
        raise JourneyFailure("orphan damage requires closed native connections")
    directory = fixture.runtime.directory.resolve()
    path = directory / "projects" / f"{project}.orphaned.sqlite"
    if not path.resolve().is_relative_to(directory) or path.is_symlink():
        raise JourneyFailure("orphan fault escaped the owned runtime directory")
    fixture.receipt.equal(
        "rejected B exists only as the retained orphan file",
        "S-R3-F1/fault precondition",
        ([True, False], [path.is_file(), (path.parent / f"{project}.sqlite").exists()]),
    )
    before = service_file_digests(directory)
    original_size = path.stat().st_size
    damage = b"X" * 8_192
    written = path.write_bytes(damage)
    fixture.receipt.record(
        {
            "kind": "fault",
            "mode": "offline",
            "database": str(path),
            "projectId": project,
            "originalBytes": original_size,
            "originalSha256": before[str(path.relative_to(directory))],
            "damage": {"kind": "replaceAllBytes", "byteHex": "58", "length": 8_192},
            "damagedSha256": file_digest(path),
            "lifetime": "until this owned runtime root is retired; no repair is attempted",
        }
    )
    fixture.receipt.equal(
        "orphan damage is exactly 8192 bytes of 0x58",
        "S-R3-F1/fault identity",
        ([8_192, True], [written, path.read_bytes() == damage]),
    )
    refused = False
    diagnostic = ""
    try:
        with contextlib.closing(
            sqlite3.connect(f"{path.as_uri()}?mode=ro", uri=True)
        ) as connection:
            _ = connection.execute("PRAGMA integrity_check").fetchall()
    except sqlite3.DatabaseError as error:
        refused = True
        diagnostic = str(error)
    fixture.receipt.record(
        {
            "kind": "faultProbe",
            "database": str(path),
            "engine": "stdlib SQLite read-only integrity check; not native product proof",
            "refused": refused,
            "diagnostic": diagnostic,
            "observerClosedBeforeNativeRestart": True,
        }
    )
    fixture.receipt.equal(
        "read-only fixture probe refuses the damaged database",
        "S-R3-F1/fault precondition",
        (True, refused),
    )
    after = service_file_digests(directory)
    changed: list[JsonValue] = [
        name
        for name in sorted(before.keys() | after.keys())
        if before.get(name) != after.get(name)
    ]
    fixture.receipt.equal(
        "damage changed only B's orphan file and preserved healthy scope stores",
        "S-R3-F1/fault isolation",
        ([str(path.relative_to(directory))], changed),
    )


def observe_excluded_orphan_pages(
    fixture: BoardFixture, scopes: list[JsonObject], post: JsonObject
) -> None:
    for scope in scopes:
        answer = fixture.client.observe(
            "post_list",
            {"scope": scope, "selection": {"kind": "latest"}, "page": {"limit": 100}},
            "S-R3-F1/aggregate excludes damaged orphan",
        )
        fixture.receipt.equal(
            f"healthy {scope['kind']} page excludes B from unavailable",
            "S-R3-F1/PD1 §9",
            (
                [[post], [], None],
                [
                    field(answer, "page", "records"),
                    field(answer, "page", "unavailable"),
                    field(answer, "page", "nextCursor"),
                ],
            ),
        )
    inbox = fixture.inbox()
    fixture.receipt.equal(
        "healthy unread inbox excludes B from unavailable",
        "S-R3-F1/inbox scope",
        (
            [[post], [], None],
            [
                [
                    field(row, "post")
                    for row in array_value(field(inbox, "page", "records"))
                ],
                field(inbox, "page", "unavailable"),
                field(inbox, "page", "nextCursor"),
            ],
        ),
    )


def require_damaged_orphan_replay_refusal(
    fixture: BoardFixture, rejected_request: JsonObject
) -> None:
    replay = object_value(
        fixture.client.call(
            "project_create",
            rejected_request,
            "direct rejected creation replay still opens and refuses damaged ledger",
        )
    )
    fixture.receipt.equal(
        "direct rejected B replay remains a strict storage refusal",
        "S-R3-F1/direct ledger boundary",
        (
            [True, "error", "boardUnavailable", "storage"],
            [
                replay.get("isError"),
                field(replay, "value", "mcpResult"),
                field(replay, "value", "kind"),
                field(replay, "value", "stage"),
            ],
        ),
    )
