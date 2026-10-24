"""Observe Claim/Move/returning Takeover and public nonholder post forwarding.

The only arranged data is a private copy of the checked-in synthetic legacy
store. The real importer publishes it as unheld; all holder and board operations
then use the delivered owner HTTP, MCP or CLI entry points.
"""

import contextlib
import datetime
import json
import os
import shutil
import sqlite3
import subprocess
import time
import typing as t

from .board_central_cases import (
    await_view,
    await_writer,
    directory_entry,
    owner_transfer,
    work_view,
)
from .board_central_runtime import RealCentral
from .board_journey_fixture import BoardFixture, accepted_decision
from .board_journey_runtime import BoardClient, project_request
from .board_journey_subjects import create_subject_request
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    file_digest,
    fixture_identity,
    json_value,
    object_value,
    string_value,
)


def publish_unheld(fixture: BoardFixture, central: RealCentral) -> str:
    """Publish two empty projects using an immutable private synthetic store."""
    root = fixture.receipt.directory
    source = root / "synthetic-legacy.sqlite"
    repository = central.plan.repository
    template = (
        repository / "crates/board-legacy-upgrade/tests/fixtures/legacy-complete.sqlite"
    )
    _ = shutil.copyfile(template, source)
    unheld = fixture_identity()
    with contextlib.closing(sqlite3.connect(source)) as connection, connection:
        board = t.cast(
            tuple[str] | None,
            connection.execute("SELECT board_id FROM board_topics LIMIT 1").fetchone(),
        )
        if board is None:
            raise JourneyFailure("synthetic fixture has no board")
        for project in [fixture.project, unheld]:
            _ = connection.execute(
                "INSERT INTO board_topics(topic_id,board_id,name,description) VALUES(?,?,?,?)",
                (
                    project,
                    board[0],
                    "Owned unheld " + project,
                    "Synthetic joint observation",
                ),
            )
    source.chmod(0o444)
    before = file_digest(source)
    temporary = root / "import-temporary"
    temporary.mkdir(mode=0o700)
    report = root / "import-report.json"
    arguments = [
        str(repository / "target/debug/codex-router-board-upgrade"),
        "--source",
        str(source),
        "--router-directory",
        str(fixture.runtime.directory),
        "--report",
        str(report),
        "--owner-human-id",
        string_value(fixture.owner["humanId"]),
        "--machine-label",
        "verification source",
        "--central-url",
        central.ready.url,
    ]
    environment = dict(os.environ, TMPDIR=str(temporary))
    environment["CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE"] = str(
        central.assertion
    )
    with (
        (root / "import.stdout").open("wb") as stdout,
        (root / "import.stderr").open("wb") as stderr,
    ):
        result = subprocess.run(
            arguments,
            cwd=repository,
            env=environment,
            stdout=stdout,
            stderr=stderr,
            timeout=120,
            check=False,
        )
    fixture.receipt.record(
        {
            "kind": "importCommand",
            "arguments": list[JsonValue](arguments),
            "exit": result.returncode,
            "sourceSha256": before,
        }
    )
    fixture.receipt.equal(
        "real importer publishes the synthetic unheld projects",
        "FL-15 setup",
        (0, result.returncode),
    )
    fixture.receipt.equal(
        "import leaves sealed source unchanged",
        "FL-15 setup",
        (before, file_digest(source)),
    )
    fixture.receipt.equal(
        "real importer reports uploaded",
        "FL-15 setup",
        ("uploaded", field(json_value(report.read_bytes()), "kind")),
    )
    fixture.receipt.record(
        {
            "kind": "syntheticProjects",
            "claimProject": fixture.project,
            "unheldProject": unheld,
            "templateSha256": file_digest(template),
            "report": str(report),
        }
    )
    return unheld


def claim_imported_project(fixture: BoardFixture) -> None:
    request: JsonObject = {
        "requestId": fixture_identity(),
        "protocolVersion": 1,
        "projectId": fixture.project,
    }
    deadline = time.monotonic() + 30
    while True:
        answer = fixture.client.read("project_claim", request)
        if answer.get("kind") != "pending":
            break
        if time.monotonic() >= deadline:
            raise JourneyFailure("actual Claim did not settle its stable request")
        time.sleep(0.2)
    fixture.receipt.record({"kind": "publicClaim", "reply": answer})
    fixture.receipt.equal(
        "Claim returns the imported project's next epoch",
        "FL-15/16",
        (["claimed", 2], [answer.get("kind"), field(answer, "value", "epoch")]),
    )
    _ = await_writer(fixture.client, fixture.project, 2)
    _ = fixture.client.accepted(
        "role_grant",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "roleGrant",
                "subject": {"kind": "project", "subject": fixture.project},
                "role": "lead",
                "holder": fixture.owner,
                "model": None,
                "lineage": None,
                "expectedRevision": 0,
                "ownerGrant": None,
            },
        ),
    )


def project_feed(client: BoardClient, project: str, epoch: int) -> JsonObject:
    return client.read(
        "feed_read",
        {
            "projectId": project,
            "reader": {"kind": "anonymous"},
            "after": {"epoch": epoch, "position": 0},
            "limit": 100,
            "includeRedacted": False,
        },
    )


def write_lane(fixture: BoardFixture, reader: BoardFixture, epoch: int) -> JsonObject:
    lane = fixture_identity()
    marker = f"actual writer epoch {epoch}: {lane}"
    _ = fixture.client.accepted(
        "workstream_create",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "workstreamCreate",
                "workstreamId": lane,
                "name": marker,
                "goal": "independently observe new-epoch admission and replication",
            },
        ),
    )
    fixture.workstream = lane
    fixture.wait_replicated()
    current = work_view(fixture.client, fixture.project)
    copied = await_view(reader.client, fixture.project, current["asOf"])
    fixture.receipt.equal(
        f"epoch {epoch} new write reaches the real read copy",
        "1087 / OB3",
        (True, marker in json.dumps(copied)),
    )
    fixture.receipt.equal(
        f"epoch {epoch} write is in the current feed",
        "1087 / OB3",
        (epoch, field(current, "asOf", "epoch")),
    )
    return current


def task_refusal(fixture: BoardFixture, sender: BoardFixture) -> None:
    before = work_view(fixture.client, fixture.project)
    reply = sender.client.read(
        "task_create",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "taskCreate",
                "taskId": fixture_identity(),
                "workstreamId": fixture.workstream,
                "title": "must not forward this task",
                "body": "Owned negative observation",
                "blockedBy": [],
                "relatedTo": [],
            },
        ),
    )
    fixture.receipt.record({"kind": "nonPostRefusal", "reply": reply})
    fixture.receipt.equal(
        "nonholder refuses a task without forwarding it",
        "WA-13 / accepted non-post limit",
        (True, field(reply, "result", "kind") in {"scopeUnavailable", "notWriter"}),
    )
    fixture.receipt.equal(
        "non-post refusal adds no holder feed entry",
        "WA-13",
        (before["asOf"], work_view(fixture.client, fixture.project)["asOf"]),
    )


def forwarded_posts(fixture: BoardFixture, sender: BoardFixture) -> None:
    root = create_subject_request(
        fixture,
        fixture.owner,
        {"kind": "project", "subject": {"line": "forwarded root"}},
    )
    created = sender.client.read("thread_create", root)
    _ = accepted_decision(created)
    reply_request = fixture.reply_request(
        string_value(root["postId"]), "actual forwarded reply " + fixture_identity()
    )
    posted = sender.client.read("thread_post", reply_request)
    _ = accepted_decision(posted)
    before = work_view(fixture.client, fixture.project)
    for tool, request, original in [
        ("thread_create", root, created),
        ("thread_post", reply_request, posted),
    ]:
        replay = sender.client.read(tool, request)
        fixture.receipt.equal(
            f"{tool} retry returns the holder's exact first decision",
            "WA-13 / RC-4",
            (original["result"], replay["result"]),
        )
    fixture.receipt.equal(
        "both same-ID retries append no second post",
        "WA-13 / RC-4",
        (before["asOf"], work_view(fixture.client, fixture.project)["asOf"]),
    )
    entries = array_value(
        field(project_feed(fixture.client, fixture.project, 4), "value", "entries")
    )
    for request in [root, reply_request]:
        matching = [
            object_value(entry)
            for entry in entries
            if field(entry, "record") == {"kind": "post", "id": request["postId"]}
        ]
        fixture.receipt.equal(
            "forwarded record occurs exactly once at the holder",
            "OB4",
            (1, len(matching)),
        )
        fixture.receipt.equal(
            "forwarded origin is the verified sender installation",
            "WA-13 / FL-14",
            (
                sender.runtime.manifest["serviceId"],
                field(matching[0], "origin", "installation"),
            ),
        )
        fixture.receipt.equal(
            "forwarding retains the declared human actor",
            "WA-13 / FL-14",
            (fixture.owner, matching[0].get("actor")),
        )
        fixture.receipt.equal(
            "forwarding retains the exact post text",
            "OB4",
            (
                request["text"],
                field(matching[0], "content", "record", "record", "text"),
            ),
        )
    fixture.receipt.record(
        {
            "kind": "forwardedRecords",
            "entries": entries,
            "created": created,
            "posted": posted,
        }
    )
    task_refusal(fixture, sender)


def unavailable_posts(
    fixture: BoardFixture, sender: BoardFixture, central: RealCentral, unheld: str
) -> None:
    request = create_subject_request(
        fixture,
        fixture.owner,
        {"kind": "project", "subject": {"line": "unheld forwarding"}},
    )
    request["projectId"] = unheld
    answer = sender.client.read("thread_create", request)
    fixture.receipt.record({"kind": "unheldReply", "reply": answer})
    fixture.receipt.equal(
        "unheld project reports notHeld",
        "FL-14/15",
        ("notHeld", field(answer, "result", "kind")),
    )
    fixture.runtime.stop()
    preview = central.post(
        f"/admin/v1/projects/{fixture.project}/takeover/preview", None
    )
    fixture.receipt.record({"kind": "offlinePresence", "preview": preview})
    last_seen = preview.get("lastSeen")
    if not isinstance(last_seen, int):
        raise JourneyFailure("actual disconnected holder had no independent lastSeen")
    post = fixture_identity()
    text = sender.runtime.root / "offline-root.txt"
    _ = text.write_text("offline holder must not queue this post")
    answer = sender.client.cli(
        [
            "thread",
            "create",
            "--project-id",
            fixture.project,
            "--post-id",
            post,
            "--about",
            "project:offline holder",
            "--actor",
            json.dumps(fixture.owner),
            "--no-watch",
            "--text-file",
            str(text),
        ],
        "typed offline WriteReply exits zero through the real CLI",
    )
    reply = object_value(answer["record"])
    fixture.receipt.record({"kind": "offlineHolderReply", "reply": reply})
    fixture.receipt.equal(
        "closed holder Session reports holderUnreachable",
        "FL-14",
        ("holderUnreachable", field(reply, "result", "kind")),
    )
    actual = datetime.datetime.fromisoformat(
        string_value(field(reply, "result", "decision", "lastSeen"))
    )
    fixture.receipt.equal(
        "holderUnreachable retains independently read lastSeen",
        "FL-14",
        (last_seen, int(actual.timestamp() * 1000)),
    )
    fixture.receipt.record(
        {
            "kind": "gap",
            "case": "unanswered",
            "classification": "apparatus",
            "detail": "Fixture Access edge forwards WebSocket bytes unchanged and has no selective holder-answer hold. Native offered test checks timeout and same-ID replay; public two-Host deadline remains unobserved.",
        }
    )


def joint_holder_and_forwarding(
    fixture: BoardFixture, receiver: BoardFixture, central: RealCentral
) -> None:
    unheld = publish_unheld(fixture, central)
    claim_imported_project(fixture)
    expected = write_lane(fixture, receiver, 2)
    moved = owner_transfer(
        central,
        fixture.project,
        "move",
        {
            "transferId": fixture_identity(),
            "toInstallation": receiver.runtime.manifest["serviceId"],
            "reason": "independent Claim A then Move B observation",
        },
    )
    fixture.receipt.record({"kind": "ownerMove", "reply": moved})
    writing = await_writer(receiver.client, fixture.project, 3)
    fixture.receipt.equal(
        "Move retains the acknowledged old work lanes",
        "OB3",
        (expected["workstreams"], writing["workstreams"]),
    )
    receiver.project = fixture.project
    expected = write_lane(receiver, fixture, 3)
    old_copy = await_view(fixture.client, fixture.project, expected["asOf"])
    fixture.receipt.equal(
        "prior holder's old project stays publicly readable",
        "1087 / OB3",
        (expected["workstreams"], old_copy["workstreams"]),
    )
    preview = central.post(
        f"/admin/v1/projects/{fixture.project}/takeover/preview", None
    )
    takeover = owner_transfer(
        central,
        fixture.project,
        "takeover",
        {
            "transferId": fixture_identity(),
            "toInstallation": fixture.runtime.manifest["serviceId"],
            "reason": "independent returning-holder observation",
            "preview": preview["preview"],
        },
    )
    fixture.receipt.record({"kind": "ownerTakeover", "reply": takeover})
    restored = await_writer(fixture.client, fixture.project, 4)
    fixture.receipt.equal(
        "returning A becomes writer with all acknowledged lanes",
        "1087 / OB3",
        (expected["workstreams"], restored["workstreams"]),
    )
    _ = write_lane(fixture, receiver, 4)
    task_refusal(fixture, receiver)
    forwarded_posts(fixture, receiver)
    fixture.wait_replicated()
    unavailable_posts(fixture, receiver, central, unheld)
    fixture.runtime.start()
    reopened = await_writer(fixture.client, fixture.project, 4)
    fixture.receipt.equal(
        "returning writer survives owning stop and reopen",
        "OB3 durability",
        (4, field(reopened, "currentWriter", "epoch")),
    )
    fixture.receipt.record(
        {"kind": "finalDirectory", "entry": directory_entry(central, fixture.project)}
    )
