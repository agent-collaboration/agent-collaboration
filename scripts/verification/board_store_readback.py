"""Read a stopped project file and check PD2 §3a without changing its schema.

The caller must settle its clients and close the owning Host first. This helper
also refuses files with open consumers. It uses SQLite's read-only file route,
not the library writer; only a real Host restart qualifies owning reopen.
"""

import contextlib
import json
import sqlite3
import subprocess
import typing as t
import uuid
from pathlib import Path

from pydantic import BaseModel, ConfigDict, JsonValue, TypeAdapter

from .board_journey_values import JourneyFailure, file_digest

# Adopted oracle: PD2 §3a.1 at the board design's 3dd15c23 revision.
REPLICATED_KEYS: t.Final[dict[str, str]] = {
    "project_state": "project_id",
    "workstreams": "workstream_id",
    "tasks": "task_id",
    "task_links": "link_id",
    "task_link_events": "event_id",
    "subtasks": "subtask_id",
    "actors": "actor_id",
    "threads": "thread_id",
    "thread_refilings": "refiling_id",
    "posts": "post_id",
    "post_references": "reference_id",
    "thread_participation": "participation_id",
    "role_assignments": "assignment_id",
    "owner_grants": "grant_id",
    "stand_down_acknowledgements": "acknowledgement_id",
    "work_trail_rows": "row_id",
    "work_trail_tags": "tag_id",
    "work_trail_links": "link_id",
    "moves": "move_id",
    "facts": "fact_id",
    "plan_revisions": "revision_id",
    "read_marks": "mark_id",
    "thread_watches": "watch_id",
    "project_watches": "watch_id",
    "subscription_memberships": "membership_id",
    "notify_intents": "intent_id",
    "mail_handoffs": "item_id",
    "handoff_frontier": "frontier_id",
    "request_decisions": "request_id",
    "epoch_lines": "line_id",
    "project_feed": "entry_id",
}
BUSINESS_KEYS: t.Final[dict[str, tuple[str, ...]]] = {
    "task_links": ("from_task", "link_kind", "to_task"),
    "actors": ("canonical_identity",),
    "post_references": ("source_post", "place"),
    "thread_participation": (
        "actor_id",
        "thread_id",
        "joined_epoch",
        "joined_position",
    ),
    "role_assignments": ("owner_grant",),
    "work_trail_tags": ("row_id", "place"),
    "work_trail_links": ("row_id", "place"),
    "plan_revisions": ("content_hash",),
    "read_marks": ("reader_actor", "scope_kind", "scope_id"),
    "thread_watches": ("reader_actor", "thread_id"),
    "project_watches": ("reader_actor",),
    "subscription_memberships": (
        "reader_actor",
        "scope_kind",
        "scope_id",
        "generation",
    ),
    "notify_intents": ("cause_kind", "cause_id", "recipient_key"),
    "mail_handoffs": ("handoff_seq",),
    "epoch_lines": ("epoch",),
    "project_feed": ("epoch", "position"),
}
RETIRED_TABLES: t.Final[set[str]] = {
    "record_identities",
    "fleet_records",
    "feed_checkpoint",
    "writer_epochs",
    "request_ledger",
    "legacy_cursor_mappings",
}
_json_adapter = TypeAdapter(JsonValue)


class ReadbackModel(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")


class StoredColumn(ReadbackModel):
    name: str
    declared_type: str
    not_null: bool
    key_order: int


class StoredTable(ReadbackModel):
    definition: str
    columns: list[StoredColumn]
    unique_keys: list[list[str | None]]
    index_definitions: dict[str, str | None]
    rows: list[dict[str, JsonValue]]


class ProjectReadback(ReadbackModel):
    database: str
    engine: str = "SQLite read-only, after owning Host exit"
    file_digests: dict[str, str]
    auxiliary_digests_before: dict[str, str]
    auxiliary_digests_after: dict[str, str]
    tables: dict[str, StoredTable]
    reserved_table_names: list[str]


class SchemaObservation(ReadbackModel):
    label: str
    expected: JsonValue
    observed: JsonValue
    passed: bool


def quoted_identifier(name: str) -> str:
    return '"' + name.replace('"', '""') + '"'


def sqlite_value(value: object) -> JsonValue:
    if isinstance(value, bytes):
        return {"blobHex": value.hex()}
    return _json_adapter.validate_python(value)


def read_stopped_store(database: Path) -> ProjectReadback:
    database = database.resolve(strict=True)
    files = [
        path
        for path in [database, Path(f"{database}-wal"), Path(f"{database}-shm")]
        if path.exists()
    ]
    consumers = subprocess.run(
        ["lsof", "-t", "--", *map(str, files)],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if (
        consumers.returncode != 1
        or consumers.stdout.strip()
        or consumers.stderr.strip()
    ):
        raise JourneyFailure(
            f"store consumer check did not establish a closed file: {consumers.returncode}, "
            + f"stdout={consumers.stdout!r}, stderr={consumers.stderr!r}"
        )
    before = {
        str(path): file_digest(path) for path in files if not str(path).endswith("-shm")
    }
    auxiliary_before = {
        str(path): file_digest(path) for path in files if str(path).endswith("-shm")
    }
    tables: dict[str, StoredTable] = {}
    reserved: list[str] = []
    with contextlib.closing(
        sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)
    ) as connection:
        _ = connection.execute("PRAGMA query_only = ON")
        _ = connection.execute("BEGIN")
        names = t.cast(
            list[tuple[str, str]],
            connection.execute(
                "SELECT name, sql FROM sqlite_schema WHERE type='table' ORDER BY name"
            ).fetchall(),
        )
        for name, definition in names:
            if name.startswith(("sqlite_", "_fleet_sync_", "_sqlx_")):
                reserved.append(name)
                continue
            identifier = quoted_identifier(name)
            column_rows = t.cast(
                list[tuple[int, str, str, int, object, int]],
                connection.execute(f"PRAGMA table_info({identifier})").fetchall(),
            )
            columns = [
                StoredColumn(
                    name=row[1],
                    declared_type=row[2],
                    not_null=bool(row[3]),
                    key_order=row[5],
                )
                for row in column_rows
            ]
            unique_keys: list[list[str | None]] = []
            index_definitions = dict(
                t.cast(
                    list[tuple[str, str | None]],
                    connection.execute(
                        "SELECT name, sql FROM sqlite_schema WHERE type='index' AND tbl_name=? ORDER BY name",
                        (name,),
                    ).fetchall(),
                )
            )
            indices = t.cast(
                list[tuple[int, str, int, str, int]],
                connection.execute(f"PRAGMA index_list({identifier})").fetchall(),
            )
            for index in indices:
                if index[2] != 1:
                    continue
                index_columns = t.cast(
                    list[tuple[int, int, str | None]],
                    connection.execute(
                        f"PRAGMA index_info({quoted_identifier(index[1])})"
                    ).fetchall(),
                )
                unique_keys.append([entry[2] for entry in index_columns])
            raw_rows = t.cast(
                list[tuple[object, ...]],
                connection.execute(
                    f"SELECT * FROM {identifier} LIMIT 10001"
                ).fetchall(),
            )
            if len(raw_rows) > 10000:
                raise JourneyFailure(
                    f"owned fixture exceeded the readback bound: {name}"
                )
            rows = [
                {
                    column.name: sqlite_value(value)
                    for column, value in zip(columns, row, strict=True)
                }
                for row in raw_rows
            ]
            rows.sort(key=lambda row: json.dumps(row, sort_keys=True))
            tables[name] = StoredTable(
                definition=definition,
                columns=columns,
                unique_keys=sorted(unique_keys, key=lambda key: json.dumps(key)),
                index_definitions=index_definitions,
                rows=rows,
            )
        connection.rollback()
    after = {
        str(path): file_digest(path)
        for path in [database, Path(f"{database}-wal")]
        if path.exists()
    }
    if before != after:
        raise JourneyFailure(
            f"project or durable WAL bytes changed during read-only inspection: before={before}, after={after}"
        )
    # SQLite may build its non-durable WAL index even when the database is open
    # mode=ro. Retain that fact separately; never silently exempt database/WAL.
    auxiliary_after = {
        str(path): file_digest(path)
        for path in [Path(f"{database}-shm")]
        if path.exists()
    }
    return ProjectReadback(
        database=str(database),
        file_digests=after,
        auxiliary_digests_before=auxiliary_before,
        auxiliary_digests_after=auxiliary_after,
        tables=tables,
        reserved_table_names=reserved,
    )


def schema_observations(
    snapshot: ProjectReadback, project: str, workstream: str
) -> list[SchemaObservation]:
    observations: list[SchemaObservation] = []

    def observe(label: str, expected: object, observed: object) -> None:
        observations.append(
            SchemaObservation(
                label=label,
                expected=_json_adapter.validate_python(expected),
                observed=_json_adapter.validate_python(observed),
                passed=expected == observed,
            )
        )

    names = set(snapshot.tables)
    observe(
        "every PD2 §3a replicated table exists",
        [],
        sorted(set(REPLICATED_KEYS) - names),
    )
    observe(
        "retired M2 tables are absent",
        [],
        sorted(
            names & RETIRED_TABLES
            | {name for name in names if name.startswith("_replication_")}
        ),
    )
    observe("capture bookkeeping exists", True, "capture_feed_marks" in names)
    for table_name, key in REPLICATED_KEYS.items():
        table = snapshot.tables.get(table_name)
        if table is None:
            continue
        primary = sorted(
            (column for column in table.columns if column.key_order),
            key=lambda column: column.key_order,
        )
        observe(
            f"{table_name} has its one TEXT NOT NULL primary key",
            [[key, "TEXT", True]],
            [
                [column.name, column.declared_type.upper(), column.not_null]
                for column in primary
            ],
        )
        invalid: list[JsonValue] = []
        for row in table.rows:
            value = row.get(key)
            try:
                parsed = uuid.UUID(value) if isinstance(value, str) else None
                valid = (
                    parsed is not None and parsed.version == 7 and str(parsed) == value
                )
            except ValueError:
                valid = False
            if not valid:
                invalid.append(value)
        observe(f"{table_name} row keys are canonical UUIDv7", [], invalid)
        business_key = BUSINESS_KEYS.get(table_name)
        if business_key is not None:
            observe(
                f"{table_name} enforces its business key separately",
                True,
                list(business_key) in table.unique_keys,
            )
    for table_name, key, expected in [
        ("project_state", "project_id", project),
        ("workstreams", "workstream_id", workstream),
    ]:
        table = snapshot.tables.get(table_name)
        observe(
            f"{table_name} contains the API-created fixture identity",
            True,
            table is not None and any(row.get(key) == expected for row in table.rows),
        )
    feed = snapshot.tables.get("project_feed")
    if feed is not None:
        observe("API-created fixture has a nonempty feed", True, bool(feed.rows))
        positions = [row.get("position") for row in feed.rows]
        numeric = [
            value
            for value in positions
            if isinstance(value, int) and not isinstance(value, bool)
        ]
        observe(
            "feed is nonempty with dense positions starting at one",
            list(range(1, len(positions) + 1)),
            sorted(numeric),
        )
        bad_times: list[JsonValue] = [
            row
            for row in feed.rows
            if row.get("time_basis") != "router" or row.get("router_time") is None
        ]
        observe("every live feed row has Router time and router basis", [], bad_times)
    return observations
