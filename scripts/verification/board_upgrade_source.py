"""Independent legacy source expectations for the upgrade journey."""

import contextlib
import re
import sqlite3
from datetime import UTC, datetime, timedelta
from pathlib import Path

from pydantic import JsonValue

from .board_store_readback import quoted_identifier
from .board_upgrade_views import Object, SourceRows, int_field, json_object, text_field
from .legacy_selection_oracle import read_rows

SOURCE_TABLES = (
    "board_projects",
    "project_boards",
    "board_topics",
    "board_activity",
    "board_messages",
    "board_threads",
    "board_identities",
    "message_references",
    "thread_participants",
    "project_reader_state",
    "topic_read_bookmarks",
    "thread_read_bookmarks",
    "topic_watches",
    "thread_watches",
    "thread_subscriptions",
    "subscription_windows",
    "actor_board_cooldowns",
)


def source_rows(path: Path) -> SourceRows:
    with contextlib.closing(
        sqlite3.connect(f"{path.resolve().as_uri()}?mode=ro&immutable=1", uri=True)
    ) as connection:
        connection.row_factory = sqlite3.Row
        return SourceRows(
            tables={
                name: [
                    json_object.validate_python(dict(row))
                    for row in read_rows(
                        connection, f"SELECT * FROM {quoted_identifier(name)}"
                    )
                ]
                for name in SOURCE_TABLES
            }
        )


def identity_key(identity: Object) -> str:
    match identity["kind"]:
        case "human":
            return "human:" + text_field(identity, "humanId")
        case "session":
            session = json_object.validate_python(identity["session"])
            endpoint = json_object.validate_python(session["endpoint"])
            return "session:" + ":".join(
                [
                    text_field(endpoint, "serviceId"),
                    text_field(endpoint, "endpointId"),
                    text_field(session, "sessionId"),
                ]
            )
        case _:
            raise ValueError("unexpected non-declared actor in legacy observation")


def source_readers(source: SourceRows) -> list[Object]:
    result: list[Object] = []
    for row in source.tables["board_identities"]:
        if row["kind"] == "human":
            result.append({"kind": "human", "humanId": row["human_id"]})
        elif row["kind"] == "session":
            result.append(
                {
                    "kind": "session",
                    "session": {
                        "endpoint": {
                            "serviceId": row["service_id"],
                            "endpointId": row["endpoint_id"],
                        },
                        "sessionId": row["session_id"],
                    },
                }
            )
        else:
            raise ValueError("unknown source identity")
    return result


def activity_mapping(source: SourceRows, project: str) -> dict[int, int]:
    """PD2: metadata first; root + creator join fuse at the root Post position."""
    all_events = sorted(
        source.tables["board_activity"],
        key=lambda row: int_field(row, "activity_sequence"),
    )
    events = [row for row in all_events if row["topic_id"] == project]
    mapping: dict[int, int] = {}
    position = 1
    for event in events:
        sequence = int_field(event, "activity_sequence")
        if sequence in mapping:
            continue
        position += 2 if event["kind"] == "mainMessageCreated" else 1
        mapping[sequence] = position
        if event["kind"] == "mainMessageCreated":
            following = next(
                (
                    row
                    for row in all_events
                    if int_field(row, "activity_sequence") > sequence
                ),
                None,
            )
            if (
                following is not None
                and following["kind"] == "participantJoined"
                and following["root_id"] == event["message_id"]
                and following["participant_key"] == event["actor_key"]
                and following["topic_id"] == project
            ):
                mapping[int_field(following, "activity_sequence")] = position
    return mapping


def cutoff(mapping: dict[int, int], old: int) -> int:
    return max(
        (position for sequence, position in mapping.items() if sequence <= old),
        default=0,
    )


def canonical_time(value: str) -> str:
    fraction = re.search(r"\.(\d+)", value)
    nanos = (fraction.group(1) if fraction else "").ljust(9, "0")
    instant = datetime.fromisoformat(value).replace(microsecond=0).astimezone(UTC)
    return instant.isoformat(timespec="seconds").replace("+00:00", f".{nanos}Z")


def last_recorded_time(source: SourceRows) -> str:
    values: list[str] = []
    for table, fields in [
        ("thread_subscriptions", ["renewed_at", "ended_at"]),
        ("subscription_windows", ["opened_at", "last_arrival_at", "held_since"]),
    ]:
        for row in source.tables[table]:
            values.extend(
                canonical_time(text_field(row, name))
                for name in fields
                if row[name] is not None
            )
    for row in source.tables["actor_board_cooldowns"]:
        milliseconds = int_field(row, "last_post_at_ms")
        values.append(
            canonical_time(
                (
                    datetime.fromtimestamp(milliseconds // 1000, UTC)
                    + timedelta(milliseconds=milliseconds % 1000)
                ).isoformat()
            )
        )
    if not values:
        raise ValueError("source has no stored event time for a draining ending")
    return max(values)


def interval_history(
    source: SourceRows, project: str, mapping: dict[int, int]
) -> list[list[JsonValue]]:
    """Replay only the adopted join/leave/replacement/resolution interval rules."""
    rows: list[list[JsonValue]] = []
    active: dict[tuple[str, str], list[JsonValue]] = {}
    for event in sorted(
        source.tables["board_activity"],
        key=lambda row: int_field(row, "activity_sequence"),
    ):
        if event["topic_id"] != project:
            continue
        kind = event["kind"]
        if kind in ["mainMessageCreated", "threadMessageCreated", "threadUnresolved"]:
            continue
        thread = text_field(event, "root_id")
        position = mapping[int_field(event, "activity_sequence")]
        if kind == "threadResolved":
            keys = [key for key in active if key[0] == thread]
            for key in keys:
                previous = active.pop(key)
                previous[4:] = [position, "resolved", None]
            continue
        reader = text_field(event, "participant_key")
        key = (thread, reader)
        if kind == "participantLeft":
            previous = active.pop(key)
            previous[4:] = [position, "left", None]
            continue
        if kind in ["orchestratorReplaced", "implementerReplaced"]:
            replaced = text_field(event, "replaced_participant_key")
            previous = active.pop((thread, replaced))
            previous[4:] = [position, "replaced", reader]
        if key in active:
            previous = active.pop(key)
            # ffced1c1 join writes actor=participant; HandTo writes outgoing
            # actor separately from the promoted target (participant_records:258).
            previous[4:] = [
                position,
                "rejoined"
                if kind == "participantJoined" or event["actor_key"] == reader
                else "promoted",
                None,
            ]
        current: list[JsonValue] = [
            thread,
            reader,
            event["participant_role"],
            position,
            None,
            None,
            None,
        ]
        rows.append(current)
        active[key] = current
    return rows
