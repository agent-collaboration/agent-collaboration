"""Apparatus controls only: real SQLite files, no Router outcome claims."""

import contextlib
import sqlite3
from pathlib import Path

import pytest

from scripts.verification.board_journey_values import JourneyFailure, file_digest
from scripts.verification.board_store_readback import (
    read_stopped_store,
    schema_observations,
)


def test_readback_requires_consumers_to_close(tmp_path: Path) -> None:
    database = tmp_path / "owned.sqlite"
    with contextlib.closing(sqlite3.connect(database)) as owner:
        _ = owner.execute("CREATE TABLE example (item_id TEXT NOT NULL PRIMARY KEY)")
        owner.commit()
        before = file_digest(database)

        with pytest.raises(JourneyFailure, match="closed file"):
            _ = read_stopped_store(database)

        assert file_digest(database) == before
    snapshot = read_stopped_store(database)
    assert snapshot.tables["example"].rows == []


def test_readback_retains_cells_and_changes_no_bytes(tmp_path: Path) -> None:
    database = tmp_path / "owned.sqlite"
    with contextlib.closing(sqlite3.connect(database)) as owner:
        _ = owner.execute(
            "CREATE TABLE example (item_id TEXT NOT NULL PRIMARY KEY, payload BLOB, note TEXT)"
        )
        _ = owner.execute(
            "INSERT INTO example VALUES (?, ?, ?)",
            ("018f0000-0000-7000-8000-000000000001", b"\x00\xff", None),
        )
        owner.commit()
    before = file_digest(database)

    snapshot = read_stopped_store(database)

    assert snapshot.tables["example"].rows == [
        {
            "item_id": "018f0000-0000-7000-8000-000000000001",
            "payload": {"blobHex": "00ff"},
            "note": None,
        }
    ]
    assert snapshot.tables["example"].unique_keys == [["item_id"]]
    assert snapshot.tables["example"].columns[0].key_order == 1
    assert snapshot.file_digests[str(database)] == before == file_digest(database)


def test_expression_index_is_retained_without_inventing_a_column(
    tmp_path: Path,
) -> None:
    database = tmp_path / "owned.sqlite"
    with contextlib.closing(sqlite3.connect(database)) as owner:
        _ = owner.execute(
            "CREATE TABLE example (item_id TEXT NOT NULL PRIMARY KEY, label TEXT)"
        )
        _ = owner.execute("CREATE UNIQUE INDEX label_case ON example(lower(label))")
        owner.commit()

    snapshot = read_stopped_store(database)

    assert [None] in snapshot.tables["example"].unique_keys
    assert (
        snapshot.tables["example"].index_definitions["label_case"]
        == "CREATE UNIQUE INDEX label_case ON example(lower(label))"
    )


@pytest.mark.parametrize(
    "positions,expected",
    [((), False), ((1, 2, 3), True), ((1, 3), False), ((1, 1), False)],
)
def test_nonempty_density_covers_epoch_boundaries(
    tmp_path: Path, positions: tuple[int, ...], expected: bool
) -> None:
    database = tmp_path / "owned.sqlite"
    with contextlib.closing(sqlite3.connect(database)) as owner:
        _ = owner.execute(
            "CREATE TABLE project_feed (entry_id TEXT NOT NULL PRIMARY KEY, epoch INTEGER NOT NULL, position INTEGER NOT NULL, time_basis TEXT NOT NULL, router_time TEXT)"
        )
        for index, position in enumerate(positions, start=1):
            _ = owner.execute(
                "INSERT INTO project_feed VALUES (?, ?, ?, 'router', '2026-10-18T00:00:00Z')",
                (
                    f"018f0000-0000-7000-8000-{index:012x}",
                    1 if index == 1 else 2,
                    position,
                ),
            )
        owner.commit()

    checks = schema_observations(
        read_stopped_store(database), "unused-project", "unused-workstream"
    )
    selected = [
        check
        for check in checks
        if check.label
        in {
            "API-created fixture has a nonempty feed",
            "feed is nonempty with dense positions starting at one",
        }
    ]

    assert len(selected) == 2
    assert all(check.passed for check in selected) is expected


def test_v4_and_retired_schema_are_refused(tmp_path: Path) -> None:
    database = tmp_path / "owned.sqlite"
    with contextlib.closing(sqlite3.connect(database)) as owner:
        _ = owner.execute(
            "CREATE TABLE record_identities (kind TEXT, id TEXT, payload TEXT, PRIMARY KEY(kind, id))"
        )
        _ = owner.execute(
            "CREATE TABLE project_state (project_id TEXT NOT NULL PRIMARY KEY)"
        )
        _ = owner.execute(
            "INSERT INTO project_state VALUES ('018f0000-0000-4000-8000-000000000001')"
        )
        owner.commit()

    checks = {
        check.label: check
        for check in schema_observations(
            read_stopped_store(database), "unused-project", "unused-workstream"
        )
    }

    assert checks["retired M2 tables are absent"].passed is False
    assert checks["retired M2 tables are absent"].observed == ["record_identities"]
    assert checks["project_state row keys are canonical UUIDv7"].passed is False
    assert checks["every PD2 §3a replicated table exists"].passed is False
