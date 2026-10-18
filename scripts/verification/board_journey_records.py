"""Observe keyed request effects, move verdicts and causal late facts."""

import datetime

from .board_journey_fixture import BoardFixture, accepted_decision, record_reference
from .board_journey_record_fixture import feed_records, governing_definition
from .board_journey_runtime import (
    decision_values,
    project_request,
)
from .board_journey_values import (
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
)


def request_result(reply: JsonObject) -> JsonValue:
    return reply.get("result")


def refusal_kinds(reply: JsonObject) -> list[JsonValue]:
    decisions = decision_values(reply)
    if len(decisions) != 1:
        return []
    return [
        field(reason, "reason", "kind")
        if field(reason, "kind") == "move"
        else field(reason, "kind")
        for reason in array_value(decisions[0].get("reasons"))
    ]


def record_evaluator(fixture: BoardFixture, fields: JsonObject) -> JsonObject:
    actor = next(
        role["holder"]
        for role in feed_records(fixture, "roleAssignment")
        if role.get("role") == "lead"
    )
    fact: JsonObject = {
        "factId": fixture_identity(),
        "kind": {
            "kind": "evaluatorResult",
            "fact": {
                "guard": "proof",
                "subject": record_reference("project", fixture.project),
                "outcome": "met",
                "evidence": [],
                "evaluator": {"actor": actor, "engine": None, "calibration": None},
                "executor": None,
            },
        },
        "links": [record_reference("project", fixture.project)],
        "producerTime": datetime.datetime.now(datetime.UTC).isoformat(),
        "anchor": None,
        **fields,
    }
    return project_request(
        fixture.owner, fixture.project, {"kind": "factRecord", "fact": fact}
    )


def keyed_replay_moves_and_late_facts(fixture: BoardFixture) -> None:
    thread = fixture.create_thread()
    request = fixture.reply_request(thread, "original keyed immutable post")
    first = fixture.client.accepted("thread_post", request)
    original_decision = accepted_decision(first)
    original_post = fixture.client.read("post_show", {"postId": request["postId"]})
    before = fixture.feed()
    replay = fixture.client.read("thread_post", request)
    fixture.receipt.equal(
        "accepted post replay returns original IDs positions and decision",
        "RC-4",
        (
            original_decision,
            accepted_decision(replay),
        ),
    )
    fixture.receipt.equal(
        "accepted post replay repeats no feed effects",
        "RC-4/RC-12",
        (
            before.get("value"),
            fixture.feed().get("value"),
        ),
    )
    conflicting = {**request, "text": "conflicting content under the same request ID"}
    conflict = fixture.client.read("thread_post", conflicting)
    fixture.receipt.equal(
        "same request ID with different content conflicts",
        "RC-4",
        (
            "conflictingRequest",
            field(conflict, "result", "kind"),
        ),
    )
    fixture.receipt.equal(
        "conflict does not alter original content or feed",
        "RC-4/RC-12",
        (
            [original_post, before.get("value")],
            [
                fixture.client.read("post_show", {"postId": request["postId"]}),
                fixture.feed().get("value"),
            ],
        ),
    )
    _ = governing_definition(fixture, True)
    check: JsonObject = {
        "projectId": fixture.project,
        "actor": fixture.owner,
        "moveName": "finish",
        "expectedRevision": 1,
    }
    before_check = fixture.feed()
    verdict = fixture.client.observe("move_check", check, "G7/MV-4")
    fixture.receipt.equal(
        "missing recorded evaluator result never permits a move",
        "MV-2/MV-4",
        (
            ["rejected", ["guardUnavailable"]],
            [
                verdict.get("kind"),
                [
                    field(reason, "kind")
                    for reason in array_value(verdict.get("reasons"))
                ],
            ],
        ),
    )
    fixture.receipt.equal(
        "move check leaves no record or intent behind",
        "MV-4",
        (
            before_check.get("value"),
            fixture.feed().get("value"),
        ),
    )
    rejected_request = project_request(
        fixture.owner,
        fixture.project,
        {
            "kind": "moveRequest",
            "moveId": fixture_identity(),
            "moveName": "finish",
            "evidence": [],
            "reason": "attempt without recorded proof",
            "expectedRevision": 1,
            "addressees": [],
        },
    )
    rejected = fixture.client.read("move_request", rejected_request)
    fixture.receipt.equal(
        "attempt records the actual guard refusal",
        "MV-2/MV-3",
        (
            ["guardUnavailable"],
            refusal_kinds(rejected),
        ),
    )
    rejected_head = fixture.feed()
    retried = fixture.client.read("move_request", rejected_request)
    fixture.receipt.equal(
        "rejected move replay preserves the terminal decision",
        "RC-4",
        (
            request_result(rejected),
            request_result(retried),
        ),
    )
    fixture.receipt.equal(
        "rejected replay repeats no record or intent",
        "RC-4/NI-3",
        (
            rejected_head.get("value"),
            fixture.feed().get("value"),
        ),
    )
    evaluator = record_evaluator(fixture, {})
    _ = fixture.client.accepted("fact_record", evaluator)
    before_allowed = fixture.feed()
    allowed = fixture.client.observe("hook_check", check, "G7/VW-7/MV-4")
    fixture.receipt.equal(
        "recorded evaluator result enables exactly the pinned edge",
        "MV-2/VW-7",
        (
            ["ok", "check", "allowed"],
            [
                allowed.get("kind"),
                field(allowed, "payload", "kind"),
                field(allowed, "payload", "payload", "kind"),
            ],
        ),
    )
    fixture.receipt.equal(
        "allowed hook check remains side-effect free",
        "MV-4",
        (
            before_allowed.get("value"),
            fixture.feed().get("value"),
        ),
    )
    accepted_request = project_request(
        fixture.owner,
        fixture.project,
        {
            "kind": "moveRequest",
            "moveId": fixture_identity(),
            "moveName": "finish",
            "evidence": [],
            "reason": "recorded proof now met",
            "expectedRevision": 1,
            "addressees": [],
        },
    )
    accepted = fixture.client.accepted("move_request", accepted_request)
    accepted_head = fixture.feed()
    fixture.receipt.equal(
        "accepted move replay preserves exact records and positions",
        "RC-4",
        (
            accepted_decision(accepted),
            accepted_decision(fixture.client.read("move_request", accepted_request)),
        ),
    )
    fixture.receipt.equal(
        "accepted move replay repeats no feed effects",
        "RC-4/RC-12",
        (
            accepted_head.get("value"),
            fixture.feed().get("value"),
        ),
    )
    state = feed_records(fixture, "project")[-1]
    fixture.receipt.equal(
        "project status changes through the accepted move",
        "MV-1",
        (
            "done",
            state.get("status"),
        ),
    )
    before_close = field(fixture.feed(), "value", "highWater")
    _ = fixture.client.accepted(
        "project_close",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "projectClose",
                "reason": "verification scope completed",
                "expectedRevision": 2,
            },
        ),
    )
    late_request = record_evaluator(fixture, {"anchor": before_close})
    _ = fixture.client.accepted("fact_record", late_request)
    late_id = field(late_request, "operation", "fact", "factId")
    fact = fixture.client.observe(
        "fact_show",
        {
            "projectId": fixture.project,
            "record": {"kind": "fact", "id": late_id},
        },
        "RC-11/late fact readback",
    )
    fixture.receipt.equal(
        "causal pre-close fact is accepted and marked late",
        "RC-11",
        (
            [True, before_close],
            [field(fact, "record", "late"), field(fact, "record", "anchor")],
        ),
    )
    unanchored = fixture.client.read("fact_record", record_evaluator(fixture, {}))
    fixture.receipt.equal(
        "an unanchored post-close fact is explicitly refused",
        "RC-11",
        (
            ["closed"],
            refusal_kinds(unanchored),
        ),
    )
    intents = feed_records(fixture, "notifyIntent")
    fixture.receipt.equal(
        "late fact emits no notify intent",
        "RC-11/NI-3",
        (
            [],
            [
                intent.get("intentId")
                for intent in intents
                if field(intent, "cause", "id") == late_id
            ],
        ),
    )
