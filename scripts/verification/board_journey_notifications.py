"""Observe exact local BoardNotification pushes without claiming native arrival."""

import contextlib
import json
import os
import select
import sqlite3
import subprocess
import time
import typing as t

from .board_journey_fixture import BoardFixture
from .board_journey_roles import RoleScenario, record_payloads
from .board_journey_values import (
    JsonObject,
    JsonValue,
    JourneyFailure,
    field,
    fixture_identity,
    json_value,
    object_value,
    string_value,
)


def local_session(fixture: BoardFixture, label: str) -> JsonObject:
    return {
        "kind": "session",
        "session": {
            "endpoint": {
                "serviceId": fixture.runtime.manifest["serviceId"],
                "endpointId": "codex-local",
            },
            "sessionId": f"m2b-{label}-{fixture_identity()}",
        },
    }


def post_intents(fixture: BoardFixture, cause: str) -> list[JsonObject]:
    return [
        row
        for row in record_payloads(fixture, "intent_list")
        if row.get("cause") == {"kind": "post", "id": cause}
    ]


def intent_actor(intent: JsonObject) -> JsonValue:
    return field(intent, "recipient", "recipient", "actor", "actor", "identity")


def locate_push_ids(fixture: BoardFixture, cause: str, minimum: int) -> list[str]:
    """Locate unknown push IDs in owned storage; all payload proof uses public reads.

    The public show API needs a push ID and its inbox only lists direct messages.
    Watch actual SQLite file writes for bounded completion instead of sleeping.
    This read-only locator neither creates a push nor simulates its receiver.
    """
    database = fixture.runtime.directory / "automation.sqlite"
    deadline = time.monotonic() + 15
    with contextlib.ExitStack() as stack:
        queue = select.kqueue()
        _ = stack.callback(queue.close)
        watches: list[select.kevent] = []
        for path in [database, database.with_name(database.name + "-wal")]:
            if path.exists():
                handle = stack.enter_context(path.open("rb"))
                watches.append(
                    select.kevent(
                        handle.fileno(),
                        filter=select.KQ_FILTER_VNODE,
                        flags=select.KQ_EV_ADD | select.KQ_EV_CLEAR,
                        fflags=select.KQ_NOTE_WRITE | select.KQ_NOTE_EXTEND,
                    )
                )
        _ = queue.control(watches, 0, 0)
        while True:
            with contextlib.closing(
                sqlite3.connect(f"file:{database}?mode=ro", uri=True)
            ) as connection:
                rows = t.cast(
                    list[tuple[str]],
                    connection.execute(
                        "SELECT push_id FROM router_pushes "
                        + "WHERE kind='board-notification' "
                        + "AND json_extract(header_facts_json,'$.project')=? "
                        + "AND json_extract(header_facts_json,'$.cause.id')=? "
                        + "ORDER BY push_id",
                        (fixture.project, cause),
                    ).fetchall(),
                )
            if len(rows) >= minimum:
                identities = [row[0] for row in rows]
                fixture.receipt.record(
                    {
                        "kind": "pushIdLocator",
                        "database": str(database),
                        "mode": "readOnly",
                        "cause": cause,
                        "project": fixture.project,
                        "pushIds": list[JsonValue](identities),
                        "publicPayloadProof": "router_show and CLI show below",
                        "observerClosedBeforeReturn": True,
                    }
                )
                return identities
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not queue.control(None, 8, remaining):
                raise JourneyFailure(
                    f"expected at least {minimum} real BoardNotification pushes for {cause}; found {rows}"
                )


def cli_push_show(fixture: BoardFixture, target: JsonObject, push: str) -> JsonObject:
    environment = os.environ.copy()
    for key in ["CODEX_THREAD_ID", "CLAUDE_CODE_SESSION_ID", "CURSOR_CONVERSATION_ID"]:
        _ = environment.pop(key, None)
    environment["CODEX_THREAD_ID"] = string_value(target["sessionId"])
    command = [
        str(fixture.runtime.target.cli),
        "show",
        push,
        "--service-directory",
        str(fixture.runtime.directory),
        "--json",
    ]
    reply = subprocess.run(
        command,
        cwd=fixture.runtime.target.source_root,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )
    output = object_value(json_value(reply.stdout)) if reply.stdout else {}
    fixture.receipt.record(
        {
            "kind": "call",
            "surface": "cli",
            "label": "observe owned recipient BoardNotification through real CLI",
            "command": list[JsonValue](command),
            "declaredFixtureCaller": target,
            "exit": reply.returncode,
            "raw": {"output": output, "stderr": reply.stderr},
        }
    )
    if reply.returncode or output.get("kind") != "result":
        raise JourneyFailure(f"CLI push observation failed: {output}")
    return object_value(field(output, "result", "record"))


def push_identity(push: JsonObject) -> JsonObject:
    return {
        key: push.get(key)
        for key in [
            "pushId",
            "kind",
            "target",
            "originRouterRef",
            "headerFacts",
            "body",
        ]
    }


def observe_pushes(fixture: BoardFixture, cause: str, actors: list[JsonObject]) -> None:
    push_ids = locate_push_ids(fixture, cause, len(actors))
    intents = post_intents(fixture, cause)
    fixture.receipt.equal(
        "addressed role resolves to exactly the active actors, one intent each",
        "RL-3/NI-2/M2b Lead ruling",
        (
            sorted(actors, key=lambda actor: json.dumps(actor, sort_keys=True)),
            sorted(
                [intent_actor(intent) for intent in intents],
                key=lambda actor: json.dumps(actor, sort_keys=True),
            ),
        ),
    )
    fixture.receipt.equal(
        "one local push is recorded per resolved actor for this post cause",
        "NI-3/M2b Lead ruling",
        (len(actors), len(push_ids)),
    )
    targets: list[JsonValue] = []
    for push in push_ids:
        matches: list[JsonObject] = []
        for actor in actors:
            target = object_value(actor["session"])
            response = object_value(
                fixture.client.call(
                    "router_show",
                    {"caller": target, "reference": push},
                    "recipient push read",
                )
            )
            if response.get("isError") is False:
                matches.append(object_value(response["value"]))
        if len(matches) != 1:
            raise JourneyFailure(
                f"push {push} must be readable by exactly one fixture recipient"
            )
        record = object_value(matches[0]["record"])
        target = object_value(record["target"])
        targets.append(target)
        intent = next(
            row
            for row in intents
            if field(
                row, "recipient", "recipient", "actor", "actor", "identity", "session"
            )
            == target
        )
        shown = fixture.client.read(
            "intent_show",
            {
                "projectId": fixture.project,
                "record": {"kind": "notifyIntent", "id": intent["intentId"]},
            },
        )
        fixture.receipt.equal(
            "intent show preserves the same cause, recipient and intent identity as list",
            "NI-2/V8",
            (
                [intent["cause"], intent["recipient"], intent["intentId"]],
                [
                    field(shown, "record", "cause"),
                    field(shown, "record", "recipient"),
                    field(shown, "record", "intentId"),
                ],
            ),
        )
        fixture.receipt.equal(
            "actual push identifies this post cause and this recipient's intent",
            "NI-3/NI-4/M2b Lead ruling",
            (
                [
                    "board-notification",
                    fixture.project,
                    intent["intentId"],
                    {"kind": "post", "id": cause},
                ],
                [
                    record.get("kind"),
                    field(record, "headerFacts", "project"),
                    field(record, "headerFacts", "intent"),
                    field(record, "headerFacts", "cause"),
                ],
            ),
        )
        cli = cli_push_show(fixture, target, push)
        fixture.receipt.equal(
            "CLI and MCP observe the same immutable notification payload",
            "V8/M2b recipient observation",
            (push_identity(record), push_identity(cli)),
        )
        fixture.receipt.record(
            {
                "kind": "recipientObservation",
                "target": target,
                "pushId": push,
                "intent": intent["intentId"],
                "deliveryState": record.get("deliveryState"),
                "lastOutcome": record.get("lastOutcome"),
                "nativeArrival": "outside M2b; not observed",
            }
        )
    fixture.receipt.equal(
        "all exact recipients observe their own notification record",
        "M2b Lead ruling",
        (
            sorted(
                [actor["session"] for actor in actors],
                key=lambda target: json.dumps(target, sort_keys=True),
            ),
            sorted(targets, key=lambda target: json.dumps(target, sort_keys=True)),
        ),
    )


def reviewer_role_notifications(fixture: BoardFixture) -> None:
    roles = RoleScenario(fixture)
    actors = [
        local_session(fixture, "reviewer-a"),
        local_session(fixture, "reviewer-b"),
    ]
    for actor in actors:
        _ = roles.grant("reviewer", actor)
    thread = fixture.create_thread()
    request = fixture.reply_request(
        thread, "M2b exact two-holder reviewer notification"
    )
    request["addressees"] = [
        {
            "kind": "role",
            "recipient": {"kind": "reviewer", "subject": fixture.workstream},
        }
    ]
    _ = fixture.client.accepted("thread_post", request)
    observe_pushes(fixture, string_value(request["postId"]), actors)
    changing = fixture.reply_request(
        thread, "M2b natural claim versus holder-change ordering"
    )
    changing["addressees"] = request["addressees"]
    _ = fixture.client.accepted("thread_post", changing)
    _ = roles.change(
        {
            "kind": "roleRelease",
            "role": "reviewer",
            "holder": roles.recorded_holder("reviewer", actors[0]),
        }
    )
    replacement = local_session(fixture, "reviewer-c")
    _ = roles.grant("reviewer", replacement)
    cause = string_value(changing["postId"])
    pushes = locate_push_ids(fixture, cause, 1)
    naturally_resolved = post_intents(fixture, cause)
    fixture.receipt.record(
        {
            "kind": "naturalReResolutionObservation",
            "cause": cause,
            "originalActors": list[JsonValue](actors),
            "currentActors": [actors[1], replacement],
            "intentsAtObservation": list[JsonValue](naturally_resolved),
            "pushIdsAtObservation": list[JsonValue](pushes),
            "forcedInterleaving": False,
            "limit": "separate real reads, no forced interleaving; deterministic claim races belong to V5 integration",
        }
    )
    next_post = fixture.reply_request(
        thread, "M2b stable recipient set after the holder change"
    )
    next_post["addressees"] = request["addressees"]
    _ = fixture.client.accepted("thread_post", next_post)
    observe_pushes(fixture, string_value(next_post["postId"]), [actors[1], replacement])
