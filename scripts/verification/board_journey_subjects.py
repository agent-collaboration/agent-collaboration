"""Observe required subjects, authority, read projection and recorded re-filing."""

import json

from .board_journey_fixture import (
    BoardFixture,
    accepted_decision,
    human_actor,
    project_thread_subject,
)
from .board_journey_record_fixture import feed_records
from .board_journey_roles import RoleScenario, expect_reasons, role_required
from .board_journey_runtime import project_request, write_header
from .board_journey_values import (
    JsonObject,
    JsonValue,
    field,
    fixture_identity,
    object_value,
    run_command,
)


def create_subject_request(
    fixture: BoardFixture, actor: JsonObject, about: JsonObject
) -> JsonObject:
    post = fixture_identity()
    return {
        **write_header(actor),
        "observedEpoch": 1,
        "postId": post,
        "projectId": fixture.project,
        "about": about,
        "text": f"M2b subject root {post}",
        "references": [],
        "addressees": [],
        "role": None,
        "watch": False,
    }


def cli_create_subject(
    fixture: BoardFixture, request: JsonObject, argument: str
) -> JsonObject:
    path = fixture.runtime.root / f"subject-{request['postId']}.txt"
    _ = path.write_text(str(request["text"]), encoding="utf-8")
    reply = fixture.client.cli(
        [
            "thread",
            "create",
            "--project-id",
            fixture.project,
            "--post-id",
            str(request["postId"]),
            "--request-id",
            str(request["requestId"]),
            "--observed-epoch",
            "1",
            "--about",
            argument,
            "--actor",
            json.dumps(request["actor"]),
            "--no-watch",
            "--text-file",
            str(path),
        ],
        "M2b subject through the real CLI parser and Unix listener",
    )
    mcp = fixture.client.read("thread_create", request)
    fixture.receipt.equal(
        "CLI subject create and identical MCP replay return the same decision",
        "V8/TH-1/RC-4",
        (field(reply, "record", "result"), mcp.get("result")),
    )
    _ = accepted_decision(mcp)
    return mcp


def subject_show(fixture: BoardFixture, thread: str) -> JsonObject:
    mcp = fixture.client.read(
        "thread_show",
        {
            "thread": {"project": fixture.project, "thread": thread},
            "reader": {"kind": "anonymous"},
        },
    )
    cli = fixture.client.cli(
        ["thread", "show", "--thread-id", f"{fixture.project}/{thread}"],
        "M2b anonymous subject projection through Unix CLI",
    )
    fixture.receipt.equal(
        "both listeners return the full identical anonymous thread and subject projection",
        "V8/TH-1",
        (object_value(cli["record"]), mcp),
    )
    return mcp


def cli_refile_subject(fixture: BoardFixture, request: JsonObject) -> JsonObject:
    about = object_value(request["about"])
    reply = fixture.client.cli(
        [
            "thread",
            "refile",
            "--thread-id",
            f"{fixture.project}/{field(request, 'thread', 'thread')}",
            "--request-id",
            str(request["requestId"]),
            "--observed-epoch",
            "1",
            "--actor",
            json.dumps(request["actor"]),
            "--about",
            f"workstream:{about['subject']}",
        ],
        "M2b refile through real CLI parser and Unix listener",
    )
    mcp = fixture.client.read("thread_refile", request)
    fixture.receipt.equal(
        "CLI refile and identical TCP MCP replay return the same structured result",
        "V8/TH-3/RC-4",
        (field(reply, "record", "result"), mcp.get("result")),
    )
    _ = accepted_decision(mcp)
    return mcp


def cli_root_refusals(fixture: BoardFixture, actor: JsonObject) -> None:
    text_file = fixture.runtime.root / "refused-root.txt"
    _ = text_file.write_text("M2b forbidden subjectless root", encoding="utf-8")
    for mode in ["missing-about", "removed-project-post"]:
        post = fixture_identity()
        arguments = (
            [
                "thread",
                "create",
                "--project-id",
                fixture.project,
                "--post-id",
                post,
                "--actor",
                json.dumps(actor),
                "--no-watch",
                "--text-file",
                str(text_file),
            ]
            if mode == "missing-about"
            else [
                "post",
                "post",
                "--placement",
                "project",
                "--project-id",
                fixture.project,
                "--post-id",
                post,
                "--actor",
                json.dumps(actor),
                "--text-file",
                str(text_file),
            ]
        )
        command = [
            str(fixture.runtime.target.cli),
            *arguments,
            "--service-directory",
            str(fixture.runtime.directory),
            "--json",
        ]
        result = run_command(command, fixture.runtime.target.source_root)
        fixture.receipt.record(
            {
                "kind": "call",
                "surface": "cli",
                "label": mode,
                "command": list[JsonValue](command),
                "exit": result.returncode,
                "raw": {"stdout": result.stdout, "stderr": result.stderr},
            }
        )
        fixture.receipt.equal(
            f"CLI refuses {mode} at its public parser boundary",
            "TH-1/PD1 sole-root-entry/V8",
            (2, result.returncode),
        )
        absent = object_value(
            fixture.client.call(
                "post_show", {"postId": post}, f"{mode} created no root"
            )
        )
        fixture.receipt.equal(
            f"CLI {mode} refusal stores no subjectless root",
            "TH-1/RC-12",
            (
                [True, "resourceNotFound"],
                [absent.get("isError"), field(absent, "value", "kind")],
            ),
        )


def referenced_subjects_through_cli(fixture: BoardFixture) -> None:
    task_title = "M2b task subject title"
    task = fixture.create_task(task_title)
    row, adr = fixture_identity(), fixture_identity()
    row_title, adr_title = "M2b row first line", "M2b workspace decision"
    _ = fixture.client.accepted(
        "work_trail_add",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "workTrailAdd",
                "rowId": row,
                "subject": {"kind": "project"},
                "text": f"{row_title}\nM2b row detail",
                "tags": [],
                "links": [],
            },
        ),
    )
    _ = fixture.client.accepted(
        "adr_create",
        {
            **write_header(fixture.owner),
            "workspaceId": fixture.workspace,
            "adrId": adr,
            "title": adr_title,
            "text": "M2b decision content",
            "supersedes": None,
            "madeIn": None,
            "repositoryFiles": [],
        },
    )
    for cli_kind, wire_kind, identity, title in [
        ("task", "task", task, task_title),
        ("row", "workTrailRow", row, row_title),
        ("adr", "workspaceAdr", adr, adr_title),
    ]:
        about: JsonObject = {"kind": wire_kind, "subject": identity}
        request = create_subject_request(
            fixture, human_actor(f"subject-{cli_kind}-outsider"), about
        )
        _ = cli_create_subject(fixture, request, f"{cli_kind}:{identity}")
        shown = subject_show(fixture, str(request["postId"]))
        fixture.receipt.equal(
            f"CLI {cli_kind} subject names the exact record and computed title for a non-holder",
            "TH-1/TH-2/V8",
            (
                [
                    about,
                    {"about": about, "title": {"kind": "shown", "title": title}},
                    False,
                ],
                [shown.get("about"), shown.get("subject"), shown.get("imported")],
            ),
        )


def required_subjects_and_recorded_refile(fixture: BoardFixture) -> None:
    roles = RoleScenario(fixture)
    advisor = human_actor("subject-advisor")
    outsider = human_actor("subject-outsider")
    _ = roles.grant("advisor", advisor, scope="project")
    line = "M2b named project decision"
    initial = project_thread_subject(line)
    request = create_subject_request(fixture, advisor, initial)
    _ = cli_create_subject(fixture, request, f"project:{line}")
    thread = str(request["postId"])
    shown = subject_show(fixture, thread)
    fixture.receipt.equal(
        "role holder opens an explicitly named project-wide thread",
        "TH-1/TH-2/U65",
        ([initial, False], [shown.get("about"), shown.get("imported")]),
    )
    fixture.receipt.equal(
        "project subject title is shown through the actual read projection",
        "TH-1/ThreadSubjectView",
        (
            {"about": initial, "title": {"kind": "shown", "title": line}},
            shown.get("subject"),
        ),
    )
    cli_root_refusals(fixture, advisor)
    _ = fixture.client.accepted(
        "thread_post", fixture.reply_request(thread, "M2b existing-thread post control")
    )
    removed_post = fixture_identity()
    old_root: JsonObject = {
        **write_header(advisor),
        "observedEpoch": 1,
        "postId": removed_post,
        "placement": {"kind": "project", "projectId": fixture.project},
        "text": "This removed root-creation entry must not be admitted",
        "references": [],
        "addressees": [],
    }
    rejected_root = object_value(
        fixture.client.call(
            "thread_post", old_root, "removed project-target post refuses"
        )
    )
    fixture.receipt.equal(
        "thread_post cannot bypass required subject and authority through a project target",
        "TH-1/TH-2/PD1 root-entry ruling 98d90f53",
        (
            [True, "validation"],
            [rejected_root.get("isError"), field(rejected_root, "value", "stage")],
        ),
    )
    absent = object_value(
        fixture.client.call(
            "post_show", {"postId": removed_post}, "refused root was not stored"
        )
    )
    fixture.receipt.equal(
        "removed project-post refusal creates no root post",
        "TH-1/RC-12",
        (
            [True, "resourceNotFound"],
            [absent.get("isError"), field(absent, "value", "kind")],
        ),
    )
    refused = fixture.client.read(
        "thread_create", create_subject_request(fixture, outsider, initial)
    )
    expect_reasons(fixture, refused, [role_required({"kind": "anyProjectRole"})])
    open_subject: JsonObject = {"kind": "workstream", "subject": fixture.workstream}
    open_request = create_subject_request(fixture, outsider, open_subject)
    _ = cli_create_subject(fixture, open_request, f"workstream:{fixture.workstream}")
    fixture.receipt.equal(
        "the same non-holder can open a referenced-record thread",
        "TH-2/record subjects remain open to every actor",
        (
            open_subject,
            subject_show(fixture, str(open_request["postId"])).get("about"),
        ),
    )
    referenced_subjects_through_cli(fixture)
    missing = create_subject_request(fixture, outsider, initial)
    del missing["about"]
    response = object_value(
        fixture.client.call("thread_create", missing, "missing about is refused")
    )
    fixture.receipt.equal(
        "missing about is refused at the public boundary",
        "TH-1/hard cutover",
        (
            [True, "invalidShape", "about"],
            [
                response.get("isError"),
                field(response, "value", "kind"),
                field(response, "value", "details", "field"),
            ],
        ),
    )
    target: JsonObject = {"kind": "workstream", "subject": fixture.workstream}
    refile: JsonObject = {
        **write_header(advisor),
        "observedEpoch": 1,
        "thread": {"project": fixture.project, "thread": thread},
        "about": target,
    }
    before = feed_records(fixture, "threadRefiled")
    reply = cli_refile_subject(fixture, refile)
    changed = accepted_decision(reply)
    fixture.receipt.equal(
        "refile changes the recorded subject",
        "TH-3/RC-12",
        ("changed", changed.get("change")),
    )
    events = feed_records(fixture, "threadRefiled")
    fixture.receipt.equal(
        "refile appends exactly one event with the actual previous and next subjects",
        "TH-3/RC-5",
        (
            [len(before) + 1, [[thread, initial, target]]],
            [
                len(events),
                [[row.get("thread"), row.get("from"), row.get("to")] for row in events],
            ],
        ),
    )
    fixture.receipt.equal(
        "refiled thread points at its current workstream",
        "TH-1/TH-3",
        (target, subject_show(fixture, thread).get("about")),
    )
    fixture.receipt.equal(
        "refiled thread displays the referenced workstream name",
        "TH-1/ThreadSubjectView",
        (
            {"about": target, "title": {"kind": "shown", "title": "verification lane"}},
            subject_show(fixture, thread).get("subject"),
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
                "name": "Renamed verification lane",
                "expectedRevision": 0,
            },
        ),
    )
    fixture.receipt.equal(
        "subject title follows a later rename rather than a stored copy",
        "TH-1/ThreadSubjectView",
        (
            {
                "about": target,
                "title": {"kind": "shown", "title": "Renamed verification lane"},
            },
            subject_show(fixture, thread).get("subject"),
        ),
    )
    replay = fixture.client.read("thread_refile", refile)
    fixture.receipt.equal(
        "keyed refile replay repeats its exact decision and no event",
        "RC-4/TH-3",
        (
            [changed, list[JsonValue](events)],
            [
                accepted_decision(replay),
                list[JsonValue](feed_records(fixture, "threadRefiled")),
            ],
        ),
    )
    no_op = fixture.client.accepted(
        "thread_refile", {**refile, "requestId": fixture_identity()}
    )
    fixture.receipt.equal(
        "refiling to the same subject is AlreadySo without a history record",
        "TH-3",
        (
            ["alreadySo", list[JsonValue](events)],
            [
                accepted_decision(no_op).get("change"),
                list[JsonValue](feed_records(fixture, "threadRefiled")),
            ],
        ),
    )
    denied = fixture.client.read(
        "thread_refile",
        {**refile, **write_header(outsider), "about": initial},
    )
    expect_reasons(fixture, denied, [role_required({"kind": "anyProjectRole"})])
    fixture.receipt.equal(
        "unauthorized refile preserves the current subject and event history",
        "TH-3",
        (
            [target, list[JsonValue](events)],
            [
                subject_show(fixture, thread).get("about"),
                list[JsonValue](feed_records(fixture, "threadRefiled")),
            ],
        ),
    )
