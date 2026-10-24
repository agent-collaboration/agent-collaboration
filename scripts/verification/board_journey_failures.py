"""Observe isolated unavailable stores and preserved aggregate resume state."""

import contextlib
import json
import sqlite3
import typing as t

from .board_journey_fixture import BoardFixture, human_actor, project_thread_subject
from .board_journey_runtime import (
    project_request,
    write_header,
)
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
    integer_value,
    object_value,
)


def change_schema_version(
    fixture: BoardFixture, project: str, replacement: int | None
) -> int:
    if fixture.runtime.child is not None:
        raise JourneyFailure("schema fault requires closed native connections")
    path = fixture.database_path(project)
    with contextlib.closing(sqlite3.connect(path)) as connection, connection:
        row = t.cast(
            tuple[int] | None,
            connection.execute("SELECT MAX(version) FROM _sqlx_migrations").fetchone(),
        )
        if row is None:
            raise JourneyFailure("owned schema has no migration history")
        previous = integer_value(row[0])
        version = previous + 1 if replacement is None else replacement
        changed = connection.execute(
            "UPDATE _sqlx_migrations SET version=? WHERE version=?", (version, previous)
        )
        if changed.rowcount != 1:
            raise JourneyFailure(
                "schema fault must affect exactly the latest migration row"
            )
    fixture.receipt.record(
        {
            "kind": "fault",
            "mode": "offline",
            "database": str(path),
            "table": "_sqlx_migrations",
            "previousVersion": previous,
            "replacementVersion": version,
            "lifetime": "until the declared restoration before the next restart",
        }
    )
    return previous


def main_post(fixture: BoardFixture, project: str) -> str:
    """Create a required-subject root with a distinct authorized fixture author."""
    post = fixture_identity()
    author = human_actor(post)
    revision = fixture.main_author_revisions.get(project, 1)
    _ = fixture.client.accepted(
        "role_grant",
        project_request(
            fixture.owner,
            project,
            {
                "kind": "roleGrant",
                "subject": {"kind": "project", "subject": project},
                "role": "advisor",
                "holder": author,
                "model": None,
                "lineage": None,
                "expectedRevision": revision,
                "ownerGrant": None,
            },
        ),
    )
    fixture.main_author_revisions[project] = revision + 1
    _ = fixture.client.accepted(
        "thread_create",
        {
            **write_header(author),
            "postId": post,
            "projectId": project,
            "about": project_thread_subject(f"Healthy main activity {post}"),
            "text": f"exact healthy main post {post}",
            "references": [],
            "addressees": [],
            "role": None,
            "watch": False,
        },
    )
    return post


def unavailable_siblings_preserve_healthy_inbox(fixture: BoardFixture) -> None:
    fixture.initialize()
    healthy_post = main_post(fixture, fixture.project)
    _, broken = fixture.foreign_project()
    thread = fixture.create_thread(broken)
    _ = fixture.client.accepted(
        "thread_watch",
        {
            **write_header(fixture.reader),
            "thread": {"project": broken, "thread": thread},
        },
    )
    fixture.runtime.stop()
    original = change_schema_version(fixture, broken, None)
    fixture.runtime.start()
    first = fixture.inbox()
    fixture.receipt.equal(
        "too-new nonselected store is reported beside the exact healthy page",
        "O-R2-F4/PD2 §6",
        (
            [[healthy_post], [broken]],
            [
                [
                    field(row, "post", "postId")
                    for row in array_value(field(first, "page", "records"))
                ],
                field(first, "page", "unavailable"),
            ],
        ),
    )
    fixture.runtime.stop()
    _ = change_schema_version(fixture, broken, original)
    path = fixture.database_path(broken)
    with contextlib.closing(sqlite3.connect(path)) as connection, connection:
        _ = connection.execute("PRAGMA ignore_check_constraints=ON")
        changed = connection.execute(
            "UPDATE thread_watches SET active=2 WHERE thread_id=? AND reader=?",
            (thread, json.dumps(fixture.reader, separators=(",", ":"))),
        )
        if changed.rowcount != 1:
            raise JourneyFailure("watch probe fault must affect exactly one owned row")
    fixture.receipt.record(
        {
            "kind": "fault",
            "mode": "offline",
            "database": str(path),
            "table": "thread_watches",
            "threadId": thread,
            "reader": fixture.reader,
            "field": "active",
            "value": 2,
            "lifetime": "until this owned fixture is retired",
        }
    )
    fixture.runtime.start()
    second = fixture.inbox()
    fixture.receipt.equal(
        "corrupt nonselected watch probe preserves healthy results and reports B",
        "O-R2-F4",
        (
            [[healthy_post], [broken]],
            [
                [
                    field(row, "post", "postId")
                    for row in array_value(field(second, "page", "records"))
                ],
                field(second, "page", "unavailable"),
            ],
        ),
    )
    direct = object_value(
        fixture.client.call(
            "thread_show",
            {
                "thread": {"project": broken, "thread": thread},
                "reader": {"kind": "identified", "reader": fixture.reader},
            },
            "direct corrupt watch diagnostic",
        )
    )
    fixture.receipt.equal(
        "directly selecting corrupt watch state refuses it",
        "full stored validation",
        (
            True,
            direct.get("isError"),
        ),
    )


def aggregate_cursor_recovers_known_unavailable_project(fixture: BoardFixture) -> None:
    broken = fixture_identity()
    fixture.create_project(broken)
    behind_sweep = main_post(fixture, broken)
    first_healthy = main_post(fixture, fixture.project)
    second_healthy = main_post(fixture, fixture.project)
    saved: JsonObject = {
        "perProject": {
            fixture.project: {"epoch": 1, "position": 0},
            broken: {"epoch": 1, "position": 0},
        },
        "sweep": None,
    }
    selection: JsonObject = {
        "kind": "afterPosition",
        "after": {"kind": "aggregate", "cursor": saved},
    }
    request: JsonObject = {
        "scope": {"kind": "board", "boardId": fixture.board},
        "selection": selection,
        "page": {"limit": 1},
    }
    fixture.runtime.stop()
    original = change_schema_version(fixture, broken, None)
    fixture.runtime.start()
    first = fixture.client.observe(
        "post_list", request, "S-R2-F2/external aggregate initialization"
    )
    fixture.receipt.equal(
        "first aggregate page returns A while explicitly reporting unavailable B",
        "S-R2-F2",
        (
            [[first_healthy], [broken]],
            [
                [
                    field(post, "postId")
                    for post in array_value(field(first, "page", "records"))
                ],
                field(first, "page", "unavailable"),
            ],
        ),
    )
    fixture.receipt.equal(
        "B retains its externally supplied position",
        "S-R2-F2/PD1 aggregate reads",
        (
            {"epoch": 1, "position": 0},
            field(first, "page", "next", "perProject", broken),
        ),
    )
    cursor = field(first, "page", "nextCursor")
    fixture.receipt.equal(
        "first page has a real healthy continuation",
        "S-R2-F2 fixture",
        (
            True,
            isinstance(cursor, str),
        ),
    )
    fixture.runtime.stop()
    _ = change_schema_version(fixture, broken, original)
    fixture.runtime.start()
    observed: list[JsonValue] = [first_healthy]
    for _ in range(3):
        request["page"] = {"limit": 1, "cursor": cursor}
        page = fixture.client.observe(
            "post_list", request, "S-R2-F2/resume after B returns"
        )
        observed.extend(
            field(post, "postId")
            for post in array_value(field(page, "page", "records"))
        )
        cursor = field(page, "page", "nextCursor")
        if cursor is None:
            break
    fixture.receipt.equal(
        "known B's behind-sweep record is returned once before A's remaining record",
        "S-R2-F2",
        (
            [[first_healthy, behind_sweep, second_healthy], None],
            [observed, cursor],
        ),
    )
    fixture.runtime.stop()
    _ = change_schema_version(fixture, broken, None)
    _ = change_schema_version(fixture, fixture.project, None)
    fixture.runtime.start()
    unavailable = fixture.client.observe(
        "post_list",
        {
            "scope": {"kind": "board", "boardId": fixture.board},
            "selection": selection,
            "page": {"limit": 1},
        },
        "S-R2-F2/unavailable-only page",
    )
    fixture.receipt.equal(
        "unavailable-only page terminates without erasing known positions",
        "S-R2-F2",
        (
            [[], None, saved],
            [
                field(unavailable, "page", "records"),
                field(unavailable, "page", "nextCursor"),
                field(unavailable, "page", "next"),
            ],
        ),
    )
