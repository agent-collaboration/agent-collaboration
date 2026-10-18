"""Cold-start the real Host over an owned legacy copy, never an account database."""

import contextlib
import datetime
import json
import shutil
import sqlite3
import typing as t

from .board_journey_runtime import BoardClient, BoardRuntime
from .board_journey_values import (
    JsonObject,
    JourneyFailure,
    field,
    fixture_identity,
    object_value,
    json_value,
    string_value,
)


def utc_millis() -> int:
    now = datetime.datetime.now(datetime.timezone.utc)
    delta = now - datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc)
    return delta.days * 86400000 + delta.seconds * 1000 + delta.microseconds // 1000


def parsed_millis(value: str) -> int:
    timestamp = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    delta = timestamp - datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc)
    return delta.days * 86400000 + delta.seconds * 1000 + delta.microseconds // 1000


def legacy_copy(source: BoardRuntime, destination: BoardRuntime) -> dict[str, str]:
    if source.child is not None or destination.child is not None:
        raise JourneyFailure("legacy copy requires both owned runtimes stopped")
    _ = shutil.copytree(source.directory, destination.directory, dirs_exist_ok=True)
    path = destination.directory / "automation.sqlite"
    service = source.manifest.get("serviceId")
    ids = {name: fixture_identity() for name in ["older", "newest", "fast", "far"]}
    now = utc_millis()
    with contextlib.closing(sqlite3.connect(path)) as connection:
        if connection.execute("SELECT count(*) FROM mail_items").fetchone() != (0,):
            raise JourneyFailure(
                "legacy fixture must come from the fresh owned debug store"
            )
        if connection.execute("SELECT count(*) FROM wakeup_definitions").fetchone() != (
            0,
        ):
            raise JourneyFailure(
                "legacy fixture unexpectedly contains current-format wakes"
            )
        with connection:
            # Model the historical schema only in the stopped private copy.
            # Native migration checksums 1..8 stay those produced by the real Host.
            _ = connection.execute("DROP INDEX router_pushes_mail_retention")
            _ = connection.execute("DROP TABLE mail_items")
            _ = connection.execute("ALTER TABLE router_pushes DROP COLUMN retention")
            _ = connection.execute("DROP INDEX wakeup_occupying_target")
            _ = connection.execute(
                "DELETE FROM _sqlx_migrations WHERE version>=20261016000100"
            )
            for name in ids:
                target_name = (
                    "m2c-upgrade-slot"
                    if name in ["older", "newest"]
                    else f"m2c-upgrade-{name}"
                )
                timing: JsonObject = {"kind": "after", "seconds": 600}
                expiry: int | None = None
                state = "paused" if name == "older" else "active"
                created = now - (2000 if name == "older" else 1000)
                if name == "newest":
                    timing = {"kind": "interval", "seconds": 300}
                    expiry = now + 30 * 86400000
                elif name == "fast":
                    timing = {
                        "kind": "cron",
                        "expression": "* * * * *",
                        "timezone": "UTC",
                    }
                    expiry = now + 7200000
                elif name == "far":
                    timing = {"kind": "after", "seconds": 259200}
                definition: JsonObject = {
                    "wakeupId": ids[name],
                    "changeId": fixture_identity(),
                    "message": {
                        "target": {
                            "endpoint": {
                                "serviceId": service,
                                "endpointId": "codex-local",
                            },
                            "sessionId": target_name,
                        },
                        "content": {
                            "kind": "humanUser",
                            "text": "M2c migration fixture only",
                        },
                        "delivery": "auto",
                    },
                    "timing": timing,
                    "anchorAtMs": created,
                    "expiresAtMs": expiry,
                    "createdAtMs": created,
                }
                due = now + (259200000 if name == "far" else 300000)
                _ = connection.execute(
                    "INSERT INTO wakeup_definitions(wakeup_id,change_id,wakeup_status,definition_json,anchor_at_ms,evaluated_through_ms,next_due_at_ms,expires_at_ms,first_fire_json,pending_delivery_id,created_at_ms,updated_at_ms) VALUES (?,?,?,?,?,?,?,?,NULL,NULL,?,?)",
                    (
                        ids[name],
                        definition["changeId"],
                        state,
                        json.dumps(definition),
                        created,
                        created,
                        due,
                        expiry,
                        created,
                        created,
                    ),
                )
    identities: JsonObject = {name: value for name, value in ids.items()}
    source.receipt.record(
        {
            "kind": "fixture",
            "scope": "stopped copy of fresh real debug automation store",
            "source": str(source.directory / "automation.sqlite"),
            "copy": str(path),
            "legacyVersions": "native migration history 1..8",
            "controlledChange": "reverse only the two unreleased M2c migrations in the copy and insert typed legacy wake definitions",
            "ids": identities,
        }
    )
    return ids


def observe_migration(source: BoardRuntime) -> None:
    receipt = source.receipt
    destination = BoardRuntime(source.target, receipt)
    ids = legacy_copy(source, destination)
    before = utc_millis()
    try:
        destination.start()
        after = utc_millis()
        client = BoardClient(destination)
        snapshots: dict[str, JsonObject] = {}
        for name, identity in ids.items():
            snapshot = client.read("wake_show", {"wakeupId": identity})
            snapshots[name] = snapshot
            receipt.equal(
                f"upgraded {name} creator is Unrecorded",
                "MS-10 upgrade",
                ({"kind": "unrecorded"}, field(snapshot, "definition", "createdBy")),
            )
        receipt.equal(
            "newest occupying wake is kept",
            "MS-10 upgrade",
            ("active", snapshots["newest"].get("state")),
        )
        receipt.equal(
            "older occupying wake is expired",
            "MS-10 upgrade",
            ("expired", snapshots["older"].get("state")),
        )
        receipt.equal(
            "one-minute Cron is expired",
            "MS-10 upgrade",
            ("expired", snapshots["fast"].get("state")),
        )
        for name, lifetime in [("newest", 604800000), ("far", 86400000)]:
            end = parsed_millis(
                string_value(field(snapshots[name], "definition", "expiresAt"))
            )
            receipt.equal(
                f"{name} expiry is capped from the actual release",
                "MS-10 upgrade",
                (True, before + lifetime <= end <= after + lifetime),
            )
        receipt.equal(
            "far one-shot has no due time outside its cap",
            "MS-10 upgrade",
            (None, snapshots["far"].get("nextDueAt")),
        )
        with contextlib.closing(
            sqlite3.connect(destination.directory / "automation.sqlite")
        ) as connection:
            for name, event in [
                ("older", "upgradeDuplicate"),
                ("fast", "upgradeLimit"),
            ]:
                row = t.cast(
                    tuple[str, str] | None,
                    connection.execute(
                        "SELECT event_kind,event_body_json FROM automation_events WHERE subject_id=? AND event_kind=?",
                        (ids[name], event),
                    ).fetchone(),
                )
                if row is None:
                    raise JourneyFailure(f"missing migration event: {event}")
                receipt.equal(
                    f"{event} event is recorded", "MS-10 upgrade", (event, row[0])
                )
                body = object_value(json_value(row[1]))
                receipt.equal(
                    f"{event} event records expiry",
                    "MS-10 upgrade",
                    ("expired", body.get("after")),
                )
            rows = t.cast(
                list[tuple[int]],
                connection.execute(
                    "SELECT version FROM _sqlx_migrations ORDER BY version"
                ).fetchall(),
            )
            receipt.equal(
                "real Host commits all ten native migrations",
                "schema migration",
                (10, len(rows)),
            )
            receipt.equal(
                "Local custody migration is installed",
                "schema migration",
                (20261016000200, rows[-1][0]),
            )
        destination.stop()
        destination.start()
        repeated = BoardClient(destination).read(
            "wake_show", {"wakeupId": ids["newest"]}
        )
        receipt.equal(
            "reopen does not re-anchor the release cap",
            "MS-10 upgrade",
            (field(snapshots["newest"], "definition"), field(repeated, "definition")),
        )
    finally:
        destination.stop()
        destination.target.require_unchanged_binaries()
