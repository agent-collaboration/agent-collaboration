"""Deciding observations for real-central copies, holder transitions and I1."""

import datetime
import json
import time

from .board_central_runtime import RealCentral
from .board_journey_fixture import BoardFixture, accepted_decision
from .board_journey_runtime import BoardClient, project_request, write_header
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
    json_value,
    object_value,
    string_value,
)
from .board_store_readback import read_stopped_store


def directory_entry(central: RealCentral, project: str) -> JsonObject:
    page = central.get("/admin/v1/copies?limit=100")
    entries = [
        object_value(row)
        for row in array_value(page.get("projects"))
        if field(row, "project") == project
    ]
    if len(entries) != 1:
        raise JourneyFailure(
            "owner Directory did not contain exactly the selected project"
        )
    return entries[0]


def work_view(client: BoardClient, project: str) -> JsonObject:
    return client.read("view_work", {"projectId": project, "includeRemoved": False})


def await_view(client: BoardClient, project: str, expected: JsonValue) -> JsonObject:
    deadline = time.monotonic() + 30
    last: JsonObject = {}
    while time.monotonic() < deadline:
        try:
            last = work_view(client, project)
            if last.get("asOf") == expected:
                return last
        except JourneyFailure:
            pass
        time.sleep(0.2)
    raise JourneyFailure(
        f"nonholder copy did not reach the independently observed feed: {last}"
    )


def replication(
    fixture: BoardFixture, reader: BoardClient, central: RealCentral
) -> None:
    fixture.create()
    marker = "G3 replicated immutable record " + fixture_identity()
    thread = fixture.create_thread()
    request = fixture.reply_request(thread, marker)
    reply = fixture.client.accepted("thread_post", request)
    accepted = accepted_decision(reply)
    original = work_view(fixture.client, fixture.project)
    observed = await_view(reader, fixture.project, original["asOf"])
    checks = fixture.receipt
    checks.equal(
        "nonholder uses the actual central read copy",
        "VD OB2 / PD3 A9",
        ({"kind": "readCopy"}, observed.get("source")),
    )
    checks.equal(
        "nonholder work lanes match holder records",
        "VD OB2",
        (original.get("workstreams"), observed.get("workstreams")),
    )
    checks.equal(
        "nonholder asOf equals its acknowledged prefix",
        "VD OB2",
        (observed.get("asOf"), observed.get("replicatedThrough")),
    )
    replay = fixture.client.read("thread_post", request)
    checks.equal(
        "retry retains the first decision",
        "VD OB1 RC-4",
        (accepted, accepted_decision(replay)),
    )
    after = work_view(fixture.client, fixture.project)
    checks.equal(
        "retry adds no project feed effect",
        "VD OB1 RC-4",
        (original.get("asOf"), after.get("asOf")),
    )
    # The enrollment-bound origin must survive transport and copy readback.
    feed = reader.read(
        "feed_read",
        {
            "projectId": fixture.project,
            "reader": {"kind": "anonymous"},
            "after": {"epoch": 1, "position": 0},
            "limit": 100,
            "includeRedacted": False,
        },
    )
    posts = [
        object_value(entry)
        for entry in array_value(field(feed, "value", "entries"))
        if field(entry, "record", "id") == request["postId"]
    ]
    checks.equal(
        "the copied post has exactly one feed entry",
        "VD OB2 / verified origin",
        (1, len(posts)),
    )
    checks.equal(
        "central copy contains the exact immutable post",
        "VD OB2",
        (True, marker in json.dumps(posts[0])),
    )
    checks.equal(
        "the copied post origin is its enrolled writer",
        "PD3 verified origin",
        (
            fixture.runtime.manifest["serviceId"],
            field(posts[0], "origin", "installation"),
        ),
    )
    deadline = time.monotonic() + 30
    while original.get("replicatedThrough") != original.get("asOf"):
        if time.monotonic() >= deadline:
            raise JourneyFailure("holder acknowledgement did not reach its exact feed")
        time.sleep(0.2)
        original = work_view(fixture.client, fixture.project)
    entry = directory_entry(central, fixture.project)
    checks.record({"kind": "directorySummary", "entry": entry, "holderView": original})
    checks.equal(
        "Directory copy summary is reported",
        "PD3 FL-1",
        ("known", field(entry, "copySide", "kind")),
    )
    checks.equal(
        "Directory names the real enrolled holder",
        "PD3 FL-1",
        (
            fixture.runtime.manifest["serviceId"],
            field(entry, "leaseSide", "value", "lease", "value", "holder"),
        ),
    )
    checks.equal(
        "Directory latest record time is the copied post time",
        "PD3 copy summaries",
        (posts[0]["time"], field(entry, "copySide", "value", "latestRecordAt")),
    )
    fixture.runtime.stop()
    snapshot = read_stopped_store(fixture.database_path())
    copy_position = object_value(field(entry, "copySide", "value", "asOf", "position"))
    marks = [
        row
        for row in snapshot.tables["capture_feed_marks"].rows
        if row["epoch"] == copy_position["epoch"]
        and row["captured_position"] == copy_position["sequence"]
    ]
    checks.record(
        {
            "kind": "captureMapping",
            "centralPosition": copy_position,
            "marks": json_value(json.dumps(marks)),
            "boardAsOf": observed["asOf"],
        }
    )
    checks.equal(
        "Directory summary uses an actual captured transaction",
        "VD OB2 / PD2 §3a.2",
        (1, len(marks)),
    )
    checks.equal(
        "Directory summary maps to the exact independently read board prefix",
        "VD OB2 / PD2 §3a.2",
        (field(observed, "asOf", "position"), marks[0]["last_feed_position"]),
    )
    first_identity = dict(fixture.runtime.manifest)
    fixture.runtime.start()
    reopened = work_view(fixture.client, fixture.project)
    checks.equal(
        "owning reopen retains the exact logical work view",
        "VD §2",
        (original, reopened),
    )
    checks.equal(
        "owning reopen preserves installation",
        "VD §2",
        (first_identity["serviceId"], fixture.runtime.manifest["serviceId"]),
    )
    checks.equal(
        "owning reopen changes Host epoch",
        "VD §2",
        (
            False,
            first_identity["serviceEpoch"] == fixture.runtime.manifest["serviceEpoch"],
        ),
    )


def expiry(fixture: BoardFixture, reader: BoardClient, central: RealCentral) -> None:
    before = work_view(fixture.client, fixture.project)
    copied = await_view(reader, fixture.project, before["asOf"])
    original_entry = directory_entry(central, fixture.project)
    fixture.receipt.record(
        {"kind": "i1Before", "directory": original_entry, "view": copied}
    )
    # The serialized Directory shape is inspected before the clock drive. The
    # expiry itself comes from the real committed Registry grant, never a guess.
    lease_side = object_value(original_entry["leaseSide"])
    metadata = object_value(lease_side["value"])
    lease = object_value(field(metadata, "lease", "value"))
    expires = datetime.datetime.fromisoformat(string_value(lease["expires_at"]))
    _ = central.control(
        {"kind": "fleet"},
        {
            "kind": "setTime",
            "value": {
                "now": (expires - datetime.timedelta(milliseconds=1)).isoformat()
            },
        },
    )
    held = directory_entry(central, fixture.project)
    held_view = work_view(reader, fixture.project)
    _ = central.control(
        {"kind": "fleet"}, {"kind": "setTime", "value": {"now": expires.isoformat()}}
    )
    expired = directory_entry(central, fixture.project)
    expired_view = work_view(reader, fixture.project)
    fixture.receipt.record(
        {
            "kind": "i1After",
            "held": held,
            "expired": expired,
            "before": held_view,
            "after": expired_view,
        }
    )
    held_metadata = object_value(field(held, "leaseSide", "value"))
    expired_metadata = object_value(field(expired, "leaseSide", "value"))
    fixture.receipt.equal(
        "Directory displays Held before expiry",
        "1075 / I1",
        ("held", field(held_metadata, "lease", "value", "state")),
    )
    fixture.receipt.equal(
        "Directory displays Expired at expiry",
        "1075 / I1",
        ("expired", field(expired_metadata, "lease", "value", "state")),
    )
    fixture.receipt.equal(
        "expiry changes no committed lease version or asOf",
        "1075 / I1",
        (
            [held_metadata["leaseVersion"], held_metadata["asOf"]],
            [expired_metadata["leaseVersion"], expired_metadata["asOf"]],
        ),
    )
    fixture.receipt.equal(
        "nonholder observed the current unchanged lease",
        "1075 / I1",
        (
            {
                "leaseVersion": expired_metadata["leaseVersion"],
                "asOf": expired_metadata["asOf"],
            },
            expired_view["writerObservation"],
        ),
    )
    fixture.receipt.equal(
        "nonholder view survives derived Directory expiry",
        "1075 / I1",
        (held_view, expired_view),
    )


def owner_transfer(
    central: RealCentral, project: str, operation: str, body: JsonObject
) -> JsonObject:
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        reply = central.post(f"/admin/v1/projects/{project}/{operation}", body)
        if reply.get("kind") != "pending":
            return reply
        time.sleep(0.2)
    raise JourneyFailure("owner transfer never settled its stable transfer identity")


def await_writer(client: BoardClient, project: str, epoch: int) -> JsonObject:
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            view = work_view(client, project)
            if view.get("source") == {"kind": "writer"} and view.get(
                "currentWriter"
            ) == {"kind": "held", "epoch": epoch}:
                return view
        except JourneyFailure:
            pass
        time.sleep(0.2)
    raise JourneyFailure(f"target Host did not become native writer at epoch {epoch}")


def holder_change(
    fixture: BoardFixture, receiver: BoardFixture, central: RealCentral
) -> None:
    project = fixture.project
    checks = fixture.receipt
    expected = work_view(fixture.client, project)
    moved = owner_transfer(
        central,
        project,
        "move",
        {
            "transferId": fixture_identity(),
            "toInstallation": receiver.runtime.manifest["serviceId"],
            "reason": "G3 actual enrolled Host move",
        },
    )
    checks.record({"kind": "ownerMove", "reply": moved})
    writing = await_writer(receiver.client, project, 2)
    checks.equal(
        "move installs the actual native successor before use",
        "VD OB3",
        (expected["workstreams"], writing["workstreams"]),
    )
    refused = fixture.client.read(
        "project_describe",
        project_request(
            fixture.owner,
            project,
            {
                "kind": "projectDescribe",
                "description": "forbidden stale writer mutation",
                "expectedRevision": 0,
            },
        ),
    )
    checks.equal(
        "superseded Host refuses its guarded non-post write",
        "VD OB3 / WA-2",
        ("notWriter", field(refused, "result", "kind")),
    )
    checks.record({"kind": "moveDirectory", "entry": directory_entry(central, project)})
    preview = central.post(f"/admin/v1/projects/{project}/takeover/preview", None)
    takeover = owner_transfer(
        central,
        project,
        "takeover",
        {
            "transferId": fixture_identity(),
            "toInstallation": fixture.runtime.manifest["serviceId"],
            "reason": "G3 actual enrolled Host takeover",
            "preview": preview["preview"],
        },
    )
    checks.record({"kind": "ownerTakeover", "reply": takeover})
    restored = await_writer(fixture.client, project, 3)
    checks.equal(
        "takeover preserves the acknowledged board lanes",
        "VD OB3",
        (expected["workstreams"], restored["workstreams"]),
    )
    refused = receiver.client.read(
        "project_describe",
        project_request(
            fixture.owner,
            project,
            {
                "kind": "projectDescribe",
                "description": "forbidden prior holder mutation",
                "expectedRevision": 0,
            },
        ),
    )
    checks.equal(
        "takeover leaves the old holder unable to write",
        "VD OB3",
        ("notWriter", field(refused, "result", "kind")),
    )


def archive_and_notices(fixture: BoardFixture, reader: BoardFixture) -> None:
    checks = fixture.receipt
    progress_file = (
        reader.runtime.directory / "machine-session/machine-notice-progress.json"
    )
    before = object_value(json_value(progress_file.read_bytes()))
    board = fixture_identity()
    _ = fixture.client.accepted(
        "board_create",
        {
            **write_header(fixture.owner),
            "workspaceId": fixture.workspace,
            "boardId": board,
            "name": "Empty G3 archive board",
            "description": "Owned archive observation",
            "kind": {"kind": "area"},
        },
    )
    _ = fixture.client.accepted(
        "board_archive", {**write_header(fixture.owner), "boardId": board}
    )
    deadline = time.monotonic() + 30
    observed: JsonObject = {}
    while time.monotonic() < deadline:
        try:
            observed = reader.client.read("board_show", {"boardId": board})
            if field(observed, "board", "state") == "archived":
                break
        except JourneyFailure:
            pass
        time.sleep(0.2)
    checks.equal(
        "nonholder sees the finalized empty-board archive",
        "G3 / PD3 FL-5",
        ("archived", field(observed, "board", "state")),
    )
    after = object_value(json_value(progress_file.read_bytes()))
    checks.record(
        {
            "kind": "machineNoticeProgress",
            "before": before,
            "after": after,
            "root": str(reader.runtime.directory),
        }
    )
    checks.equal(
        "native notices advanced their owned durable cursor",
        "G3 / PD3 FL-6",
        (True, int(str(after["handledThrough"])) > int(str(before["handledThrough"]))),
    )


def verified_forward(fixture: BoardFixture, sender: BoardFixture) -> None:
    """A nonholder post must reach the holder with the enrolled sender origin."""
    fixture.create()
    thread = fixture.create_thread()
    marker = "G3 verified forwarded origin " + fixture_identity()
    request = fixture.reply_request(thread, marker)
    reply = sender.client.read("thread_post", request)
    fixture.receipt.record({"kind": "forwardedPostReply", "reply": reply})
    _ = accepted_decision(reply)
    feed = fixture.feed()
    entries = [
        object_value(entry)
        for entry in array_value(field(feed, "value", "entries"))
        if field(entry, "record", "id") == request["postId"]
    ]
    fixture.receipt.equal(
        "one forwarded post reaches the holder", "VD OB2/OB4 WA-2", (1, len(entries))
    )
    fixture.receipt.equal(
        "central verifies the actual enrolled sending installation",
        "PD3 verified-origin forwarding",
        (
            sender.runtime.manifest["serviceId"],
            field(entries[0], "origin", "installation"),
        ),
    )
    fixture.receipt.equal(
        "forwarded content is exact",
        "PD3 verified-origin forwarding",
        (True, marker in json.dumps(entries[0])),
    )
