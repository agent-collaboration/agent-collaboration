"""Observe actual poll-range settlement and same-reader membership recovery."""

import json

from .board_journey_failures import main_post
from .board_journey_fixture import (
    BoardFixture,
    accepted_decision,
    record_reference,
    thread_join_header,
)
from .board_journey_runtime import (
    project_request,
    write_header,
)
from .board_journey_values import (
    JsonObject,
    array_value,
    field,
    integer_value,
    object_value,
    string_value,
)


def subscribe(fixture: BoardFixture, actor: JsonObject, scope: JsonObject) -> None:
    _ = fixture.client.accepted(
        "thread_subscribe",
        {
            **write_header(actor),
            "scope": scope,
            "policy": {
                "mode": "poll",
                "quietSeconds": 0,
                "capSeconds": 0,
                "lifetimeSeconds": 86_400,
            },
        },
    )


def membership_record(
    fixture: BoardFixture, project: str, actor: JsonObject
) -> JsonObject:
    feed = fixture.client.read(
        "feed_read",
        {
            "projectId": project,
            "reader": {"kind": "anonymous"},
            "after": {"epoch": 1, "position": 0},
            "limit": 100,
            "includeRedacted": False,
        },
    )
    candidates = [
        object_value(field(entry, "content", "record", "record"))
        for entry in array_value(field(feed, "value", "entries"))
        if field(entry, "content", "record", "kind") == "subscriptionMembership"
        and field(entry, "content", "record", "record", "reader") == actor
    ]
    if not candidates:
        raise ValueError("public feed has no expected subscription membership")
    return candidates[-1]


def wait_for_project(
    fixture: BoardFixture, actor: JsonObject, project: str
) -> JsonObject:
    return fixture.client.observe(
        "thread_wait",
        {
            "protocolVersion": 1,
            "actingFor": None,
            "actor": actor,
            "renewals": [],
            "filter": {"kind": "project", "projectId": project},
            "maxWaitSeconds": 1,
        },
        "G5/actual poll delivery",
    )


def notice_for_post(
    fixture: BoardFixture, post: JsonObject, project: str
) -> JsonObject:
    shown = fixture.client.read("post_show", {"postId": post["postId"]})
    cursor = field(shown, "position", "position")
    return {
        "threadId": field(post, "placement", "thread", "thread"),
        "projectId": project,
        "from": cursor,
        "through": cursor,
        "postCount": 1,
    }


def require_exact_batch(
    fixture: BoardFixture, expected: JsonObject, observed: JsonObject
) -> None:
    fixture.receipt.equal(
        "wait delivers exactly the owed range once",
        "G5/PD1 C5",
        (
            ["batch", "ranges", [expected]],
            [
                field(observed, "outcome", "kind"),
                field(observed, "outcome", "batch", "kind"),
                field(observed, "outcome", "batch", "roots"),
            ],
        ),
    )


def no_repeated_range(fixture: BoardFixture, actor: JsonObject, project: str) -> None:
    answer = fixture.client.observe(
        "thread_wait",
        {
            "protocolVersion": 1,
            "actingFor": None,
            "actor": actor,
            "renewals": [],
            "filter": {"kind": "project", "projectId": project},
            "maxWaitSeconds": 0,
        },
        "G5/settlement without repeats",
    )
    fixture.receipt.equal(
        "second bounded wait has no repeated range",
        "G5/PD1 C5",
        (
            "timedOut",
            field(answer, "outcome", "kind"),
        ),
    )


def poll_ranges_survive_policy_change_and_restart(fixture: BoardFixture) -> None:
    thread = fixture.create_thread()
    _ = fixture.client.accepted(
        "thread_join",
        {
            **thread_join_header(fixture.reader),
            "thread": {"project": fixture.project, "thread": thread},
            "role": "participant",
            "watch": True,
            "replace": None,
            "note": None,
        },
    )
    scope: JsonObject = {
        "kind": "thread",
        "thread": {"project": fixture.project, "thread": thread},
    }
    _ = accepted_decision(
        fixture.client.cli(
            [
                "thread",
                "subscribe",
                "--thread-id",
                f"{fixture.project}/{thread}",
                "--actor",
                json.dumps(fixture.reader),
                "--mode",
                "poll",
                "--quiet",
                "0s",
                "--cap",
                "0s",
                "--for",
                "24h",
            ],
            "subscribe immediate human poll through real CLI",
        )
    )
    initial = membership_record(fixture, fixture.project, fixture.reader)
    fixture.receipt.equal(
        "new membership carries its declared immediate poll policy",
        "G5/membership",
        (
            [fixture.reader, "poll", 0, 0],
            [
                initial.get("reader"),
                field(initial, "policy", "mode"),
                field(initial, "policy", "timing", "quietSeconds"),
                field(initial, "policy", "timing", "capSeconds"),
            ],
        ),
    )
    first = fixture.reply(thread, "first actual range")
    require_exact_batch(
        fixture,
        notice_for_post(fixture, first, fixture.project),
        wait_for_project(fixture, fixture.reader, fixture.project),
    )
    no_repeated_range(fixture, fixture.reader, fixture.project)
    second = fixture.reply(thread, "message between bounded wait calls")
    require_exact_batch(
        fixture,
        notice_for_post(fixture, second, fixture.project),
        wait_for_project(fixture, fixture.reader, fixture.project),
    )
    no_repeated_range(fixture, fixture.reader, fixture.project)
    owed = fixture.reply(thread, "owed range survives policy update and restart")
    owed_range = notice_for_post(fixture, owed, fixture.project)
    _ = fixture.client.accepted(
        "thread_subscribe",
        {
            **write_header(fixture.reader),
            "scope": scope,
            "policy": {
                "mode": "poll",
                "quietSeconds": 0,
                "capSeconds": 1,
                "lifetimeSeconds": 172_800,
            },
        },
    )
    changed = membership_record(fixture, fixture.project, fixture.reader)
    fixture.receipt.equal(
        "policy change keeps activation but advances generation",
        "G5/PD1 C5",
        (
            [
                field(initial, "envelope", "id"),
                initial.get("activatedAt"),
                integer_value(initial.get("generation")) + 1,
            ],
            [
                field(changed, "envelope", "id"),
                changed.get("activatedAt"),
                changed.get("generation"),
            ],
        ),
    )
    subscriptions = fixture.client.read(
        "thread_subscriptions", {"actor": fixture.reader}
    )
    fixture.receipt.equal(
        "healthy subscription list has the required empty unavailable field",
        "round4 §4/public subscription availability",
        ([], subscriptions.get("unavailable")),
    )
    cli_subscriptions = fixture.client.cli(
        ["thread", "subscriptions", "--actor", json.dumps(fixture.reader)],
        "real CLI pending subscription outcome",
    )
    fixture.receipt.equal(
        "CLI and MCP expose the identical active subscription state",
        "G5/CLI-MCP",
        (subscriptions, cli_subscriptions.get("record")),
    )
    rows = array_value(subscriptions.get("subscriptions"))
    fixture.receipt.equal(
        "policy change retains one owed range",
        "G5/PD1 C5",
        (
            [1],
            [field(row, "pendingCount") for row in rows],
        ),
    )
    before = fixture.client.read(
        "post_list",
        {
            "scope": {
                "kind": "thread",
                "thread": {"project": fixture.project, "thread": thread},
            },
            "selection": {"kind": "latest"},
            "page": {"limit": 100},
        },
    )
    before_thread = fixture.client.read(
        "thread_show",
        {
            "thread": {"project": fixture.project, "thread": thread},
            "reader": {"kind": "anonymous"},
        },
    )
    fixture.restart()
    after_thread = fixture.client.observe(
        "thread_show",
        {
            "thread": {"project": fixture.project, "thread": thread},
            "reader": {"kind": "anonymous"},
        },
        "RC-4/thread metadata durability",
    )
    fixture.receipt.equal(
        "thread metadata survives restart exactly",
        "RC-4/durability",
        (before_thread, after_thread),
    )
    after_membership = membership_record(fixture, fixture.project, fixture.reader)
    fixture.receipt.equal(
        "membership identity policy generation and activation persist exactly",
        "G5/durability",
        (
            changed,
            after_membership,
        ),
    )
    after = fixture.client.read(
        "post_list",
        {
            "scope": {
                "kind": "thread",
                "thread": {"project": fixture.project, "thread": thread},
            },
            "selection": {"kind": "latest"},
            "page": {"limit": 100},
        },
    )
    fixture.receipt.equal(
        "post identities order and bodies survive restart",
        "RC-4/durability",
        (
            field(before, "page", "records"),
            field(after, "page", "records"),
        ),
    )
    require_exact_batch(
        fixture, owed_range, wait_for_project(fixture, fixture.reader, fixture.project)
    )
    no_repeated_range(fixture, fixture.reader, fixture.project)


def same_reader_membership_corruption_is_isolated(fixture: BoardFixture) -> None:
    _, broken = fixture.foreign_project()
    # This case uses the same actual actor as the permanent Host regression.
    reader = fixture.owner
    for project in [fixture.project, broken]:
        subscribe(fixture, reader, {"kind": "project", "projectId": project})
    _ = fixture.client.read("thread_subscriptions", {"actor": reader})
    broken_membership = membership_record(fixture, broken, reader)
    posts: dict[str, JsonObject] = {}
    for project in [fixture.project, broken]:
        thread = main_post(fixture, project)
        # Project membership's activation predates each new other-authored root.
        posts[project] = {"postId": thread, "placement": {"thread": {"thread": thread}}}
    expected = notice_for_post(fixture, posts[fixture.project], fixture.project)
    old_service = fixture.runtime.manifest.get("serviceId")
    fixture.runtime.stop()
    policy = object_value(broken_membership.get("policy")).copy()
    policy["mode"] = "corrupt-mode"
    fixture.corrupt_record(
        broken,
        record_reference(
            "subscriptionMembership",
            string_value(field(broken_membership, "envelope", "id")),
        ),
        {"policy": policy},
    )
    fixture.runtime.start()
    fixture.receipt.equal(
        "same-reader corrupt membership does not withdraw service",
        "S-R2-F1/PD2 §6",
        (
            old_service,
            fixture.runtime.manifest.get("serviceId"),
        ),
    )
    endpoints = fixture.client.cli(
        ["endpoints", "list"], "endpoint remains available after corrupt membership"
    )
    fixture.receipt.equal(
        "endpoint discovery still answers after membership recovery",
        "S-R2-F1",
        (
            [],
            field(endpoints, "page", "records"),
        ),
    )
    _ = fixture.client.accepted(
        "workstream_rename",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "workstreamRename",
                "workstreamId": fixture.workstream,
                "name": "healthy write after corrupt membership",
                "expectedRevision": 0,
            },
        ),
    )
    shown = fixture.client.observe(
        "workstream_show",
        {
            "projectId": fixture.project,
            "record": record_reference("workstream", fixture.workstream),
        },
        "S-R2-F1/healthy write readback",
    )
    fixture.receipt.equal(
        "healthy write is actually readable after recovery",
        "S-R2-F1",
        (
            "healthy write after corrupt membership",
            field(shown, "record", "name"),
        ),
    )
    for project, expected_kind in [(fixture.project, "ok"), (broken, "unavailable")]:
        card = fixture.client.observe(
            "hook_card",
            {
                "projectId": project,
                "actor": reader,
                "subject": {"kind": "project"},
            },
            "S-R2-F1/explicit project availability",
        )
        fixture.receipt.equal(
            "corruption availability is attributed to one project",
            "S-R2-F1/PD2 §6",
            (
                expected_kind,
                card.get("kind"),
            ),
        )
        if project == broken:
            fixture.receipt.equal(
                "unavailable result names the corrupt project",
                "S-R2-F1",
                (
                    broken,
                    field(card, "payload", "reason", "project"),
                ),
            )
        else:
            fixture.receipt.equal(
                "healthy restored card still contains its actual workstream",
                "S-R2-F1/VW-1",
                (1, field(card, "payload", "payload", "counts", "workstream")),
            )
    direct_broken = object_value(
        fixture.client.call(
            "feed_read",
            {
                "projectId": broken,
                "reader": {"kind": "anonymous"},
                "after": {"epoch": 1, "position": 0},
                "limit": 100,
                "includeRedacted": False,
            },
            "direct read of corrupt membership project refuses",
        )
    )
    fixture.receipt.equal(
        "direct project read refuses instead of omitting its corrupt membership",
        "S-R2-F1/strict direct diagnostics",
        (
            [True, "error", "storage", True],
            [
                direct_broken.get("isError"),
                field(direct_broken, "value", "mcpResult"),
                field(direct_broken, "value", "stage"),
                field(direct_broken, "value", "kind")
                in {"invalidRecord", "boardUnavailable"},
            ],
        ),
    )
    listed = fixture.client.observe(
        "thread_subscriptions", {"actor": reader}, "S-R2-F1/healthy restoration"
    )
    fixture.receipt.equal(
        "subscription list explicitly reports the contained corrupt project",
        "round4 §4/public subscription availability",
        ([broken], listed.get("unavailable")),
    )
    fixture.receipt.equal(
        "same reader retains the healthy project membership",
        "S-R2-F1",
        (
            [[fixture.project, 1]],
            [
                [field(row, "projectId"), field(row, "pendingCount")]
                for row in array_value(listed.get("subscriptions"))
            ],
        ),
    )
    require_exact_batch(
        fixture, expected, wait_for_project(fixture, reader, fixture.project)
    )
    no_repeated_range(fixture, reader, fixture.project)
