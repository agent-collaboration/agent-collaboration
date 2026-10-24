"""Clone retained synthetic legacy fixtures into one fresh identity namespace.

This changes fixture identities, not the importer or the derivation oracle.
Every cross-table and embedded UUID reference uses the same replacement map.
"""

import contextlib
import re
import shutil
import sqlite3
import typing as t
import uuid
from pathlib import Path

from .board_journey_values import fixture_identity
from .board_store_readback import quoted_identifier

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")


def clone_fixtures(repository: Path, root: Path, names: list[str]) -> list[Path]:
    root.mkdir(mode=0o700)
    paths: list[Path] = []
    identities: dict[str, str] = {}
    for name in names:
        source = repository / "crates/board-legacy-upgrade/tests/fixtures" / name
        target = root / name
        _ = shutil.copyfile(source, target)
        target.chmod(0o600)
        paths.append(target)
        with contextlib.closing(sqlite3.connect(target)) as connection:
            tables = t.cast(
                list[tuple[str]],
                connection.execute(
                    "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'"
                ).fetchall(),
            )
            for (table,) in tables:
                rows = t.cast(
                    list[tuple[object, ...]],
                    connection.execute(
                        f"SELECT * FROM {quoted_identifier(table)}"
                    ).fetchall(),
                )
                for row in rows:
                    for value in row:
                        if isinstance(value, str):
                            for identity in t.cast(list[str], UUID.findall(value)):
                                if identity not in identities:
                                    identities[identity] = (
                                        fixture_identity()
                                        if uuid.UUID(identity).version == 7
                                        else str(uuid.uuid4())
                                    )
    namespace = fixture_identity()
    for target in paths:
        with contextlib.closing(sqlite3.connect(target)) as connection:
            # These are closed disposable copies. Disable FK admission while all
            # references are replaced, then require relational integrity again.
            _ = connection.execute("PRAGMA foreign_keys=OFF")
            tables = t.cast(
                list[tuple[str]],
                connection.execute(
                    "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'"
                ).fetchall(),
            )
            for (table,) in tables:
                column_rows = t.cast(
                    list[tuple[int, str, str, int, object, int]],
                    connection.execute(
                        f"PRAGMA table_info({quoted_identifier(table)})"
                    ).fetchall(),
                )
                columns = [row[1] for row in column_rows]
                rows = t.cast(
                    list[tuple[object, ...]],
                    connection.execute(
                        f"SELECT rowid,* FROM {quoted_identifier(table)}"
                    ).fetchall(),
                )
                for row in rows:
                    changed: dict[str, str] = {}
                    for column, value in zip(columns, row[1:], strict=True):
                        if isinstance(value, str):
                            replacement = UUID.sub(
                                lambda match: identities[match[0]], value
                            )
                            if (
                                table in {"board_projects", "project_boards"}
                                and column == "name"
                            ):
                                replacement = f"{namespace} {replacement}"
                            if replacement != value:
                                changed[column] = replacement
                    if changed:
                        assignments = ",".join(
                            f"{quoted_identifier(c)}=?" for c in changed
                        )
                        _ = connection.execute(
                            f"UPDATE {quoted_identifier(table)} SET {assignments} WHERE rowid=?",
                            [*changed.values(), row[0]],
                        )
            connection.commit()
            if connection.execute("PRAGMA foreign_key_check").fetchall():
                raise ValueError("synthetic fixture remap broke foreign keys")
    return paths
