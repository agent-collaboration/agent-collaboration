"""OB5 external observations using the existing legacy and publication oracles."""

import contextlib
import json
import sqlite3
import subprocess
import time
import typing as t
from pathlib import Path

from .board_journey_fixture import BoardFixture
from .board_journey_runtime import BoardClient, BoardRuntime, project_request
from .board_journey_values import field, file_digest, fixture_identity, object_value
from .board_store_readback import read_stopped_store
from .board_upgrade_external_transport import MachineReadback, OwnerEndpoint
from .board_upgrade_native_oracle import Checks, native_tables, verify_native
from .board_upgrade_publication_views import UNKNOWN_SUMMARY
from .board_upgrade_source import source_readers, source_rows
from .board_upgrade_views import (
    DECLARED_OWNER,
    DECLARED_PROVENANCE,
    NativeObservation,
    NativePage,
    Object,
    UpgradeReport,
    json_object,
    text_field,
)


def owner_state(owner: OwnerEndpoint, report: UpgradeReport) -> Object:
    state: Object = {}
    needed = {project.project for project in report.projects}
    directory: dict[str, Object] = {}
    after: str | None = None
    for _ in range(2048):
        page = owner.call(
            "/admin/v1/copies?limit=100" + ("&after=" + after if after else ""),
            method="GET",
        )
        entries = page["projects"]
        if not isinstance(entries, list):
            raise TypeError("owner directory is not an array")
        for entry in entries:
            row = json_object.validate_python(entry)
            project = text_field(row, "project")
            if project in needed:
                directory[project] = row
        if set(directory) == needed or page["nextAfter"] is None:
            break
        next_after = text_field(page, "nextAfter")
        if next_after == after:
            raise ValueError("owner directory cursor stalled")
        after = next_after
    for project in report.projects:
        status = owner.call(
            f"/admin/v1/import/projects/{project.project}",
            {"kind": "status", "value": {"digest": project.digest}},
        )
        health = owner.call(f"/admin/v1/copies/{project.project}/health", method="GET")
        state[project.project] = {
            "import": status,
            "health": health,
            "directory": directory.get(project.project),
        }
    return state


def published_owner_state(owner: OwnerEndpoint, report: UpgradeReport) -> Object:
    deadline = time.monotonic() + 30
    while True:
        state = owner_state(owner, report)
        ready = all(
            json_object.validate_python(
                json_object.validate_python(
                    json_object.validate_python(row)["directory"]
                )["copySide"]
            ).get("kind")
            == "known"
            for row in state.values()
        )
        if ready or time.monotonic() >= deadline:
            return state
        time.sleep(0.1)


def verify_owner(checks: Checks, report: UpgradeReport, state: Object) -> None:
    for project in report.projects:
        stored = json_object.validate_python(state[project.project])
        progress = json_object.validate_python(stored["import"])
        checks.equal("owner import progress", "progress", progress["kind"])
        record = json_object.validate_python(progress["value"])
        header = json_object.validate_python(record["header"])
        checks.equal(
            "owner semantic digest",
            project.digest,
            json_object.validate_python(header)["digest"],
        )
        checks.equal(
            "owner workflow completed",
            {"kind": "complete", "position": project.manifest["position"]},
            record["phase"],
        )
        health = json_object.validate_python(stored["health"])
        checks.equal("owner Copy is live", "live", health["kind"])
        live = json_object.validate_python(health["value"])
        checks.equal(
            "owner Copy is Following",
            {"kind": "known", "value": "following"},
            live["stage"],
        )
        copy = json_object.validate_python(
            json_object.validate_python(live["copy"])["value"]
        )
        checks.equal("imported Copy remains unheld", None, copy["holder"])
        checks.equal("imported Copy fence", 1, copy["fence"])
        checks.equal(
            "Copy acknowledged the imported manifest",
            project.manifest["position"],
            copy["acked"],
        )
        checks.equal(
            "imported header time remains unknown", UNKNOWN_SUMMARY, header["summary"]
        )
        directory = json_object.validate_python(stored["directory"])
        side = json_object.validate_python(directory["copySide"])
        checks.equal(
            "owner directory received actual Copy summary", "known", side["kind"]
        )
        summary = json_object.validate_python(side["value"])
        checks.equal(
            "actual Copy summary time is unknown",
            {"kind": "legacyUnknown"},
            summary["latestRecordAt"],
        )
        checks.equal(
            "actual Copy summary covers imported manifest",
            project.manifest["position"],
            json_object.validate_python(summary["asOf"])["position"],
        )


def native_reference(
    repository: Path,
    binary: Path,
    source: Path,
    case: Path,
    *,
    digest: str,
) -> NativeObservation:
    if file_digest(binary) != digest:
        raise ValueError("frozen native observer bytes changed before use")
    output = case / "native-reference.json"
    request = case / "native-request.json"
    _ = request.write_text(
        json.dumps(
            {
                "source": str(source),
                "output": str(output),
                "readers": source_readers(source_rows(source)),
                "owner": DECLARED_OWNER,
                "provenance": DECLARED_PROVENANCE,
            }
        )
    )
    result = subprocess.run(
        [str(binary), str(request)],
        cwd=repository,
        capture_output=True,
        timeout=180,
        check=False,
    )
    _ = (case / "native.stdout").write_bytes(result.stdout)
    _ = (case / "native.stderr").write_bytes(result.stderr)
    if result.returncode:
        raise ValueError("native reference observer failed")
    return NativeObservation.model_validate_json(output.read_bytes())


def observe_snapshots(
    machine: MachineReadback,
    source: Path,
    expected: NativeObservation,
    checks: Checks,
    case: Path,
) -> None:
    # Product-native preparation's rows are checked against the independent
    # legacy derivation first. Remote snapshots must then retain every row.
    verify_native(source, expected, checks)
    for project in expected.projects:
        pages = machine.pages(project.project)
        _ = (case / f"{project.project}-remote-snapshot.json").write_text(
            json.dumps(pages, indent=2)
        )
        actual = [NativePage.model_validate(page) for page in pages]
        checks.equal(
            "real central snapshot preserves every prepared table/row",
            native_tables(project.pages),
            native_tables(actual),
        )


def fleet_records(
    machine: MachineReadback,
    report: UpgradeReport,
    checks: Checks,
    case: Path,
    *,
    after: int,
) -> Object:
    expected_ids = (
        {text_field(row, "workspace") for row in report.workspace_counts}
        | {text_field(row, "board") for row in report.board_counts}
        | {p.project for p in report.projects}
    )
    result: Object = {}
    for snapshot in [False, True]:
        records = machine.fleet(snapshot, after=after)
        selected = [
            row
            for row in records
            if text_field(
                json_object.validate_python(
                    json_object.validate_python(row["record"])["envelope"]
                ),
                "id",
            )
            in expected_ids
        ]
        checks.equal(
            "fleet read covers every imported identity",
            len(report.fleet.records),
            len(selected),
        )
        for row in selected:
            envelope = json_object.validate_python(
                json_object.validate_python(row["record"])["envelope"]
            )
            checks.equal(
                "fleet import time remains unknown",
                {"kind": "legacyUnknown"},
                envelope["time"],
            )
        result["snapshot" if snapshot else "feed"] = [record for record in selected]
    _ = (case / "fleet-reads.json").write_text(json.dumps(result, indent=2))
    return result


def imported_feed(client: BoardClient, project: str) -> Object:
    cursor: Object = {"epoch": 1, "position": 0}
    entries: list[Object] = []
    for _ in range(2048):
        answer = client.read(
            "feed_read",
            {
                "projectId": project,
                "reader": {"kind": "anonymous"},
                "after": cursor,
                "limit": 100,
                "includeRedacted": False,
            },
        )
        page = json_object.validate_python(answer["value"])
        if page.get("kind") != "entries":
            raise ValueError("imported prefix is not publicly readable")
        records = page["entries"]
        if not isinstance(records, list):
            raise TypeError("feed entries missing")
        entries.extend(json_object.validate_python(entry) for entry in records)
        following = json_object.validate_python(page["coveredThrough"])
        if following == page["highWater"]:
            return json_object.validate_python(
                {
                    "value": {
                        "kind": "entries",
                        "entries": entries,
                        "coveredThrough": following,
                        "highWater": page["highWater"],
                    }
                }
            )
        if following == cursor:
            raise ValueError("public project feed cursor stalled")
        cursor = following
    raise ValueError("public project feed exceeds observation bound")


def claim_and_write(
    runtime: BoardRuntime,
    report: UpgradeReport,
    checks: Checks,
    case: Path,
    *,
    owner: OwnerEndpoint,
) -> str:
    runtime.start()
    client = BoardClient(runtime)
    project = report.projects[0].project
    # Listener readiness precedes machine welcome. Wait for this fresh Host's
    # public fleet cache to contain the actual imported directory entry before
    # calling Claim; do not turn a Claim error into a successful retry.
    deadline = time.monotonic() + 60
    while True:
        directory = client.read(
            "project_list",
            {"boardId": report.projects[0].board, "page": {"limit": 100}},
        )
        records = field(directory, "page", "records")
        if not isinstance(records, list):
            raise TypeError("Host project directory records missing")
        if any(field(row, "id") == project for row in records):
            break
        if time.monotonic() >= deadline:
            raise TimeoutError("fresh Host did not receive imported fleet directory")
        time.sleep(0.1)
    _ = (case / "host-directory-before-claim.json").write_text(
        json.dumps(directory, indent=2)
    )
    request: Object = {
        "requestId": fixture_identity(),
        "protocolVersion": 1,
        "projectId": project,
    }
    deadline = time.monotonic() + 60
    while True:
        reply = client.read("project_claim", request)
        if reply.get("kind") != "pending":
            break
        if time.monotonic() >= deadline:
            raise TimeoutError("actual Host Claim did not settle")
        time.sleep(0.1)
    checks.equal("real Claim grants next epoch", "claimed", reply["kind"])
    claim = json_object.validate_python(reply["value"])
    checks.equal("Claim epoch is two", 2, claim["epoch"])
    checks.equal(
        "Claim installs acknowledged prefix",
        report.projects[0].manifest["position"],
        claim["start"],
    )
    actor: Object = {"kind": "human", "humanId": DECLARED_OWNER}
    # Read the imported Project record through the real public feed before
    # choosing the guarded revision for the first write.
    feed = imported_feed(client, project)
    _ = (case / "claimed-feed.json").write_text(json.dumps(feed, indent=2))
    entries = field(feed, "value", "entries")
    if not isinstance(entries, list):
        raise TypeError("claimed feed entries missing")
    states = [
        object_value(field(entry, "content", "record", "record"))
        for entry in entries
        if field(entry, "record", "kind") == "project"
    ]
    if not states:
        raise ValueError("Claim did not install imported ProjectState")
    state = states[-1]
    for entry in entries:
        checks.equal(
            "installed imported feed time unknown",
            {"kind": "legacyUnknown"},
            field(entry, "time"),
        )
    _ = client.accepted(
        "project_describe",
        project_request(
            actor,
            project,
            {
                "kind": "projectDescribe",
                "description": "First accepted G4 imported-holder write",
                "expectedRevision": state["revision"],
            },
        ),
    )
    deadline = time.monotonic() + 60
    while True:
        view = client.read("view_work", {"projectId": project, "includeRemoved": False})
        if view.get("asOf") == view.get("replicatedThrough"):
            break
        if time.monotonic() >= deadline:
            raise TimeoutError(
                "first accepted write not acknowledged by actual central"
            )
        time.sleep(0.1)
    checks.equal(
        "first live write belongs to epoch two", 2, field(view, "asOf", "epoch")
    )
    _ = (case / "first-write.json").write_text(json.dumps(view, indent=2))
    runtime.stop()
    fixture = BoardFixture(runtime)
    fixture.project = project
    # The existing stopped-store reader checks consumers and exact schema; the
    # Host owns this cache, and a fresh Host reopen is observed below.
    from_path = fixture.database_path()
    checks.equal("Claim produced an owning project file", True, from_path.is_file())
    stored = read_stopped_store(from_path)
    _ = (case / "claimed-owner-project.json").write_text(
        stored.model_dump_json(indent=2)
    )
    checks.equal(
        "owning project retains the first accepted description",
        "First accepted G4 imported-holder write",
        stored.tables["project_state"].rows[0]["description"],
    )
    original_rows = [
        row for row in stored.tables["project_feed"].rows if row["epoch"] == 1
    ]
    checks.equal(
        "Claim retains every imported board feed row",
        report.projects[0].feed_rows,
        len(original_rows),
    )
    checks.equal(
        "owning imported feed keeps unknown time",
        True,
        all(
            row["time_basis"] == "legacy_unknown" and row["router_time"] is None
            for row in original_rows
        ),
    )
    acknowledged = owner.call(f"/admin/v1/copies/{project}/health", method="GET")
    server_copy = json_object.validate_python(
        json_object.validate_python(
            json_object.validate_python(acknowledged["value"])["copy"]
        )["value"]
    )
    position = json_object.validate_python(server_copy["acked"])
    marks = [
        row
        for row in stored.tables["capture_feed_marks"].rows
        if row["epoch"] == position["epoch"]
        and row["captured_position"] == position["sequence"]
    ]
    checks.equal("central acknowledged the actual first-write capture", 1, len(marks))
    checks.equal(
        "central acknowledgement maps to the exact board feed head",
        [field(view, "asOf", "epoch"), field(view, "asOf", "position")],
        [marks[0]["epoch"], marks[0]["last_feed_position"]],
    )
    _ = (case / "first-write-central-ack.json").write_text(
        json.dumps(
            {
                "health": acknowledged,
                "captureMark": marks[0],
                "boardAsOf": view["asOf"],
            },
            indent=2,
        )
    )
    cache = runtime.directory / "fleet-cache.sqlite"
    # The existing native Host is stopped; use the same exact consumer guard as
    # its project reader before reading only this synthetic import's cache rows.
    consumers = subprocess.run(
        ["lsof", "-t", "--", str(cache)], capture_output=True, check=False
    )
    checks.equal("Router fleet cache has no live consumers", 1, consumers.returncode)
    cached: list[Object] = []
    with contextlib.closing(
        sqlite3.connect(cache.as_uri() + "?mode=ro", uri=True)
    ) as connection:
        for record in report.fleet.records:
            key = text_field(
                record,
                "project" if record["kind"] == "directory" else "id",
            )
            rows = t.cast(
                list[tuple[str]],
                connection.execute(
                    "SELECT payload FROM cached_fleet_records WHERE id=?", (key,)
                ).fetchall(),
            )
            checks.equal("Router cache retains imported identity", 1, len(rows))
            value = json_object.validate_json(str(rows[0][0]))
            checks.equal(
                "Router cached imported time unknown",
                {"kind": "legacyUnknown"},
                field(value, "record", "envelope", "time"),
            )
            cached.append(value)
    _ = (case / "router-fleet-cache.json").write_text(json.dumps(cached, indent=2))
    runtime.start()
    reopened = BoardClient(runtime).read(
        "view_work", {"projectId": project, "includeRemoved": False}
    )
    checks.equal("owning Host reopen preserves first write", view, reopened)
    runtime.stop()
    return project
