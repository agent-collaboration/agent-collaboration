"""Observe complete public post and participant page traversal."""

import json

from .board_journey_fixture import (
    BoardFixture,
    accepted_decision,
    record_reference,
    thread_join_header,
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
    object_value,
    string_value,
)


def complete_post_continuation_and_participants(fixture: BoardFixture) -> None:
    thread = fixture.create_thread()
    second_project = fixture_identity()
    fixture.create_project(second_project)
    second_thread = fixture.create_thread(second_project)
    posts = [
        fixture.reply(
            thread,
            f"complete traversal {number}",
            fixture.project if number == 0 else None,
        )
        for number in range(3)
    ]
    expected = [post["postId"] for post in reversed(posts)] + [thread]
    observed: list[JsonValue] = []
    cursor: str | None = None
    for number in range(len(expected) + 1):
        if number % 2 == 0:
            arguments: JsonObject = {
                "scope": {
                    "kind": "thread",
                    "thread": {"project": fixture.project, "thread": thread},
                },
                "selection": {"kind": "latest"},
                "page": {"limit": 1, "cursor": cursor},
            }
            result = fixture.client.observe(
                "post_list", arguments, "G2/complete cross-transport pages"
            )
        else:
            arguments_cli = [
                "post",
                "list",
                "--scope",
                "thread",
                "--thread-id",
                f"{fixture.project}/{thread}",
                "--limit",
                "1",
            ]
            if cursor is not None:
                arguments_cli.extend(["--cursor", cursor])
            result = fixture.client.cli(
                arguments_cli, "continue exact post traversal via CLI"
            )
        observed.extend(
            field(post, "postId")
            for post in array_value(field(result, "page", "records"))
        )
        next_cursor = field(result, "page", "nextCursor")
        cursor = string_value(next_cursor) if next_cursor is not None else None
        if cursor is None:
            break
    fixture.receipt.equal(
        "complete post traversal has no omissions repeats or order changes",
        "RC-5/PD1 pages",
        (
            [expected, None],
            [observed, cursor],
        ),
    )
    workspace_posts: list[JsonValue] = []
    workspace_cursor: str | None = None
    for number in range(len(expected) + 2):
        if number % 2 == 0:
            page = fixture.client.observe(
                "post_list",
                {
                    "scope": {"kind": "workspace", "workspaceId": fixture.workspace},
                    "selection": {"kind": "latest"},
                    "page": {"limit": 1, "cursor": workspace_cursor},
                },
                "RC-5/complete workspace post continuation",
            )
        else:
            arguments_cli = [
                "post",
                "list",
                "--scope",
                "workspace",
                "--workspace-id",
                fixture.workspace,
                "--limit",
                "1",
            ]
            if workspace_cursor is not None:
                arguments_cli.extend(["--cursor", workspace_cursor])
            page = fixture.client.cli(
                arguments_cli, "continue complete workspace posts through real CLI"
            )
        workspace_posts.extend(
            field(post, "postId")
            for post in array_value(field(page, "page", "records"))
        )
        next_workspace = field(page, "page", "nextCursor")
        workspace_cursor = (
            string_value(next_workspace) if next_workspace is not None else None
        )
        if workspace_cursor is None:
            break
    fixture.receipt.equal(
        "workspace post continuation returns the exact complete cross-project set once",
        "RC-5/PD1 aggregate pages",
        (
            list[JsonValue](
                sorted(
                    string_value(identity) for identity in [*expected, second_thread]
                )
            ),
            list[JsonValue](
                sorted(string_value(identity) for identity in workspace_posts)
            ),
        ),
    )
    fixture.receipt.equal(
        "complete workspace post traversal terminates",
        "PD1 pages",
        (None, workspace_cursor),
    )
    for actor in [fixture.helper, fixture.reader]:
        _ = fixture.client.accepted(
            "thread_join",
            {
                **thread_join_header(actor),
                "thread": {"project": fixture.project, "thread": thread},
                "role": "participant",
                "watch": False,
                "replace": None,
                "note": None,
            },
        )
    cursor = None
    participants: list[JsonValue] = []
    for _ in range(3):
        page = fixture.client.observe(
            "thread_participant_list",
            {
                "thread": {"project": fixture.project, "thread": thread},
                "page": {"limit": 1, "cursor": cursor},
            },
            "S-R2-F3/participant endpoint",
        )
        participants.extend(
            field(row, "identity")
            for row in array_value(field(page, "page", "records"))
        )
        next_cursor = field(page, "page", "nextCursor")
        cursor = string_value(next_cursor) if next_cursor is not None else None
        if cursor is None:
            break
    fixture.receipt.equal(
        "participant traversal returns the exact interval identities",
        "S-R2-F3",
        (
            list[JsonValue](
                sorted(
                    [
                        json.dumps(fixture.helper, sort_keys=True),
                        json.dumps(fixture.reader, sort_keys=True),
                    ]
                )
            ),
            list[JsonValue](
                sorted(json.dumps(actor, sort_keys=True) for actor in participants)
            ),
        ),
    )
    fixture.receipt.equal(
        "participant traversal terminates", "PD1 pages", (None, cursor)
    )
    thread_cursor: str | None = None
    thread_ids: list[str] = []
    for number in range(3):
        if number % 2 == 0:
            arguments_cli = [
                "thread",
                "list",
                "--workspace-id",
                fixture.workspace,
                "--reader",
                json.dumps(fixture.reader),
                "--limit",
                "1",
            ]
            if thread_cursor is not None:
                arguments_cli.extend(["--cursor", thread_cursor])
            roots = fixture.client.cli(
                arguments_cli, "complete workspace thread page via CLI"
            )
        else:
            roots = fixture.client.observe(
                "thread_list",
                {
                    "workspaceId": fixture.workspace,
                    "reader": fixture.reader,
                    "watchedOnly": False,
                    "page": {"limit": 1, "cursor": thread_cursor},
                },
                "RC-5/cross-transport thread roots",
            )
        thread_ids.extend(
            string_value(field(row, "threadId"))
            for row in array_value(field(roots, "page", "records"))
        )
        next_root = field(roots, "page", "nextCursor")
        thread_cursor = string_value(next_root) if next_root is not None else None
        if thread_cursor is None:
            break
    fixture.receipt.equal(
        "workspace roots traverse both projects exactly once",
        "RC-5/ThreadRef",
        (
            list[JsonValue](sorted([thread, second_thread])),
            list[JsonValue](sorted(thread_ids)),
        ),
    )
    fixture.receipt.equal(
        "complete workspace thread traversal terminates",
        "PD1 pages",
        (None, thread_cursor),
    )
    _ = accepted_decision(
        fixture.client.cli(
            [
                "thread",
                "watch",
                "--thread-id",
                f"{fixture.project}/{thread}",
                "--actor",
                json.dumps(fixture.reader),
            ],
            "qualified watch through real CLI",
        )
    )
    _ = accepted_decision(
        fixture.client.cli(
            [
                "thread",
                "watch",
                "--thread-id",
                f"{second_project}/{second_thread}",
                "--actor",
                json.dumps(fixture.reader),
            ],
            "watch independent second project through real CLI",
        )
    )
    watched = fixture.client.cli(
        [
            "thread",
            "list",
            "--workspace-id",
            fixture.workspace,
            "--reader",
            json.dumps(fixture.reader),
            "--watched-only",
        ],
        "read actual watch choice through real CLI",
    )
    fixture.receipt.equal(
        "watch selects exactly the named root",
        "preserved watch behavior",
        (
            list[JsonValue](sorted([thread, second_thread])),
            list[JsonValue](
                sorted(
                    string_value(field(row, "threadId"))
                    for row in array_value(field(watched, "page", "records"))
                )
            ),
        ),
    )
    _ = fixture.client.accepted(
        "thread_unwatch",
        {
            **write_header(fixture.reader),
            "thread": {"project": fixture.project, "thread": thread},
        },
    )
    unwatched = fixture.client.cli(
        [
            "thread",
            "list",
            "--workspace-id",
            fixture.workspace,
            "--reader",
            json.dumps(fixture.reader),
            "--watched-only",
        ],
        "read explicit opt-out through real CLI",
    )
    fixture.receipt.equal(
        "explicit unwatch removes that root",
        "preserved watch behavior",
        (
            [second_thread],
            [
                field(row, "threadId")
                for row in array_value(field(unwatched, "page", "records"))
            ],
        ),
    )
    _, corrupt_project = fixture.foreign_project()
    corrupt_thread = fixture.create_thread(corrupt_project)
    fixture.runtime.stop()
    fixture.corrupt_record(
        corrupt_project,
        record_reference("thread", corrupt_thread),
        {"state": "invalid-state"},
    )
    fixture.runtime.start()
    healthy_roots = fixture.client.observe(
        "thread_list",
        {
            "workspaceId": fixture.workspace,
            "reader": fixture.reader,
            "watchedOnly": False,
            "page": {"limit": 100, "cursor": None},
        },
        "preserved foreign-workspace corruption isolation",
    )
    fixture.receipt.equal(
        "foreign corrupt thread leaves exact healthy workspace roots readable",
        "RC-5/scope containment",
        (
            list[JsonValue](sorted([thread, second_thread])),
            list[JsonValue](
                sorted(
                    string_value(field(row, "threadId"))
                    for row in array_value(field(healthy_roots, "page", "records"))
                )
            ),
        ),
    )
    healthy_posts = fixture.client.observe(
        "post_list",
        {
            "scope": {
                "kind": "thread",
                "thread": {"project": fixture.project, "thread": thread},
            },
            "selection": {"kind": "latest"},
            "page": {"limit": 100, "cursor": None},
        },
        "preserved exact thread content after restart",
    )
    fixture.receipt.equal(
        "healthy post identities order and bodies survive a corrupt sibling restart",
        "RC-5/restart durability",
        (
            list[JsonValue](
                [[post["postId"], post["text"]] for post in reversed(posts)]
            )
            + [[thread, f"root {thread}"]],
            [
                [field(post, "postId"), field(post, "text")]
                for post in array_value(field(healthy_posts, "page", "records"))
            ],
        ),
    )
    corrupt_show = object_value(
        fixture.client.call(
            "thread_show",
            {
                "thread": {"project": corrupt_project, "thread": corrupt_thread},
                "reader": {"kind": "anonymous"},
            },
            "select corrupt foreign thread through real MCP",
        )
    )
    fixture.receipt.equal(
        "selecting the corrupt foreign thread refuses explicitly",
        "RC-5/scope containment",
        (True, corrupt_show.get("isError", False)),
    )
    diagnostic = object_value(corrupt_show.get("value"))
    fixture.receipt.equal(
        "corrupt thread refusal identifies the selected invalid stored record",
        "preserved full stored validation",
        (
            ["error", "inspection", "invalidRecord", corrupt_thread],
            [
                diagnostic.get("mcpResult"),
                diagnostic.get("stage"),
                diagnostic.get("kind"),
                field(diagnostic, "details", "resource", "threadId"),
            ],
        ),
    )
    _ = accepted_decision(
        fixture.client.cli(
            [
                "workspace",
                "archive",
                "--workspace-id",
                fixture.workspace,
                "--actor",
                json.dumps(fixture.owner),
            ],
            "archive owned workspace through real CLI",
        )
    )
    archived = fixture.client.observe(
        "workspace_show", {"workspaceId": fixture.workspace}, "FL-5/archive state"
    )
    fixture.receipt.equal(
        "archive remains honestly archiving without a real M4 holder",
        "FL-5/M4 boundary",
        (
            "archiving",
            field(archived, "workspace", "state"),
        ),
    )
