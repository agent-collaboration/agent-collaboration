"""Independent legacy inbox oracle for sealed, caller-supplied SQLite copies.

The shipped predicate is inbox_records.rs at ffced1c1:124: a topic bookmark
filters main posts only; watched replies use their own watch and thread mark.
This module reads no importer output and proves no product import by itself.
Run --help for an explicit copy root and private output file.
"""

import argparse
import contextlib
import hashlib
import json
import os
import sqlite3
import tempfile
import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict


class LegacyActivity(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    activity_sequence: int
    project_id: str
    topic_id: str
    root_id: str | None
    kind: str
    actor_key: str
    message_id: str | None


class LegacyMainStart(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    reader_key: str
    project_id: str
    main_start: int | None


class LegacyTopicMark(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    reader_key: str
    topic_id: str
    through_activity: int


class LegacyThreadMark(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    reader_key: str
    root_id: str
    through_activity: int


class LegacyThreadWatch(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    reader_key: str
    root_id: str
    starts_after_activity: int
    active: t.Literal[0, 1]


class LegacyIdentityKey(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    identity_key: str


class LegacyTopicIdentity(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    topic_id: str


class LegacySelection(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    activity: list[LegacyActivity]
    main_starts: list[LegacyMainStart]
    topic_marks: list[LegacyTopicMark]
    thread_marks: list[LegacyThreadMark]
    thread_watches: list[LegacyThreadWatch]
    reader_keys: list[str]
    topic_ids: list[str]


class OracleArguments(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    legacy_copy: Path
    copy_root: Path
    output: Path


class OraclePostSelection(BaseModel):
    reader: str
    legacy_topic: str
    mode: t.Literal["latest", "unread"]
    post_ids: list[str]


class LegacyOracleReceipt(BaseModel):
    kind: t.Literal["legacyOracleOnly"] = "legacyOracleOnly"
    product_import_observed: t.Literal[False] = False
    source_sha256: str
    driver_sha256: str
    activity_count: int
    selections: list[OraclePostSelection]


def read_rows(connection: sqlite3.Connection, query: str) -> list[sqlite3.Row]:
    rows: list[sqlite3.Row] = connection.execute(query).fetchall()
    return rows


def selected_post_ids(
    source: LegacySelection,
    reader: str,
    topic: str,
    mode: t.Literal["latest", "unread"],
) -> list[str]:
    """Project the shipped predicate onto one legacy topic's post set/order."""
    selected: list[tuple[int, str]] = []
    for event in source.activity:
        if event.topic_id != topic or event.message_id is None:
            continue
        if mode == "unread" and event.actor_key == reader:
            continue
        match event.kind:
            case "mainMessageCreated":
                start = next(
                    (
                        row.main_start
                        for row in source.main_starts
                        if row.reader_key == reader
                        and row.project_id == event.project_id
                    ),
                    None,
                )
                mark = next(
                    (
                        row.through_activity
                        for row in source.topic_marks
                        if row.reader_key == reader and row.topic_id == topic
                    ),
                    0,
                )
                include = mode == "latest" or (
                    start is not None and event.activity_sequence > max(start, mark)
                )
            case "threadMessageCreated":
                watch = next(
                    (
                        row
                        for row in source.thread_watches
                        if row.reader_key == reader and row.root_id == event.root_id
                    ),
                    None,
                )
                mark = next(
                    (
                        row.through_activity
                        for row in source.thread_marks
                        if row.reader_key == reader and row.root_id == event.root_id
                    ),
                    0,
                )
                include = (
                    watch is not None
                    and watch.active == 1
                    and (
                        mode == "latest"
                        or event.activity_sequence
                        > max(watch.starts_after_activity, mark)
                    )
                )
            case _:
                include = False
        if include:
            selected.append((event.activity_sequence, event.message_id))
    return [identifier for _, identifier in sorted(selected, reverse=mode == "latest")]


def assert_same_post_selection(expected: list[str], imported: list[str]) -> None:
    if imported != expected:
        raise AssertionError(
            "imported inbox post IDs/order differ from the independently selected legacy posts"
        )


def read_sealed_copy(source_path: Path, copy_root: Path) -> LegacySelection:
    source_path = source_path.resolve(strict=True)
    copy_root = copy_root.resolve(strict=True)
    if not source_path.is_relative_to(copy_root) or any(
        parent.name.startswith((".codex", ".codex-router"))
        for parent in source_path.parents
    ):
        raise ValueError(
            "use only a supplied legacy copy inside its declared copy root, outside normal Codex/Router state"
        )
    if any(
        Path(str(source_path) + suffix).exists()
        for suffix in ["-wal", "-shm", "-journal"]
    ):
        raise ValueError(
            "oracle needs a sealed, consolidated copy without live SQLite sidecars"
        )
    before_digest = hashlib.sha256(source_path.read_bytes()).hexdigest()
    with contextlib.closing(
        sqlite3.connect(f"{source_path.as_uri()}?mode=ro&immutable=1", uri=True)
    ) as connection:
        connection.row_factory = sqlite3.Row
        source = LegacySelection(
            reader_keys=[
                LegacyIdentityKey.model_validate(dict(row)).identity_key
                for row in read_rows(
                    connection,
                    "SELECT identity_key FROM board_identities ORDER BY identity_key",
                )
            ],
            topic_ids=[
                LegacyTopicIdentity.model_validate(dict(row)).topic_id
                for row in read_rows(
                    connection, "SELECT topic_id FROM board_topics ORDER BY topic_id"
                )
            ],
            activity=[
                LegacyActivity.model_validate(dict(row))
                for row in read_rows(
                    connection,
                    "SELECT * FROM board_activity ORDER BY activity_sequence",
                )
            ],
            main_starts=[
                LegacyMainStart.model_validate(dict(row))
                for row in read_rows(connection, "SELECT * FROM project_reader_state")
            ],
            topic_marks=[
                LegacyTopicMark.model_validate(dict(row))
                for row in read_rows(connection, "SELECT * FROM topic_read_bookmarks")
            ],
            thread_marks=[
                LegacyThreadMark.model_validate(dict(row))
                for row in read_rows(connection, "SELECT * FROM thread_read_bookmarks")
            ],
            thread_watches=[
                LegacyThreadWatch.model_validate(dict(row))
                for row in read_rows(connection, "SELECT * FROM thread_watches")
            ],
        )
    if hashlib.sha256(source_path.read_bytes()).hexdigest() != before_digest:
        raise ValueError("supplied source changed during oracle observation")
    return source


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--legacy-copy", type=Path, required=True)
    _ = parser.add_argument("--copy-root", type=Path, required=True)
    _ = parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="New private evidence JSON; never commit private source-derived IDs",
    )
    arguments = OracleArguments.model_validate(vars(parser.parse_args()))
    parent = arguments.output.parent.resolve(strict=True)
    roots = {Path(tempfile.gettempdir()).resolve(), Path("/tmp").resolve()}
    if (
        not arguments.output.is_absolute()
        or parent.stat().st_uid != os.getuid()
        or parent.stat().st_mode & 0o077
        or not any(parent != root and parent.is_relative_to(root) for root in roots)
    ):
        raise ValueError(
            "oracle output needs a private owned directory under system temp"
        )
    source = read_sealed_copy(arguments.legacy_copy, arguments.copy_root)
    observations = [
        OraclePostSelection(
            reader=reader,
            legacy_topic=topic,
            mode=mode,
            post_ids=selected_post_ids(source, reader, topic, mode),
        )
        for reader in source.reader_keys
        for topic in source.topic_ids
        for mode in ("latest", "unread")
    ]
    receipt = LegacyOracleReceipt(
        source_sha256=hashlib.sha256(arguments.legacy_copy.read_bytes()).hexdigest(),
        driver_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        activity_count=len(source.activity),
        selections=observations,
    )
    with arguments.output.open("x", encoding="utf-8") as output:
        _ = output.write(receipt.model_dump_json(indent=2) + "\n")
    print(
        json.dumps(
            {
                "kind": "legacyOracleOnly",
                "activityCount": len(source.activity),
                "selectionCount": len(observations),
                "productAssertions": 0,
            }
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
