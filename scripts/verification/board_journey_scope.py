"""Observe rejected scopes, project-watch context and semantic ACK boundaries."""

import json

from .board_journey_fixture import (
    BoardFixture,
    accepted_decision,
    project_thread_subject,
)
from .board_journey_orphan import (
    damage_rejected_orphan,
    observe_excluded_orphan_pages,
    require_damaged_orphan_replay_refusal,
)
from .board_journey_runtime import (
    write_header,
)
from .board_journey_values import (
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
    json_value,
    object_value,
    string_value,
)


def workspace_summary(fixture: BoardFixture) -> JsonObject:
    matches = [
        object_value(row)
        for row in fixture.summaries()
        if field(row, "workspaceId") == fixture.workspace
    ]
    fixture.receipt.equal(
        "owned workspace summary exists exactly once",
        "G3/PD1 inbox",
        (
            1,
            len(matches),
        ),
    )
    return matches[0]


def acknowledge(fixture: BoardFixture, entries: list[JsonValue]) -> JsonObject:
    return fixture.client.read(
        "inbox_acknowledge",
        {
            "protocolVersion": 1,
            "actor": fixture.reader,
            "actingFor": None,
            "entries": entries,
        },
    )


def acknowledgement_entry(scope: JsonObject, through: JsonValue) -> JsonObject:
    return {
        "requestId": fixture_identity(),
        "resubmits": None,
        "observedEpoch": 1,
        "scope": scope,
        "through": through,
    }


def rejected_scope_and_workspace_summary(fixture: BoardFixture) -> None:
    fixture.receipt.equal(
        "reader without interest has no workspace summaries",
        "G3/PD1 inbox",
        ([], fixture.summaries()),
    )
    _ = fixture.client.accepted(
        "project_watch",
        {
            **write_header(fixture.reader),
            "observedEpoch": 1,
            "projectId": fixture.project,
        },
    )
    fixture.receipt.equal(
        "watched workspace starts uninitialized and without unread activity",
        "G3/PD1 inbox",
        (
            {
                "workspaceId": fixture.workspace,
                "hasUnread": False,
                "mainTrackingInitialized": False,
            },
            workspace_summary(fixture),
        ),
    )
    fixture.initialize()
    initial = workspace_summary(fixture)
    fixture.receipt.equal(
        "initialized workspace has no unread activity",
        "G3/PD1 inbox",
        (
            {
                "workspaceId": fixture.workspace,
                "hasUnread": False,
                "mainTrackingInitialized": True,
            },
            initial,
        ),
    )
    session, scope_fact = fixture.metadata_session()
    sentinel = f"M2 verification sentinel {fixture_identity()}"
    post = fixture_identity()
    _ = fixture.client.accepted(
        "thread_create",
        {
            **write_header(fixture.owner),
            "observedEpoch": 1,
            "postId": post,
            "projectId": fixture.project,
            "about": project_thread_subject("Rejected-scope healthy sentinel"),
            "text": sentinel,
            "references": [],
            "addressees": [],
            "role": None,
            "watch": False,
        },
    )
    shown = fixture.client.observe("post_show", {"postId": post}, "G2/post readback")
    fixture.receipt.equal(
        "created sentinel has exact identity and content",
        "RC-1/RC-8",
        (
            [post, fixture.project, sentinel],
            [shown.get("postId"), shown.get("projectId"), shown.get("text")],
        ),
    )
    cursor = field(shown, "position", "position")
    scopes: list[JsonObject] = [
        {"kind": "board", "boardId": fixture.board},
        {"kind": "workspace", "workspaceId": fixture.workspace},
        {"kind": "allWorkspaces"},
    ]
    rejected_project = fixture_identity()
    rejected_request: JsonObject = {
        **write_header(fixture.owner),
        "boardId": fixture.board,
        "projectId": rejected_project,
        "title": fixture.project_titles[fixture.project],
    }
    original_context: JsonObject | None = None
    for phase in [
        "before rejection",
        "after rejection",
        "after damaged orphan restart",
    ]:
        if phase == "after damaged orphan restart":
            old_service = fixture.runtime.manifest.get("serviceId")
            old_epoch = fixture.runtime.manifest.get("serviceEpoch")
            fixture.runtime.stop()
            damage_rejected_orphan(fixture, rejected_project)
            fixture.runtime.start()
            fixture.receipt.equal(
                "damaged-orphan Host restart retains service identity and changes epoch",
                "S-R3-F1/cold registry",
                (
                    [old_service, True],
                    [
                        fixture.runtime.manifest.get("serviceId"),
                        fixture.runtime.manifest.get("serviceEpoch") != old_epoch,
                    ],
                ),
            )
        for scope in scopes:
            for archived in [False, True]:
                searched = fixture.client.observe(
                    "post_search",
                    {
                        "query": sentinel,
                        "scope": scope,
                        "kind": "both",
                        "includeArchived": archived,
                        "page": {"limit": 1},
                    },
                    "G2/O-R2-F1",
                )
                hits = array_value(field(searched, "page", "records"))
                fixture.receipt.equal(
                    f"search exact healthy hit {phase}: {scope['kind']}/{archived}",
                    "G2/O-R2-F1",
                    (
                        [[fixture.workspace, post, sentinel]],
                        [
                            [
                                field(hit, "workspaceId"),
                                field(hit, "post", "postId"),
                                field(hit, "post", "text"),
                            ]
                            for hit in hits
                        ],
                    ),
                )
                fixture.receipt.equal(
                    "single-hit search terminates",
                    "PD1 bounded pages",
                    (
                        None,
                        field(searched, "page", "nextCursor"),
                    ),
                )
        context = fixture.client.observe(
            "hook_context", {"session": session}, "G1/VW-5/VW-7"
        )
        fixture.receipt.equal(
            f"context reaches the actual session resolver {phase}",
            "G1/VW-7",
            (
                ["ok", "context", session],
                [
                    context.get("kind"),
                    field(context, "payload", "kind"),
                    field(context, "payload", "payload", "session"),
                ],
            ),
        )
        fixture.receipt.equal(
            f"nonempty assignment and scope facts survive {phase}",
            "G1/VW-5",
            (
                [
                    [{"kind": "implementer", "subject": fixture.workstream}],
                    [scope_fact],
                ],
                [
                    field(context, "payload", "payload", "assignments"),
                    field(context, "payload", "payload", "acknowledgedScopes"),
                ],
            ),
        )
        if original_context is None:
            original_context = context
        else:
            fixture.receipt.equal(
                f"complete healthy context is unchanged {phase}",
                "S-R3-F1/VW-5/VW-7",
                (original_context, context),
            )
        fixture.receipt.equal(
            f"workspace remains initialized and unread {phase}",
            "G3/O-R2-F1",
            (
                {
                    "workspaceId": fixture.workspace,
                    "hasUnread": True,
                    "mainTrackingInitialized": True,
                },
                workspace_summary(fixture),
            ),
        )
        if phase == "before rejection":
            rejected = fixture.client.read(
                "project_create",
                rejected_request,
            )
            fixture.receipt.equal(
                "same-title create reaches actual rejection",
                "WA-10/O-R2-F1",
                (
                    "voided",
                    field(rejected, "result", "decision", "kind"),
                ),
            )
        if phase == "after damaged orphan restart":
            observe_excluded_orphan_pages(fixture, scopes, shown)
    require_damaged_orphan_replay_refusal(fixture, rejected_request)
    entry = acknowledgement_entry(
        {"kind": "project", "projectId": fixture.project}, cursor
    )
    acknowledged = fixture.client.cli(
        [
            "inbox",
            "acknowledge",
            "--actor",
            json.dumps(fixture.reader),
            "--entry",
            json.dumps(entry),
        ],
        "acknowledge exact main scope through real CLI",
    )
    _ = accepted_decision(acknowledged)
    fixture.receipt.equal(
        "acknowledging covered main activity clears unread",
        "G3/RC-5",
        (
            {
                "workspaceId": fixture.workspace,
                "hasUnread": False,
                "mainTrackingInitialized": True,
            },
            workspace_summary(fixture),
        ),
    )


def project_watch_keeps_foreign_thread_context(fixture: BoardFixture) -> None:
    fixture.initialize()
    _, foreign = fixture.foreign_project()
    thread = fixture.create_thread(foreign)
    _ = fixture.client.accepted(
        "project_watch",
        {
            **write_header(fixture.reader),
            "observedEpoch": 1,
            "projectId": foreign,
        },
    )
    _ = fixture.client.accepted(
        "thread_resolve",
        {
            **write_header(fixture.owner),
            "observedEpoch": 1,
            "thread": {"project": foreign, "thread": thread},
            "reason": "done",
        },
    )
    answer = fixture.inbox()
    fixture.runtime.stop()
    rows = fixture.offline_rows(foreign, "SELECT reader,thread_id FROM thread_watches")
    fixture.receipt.equal(
        "project watch created no existing-thread watch row",
        "O-R2-F3 fixture",
        (
            [],
            [
                row[1]
                for row in rows
                if row[1] == thread
                and json_value(string_value(row[0])) == fixture.reader
            ],
        ),
    )
    selected = [
        row
        for row in array_value(field(answer, "page", "records"))
        if field(row, "threadId") == thread
    ]
    fixture.receipt.equal(
        "foreign project-watched resolution retains inbox context",
        "O-R2-F3/PD1 §9",
        (
            [["threadStateChanged", foreign, thread, "resolved"]],
            [
                [
                    field(row, "kind"),
                    field(row, "projectId"),
                    field(row, "threadId"),
                    field(row, "state"),
                ]
                for row in selected
            ],
        ),
    )
    fixture.receipt.equal(
        "foreign watch does not import main activity",
        "PD1 inbox scope",
        (
            1,
            len(array_value(field(answer, "page", "records"))),
        ),
    )


def semantic_acknowledgement_rejection(fixture: BoardFixture) -> None:
    thread = fixture.create_thread()
    fixture.initialize()
    _ = fixture.client.accepted(
        "thread_watch",
        {
            **write_header(fixture.reader),
            "observedEpoch": 1,
            "thread": {"project": fixture.project, "thread": thread},
        },
    )
    posts = [
        fixture.reply(thread, f"unread verification {number}") for number in range(2)
    ]
    expected = [post["postId"] for post in posts]
    before = fixture.inbox()
    observed = [
        field(row, "post", "postId")
        for row in array_value(field(before, "page", "records"))
    ]
    fixture.receipt.equal(
        "two exact messages are initially unread", "VW-6", (expected, observed)
    )
    paged: list[JsonValue] = []
    cursor: JsonValue = None
    for _ in range(3):
        page = fixture.inbox({"page": {"limit": 1, "cursor": cursor}})
        paged.extend(
            field(row, "post", "postId")
            for row in array_value(field(page, "page", "records"))
        )
        cursor = field(page, "page", "nextCursor")
        if cursor is None:
            break
    fixture.receipt.equal(
        "complete unread inbox continuation preserves exact scope order",
        "RC-5/VW-6",
        ([expected, None], [paged, cursor]),
    )
    last = fixture.client.read("post_show", {"postId": posts[-1]["postId"]})
    through = field(last, "position", "position")
    entries: list[JsonValue] = [
        acknowledgement_entry(
            {"kind": "thread", "thread": {"thread": thread}}, through
        ),
        acknowledgement_entry(
            {
                "kind": "thread",
                "thread": {
                    "project": fixture.project,
                    "thread": thread,
                },
            },
            through,
        ),
    ]
    refused = object_value(
        fixture.client.call(
            "inbox_acknowledge",
            {
                "protocolVersion": 1,
                "actor": fixture.reader,
                "actingFor": None,
                "entries": entries,
            },
            "duplicate semantic ACK must reject before effects",
        )
    )
    fixture.receipt.equal(
        "duplicate semantic ACK is a public refusal",
        "S-R2-F5",
        (
            True,
            refused.get("isError"),
        ),
    )
    error = object_value(refused.get("value"))
    fixture.receipt.equal(
        "ACK refusal identifies request validation before effects",
        "S-R2-F5",
        (
            ["error", "invalidField", "validation", "fieldConstraint", "request"],
            [
                error.get("mcpResult"),
                error.get("kind"),
                error.get("stage"),
                field(error, "details", "kind"),
                field(error, "details", "field"),
            ],
        ),
    )
    after = fixture.inbox()
    fixture.receipt.equal(
        "rejected ACK leaves both exact activities unread",
        "S-R2-F5/RC-12",
        (
            expected,
            [
                field(row, "post", "postId")
                for row in array_value(field(after, "page", "records"))
            ],
        ),
    )
    fixture.runtime.stop()
    ledger = fixture.offline_rows(
        fixture.project, "SELECT request_id FROM request_ledger"
    )
    present = {string_value(row[0]) for row in ledger}
    fixture.receipt.equal(
        "rejected batch consumed neither entry request ID",
        "S-R2-F5/RC-4",
        (
            [],
            [
                field(entry, "requestId")
                for entry in entries
                if string_value(field(entry, "requestId")) in present
            ],
        ),
    )
    fixture.runtime.start()
    _ = accepted_decision(acknowledge(fixture, [entries[0]]))
    fixture.receipt.equal(
        "corrected ACK with same ID covers the original range",
        "RC-4/VW-6",
        (
            [],
            field(
                fixture.client.cli(
                    [
                        "inbox",
                        "fetch",
                        "--scope",
                        "workspace",
                        "--workspace-id",
                        fixture.workspace,
                        "--reader",
                        json.dumps(fixture.reader),
                    ],
                    "read exact empty workspace inbox through real CLI",
                ),
                "page",
                "records",
            ),
        ),
    )
