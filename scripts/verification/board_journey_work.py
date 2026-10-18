"""Observe recorded work, pending settlement and complete public page traversal."""

import datetime
import json

from .board_journey_fixture import (
    BoardFixture,
    evidence_reference,
    record_reference,
)
from .board_journey_record_fixture import (
    create_dependency_and_checked_subtask,
    feed_records,
    governing_definition,
)
from .board_journey_runtime import (
    project_request,
)
from .board_journey_values import (
    JsonObject,
    array_value,
    field,
    object_value,
    string_value,
)


def meaningful_views_and_pending_settlement(fixture: BoardFixture) -> None:
    fixture.grant_implementer()
    _ = fixture.client.accepted(
        "role_grant",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "roleGrant",
                "subject": {"kind": "workstream", "subject": fixture.workstream},
                "role": "verifier",
                "holder": fixture.reader,
                "model": None,
                "lineage": None,
                "expectedRevision": 1,
                "ownerGrant": None,
            },
        ),
    )
    task, blocker, foreign, subtask = create_dependency_and_checked_subtask(fixture)
    task_page = fixture.client.observe(
        "task_list",
        {
            "projectId": fixture.project,
            "page": {"limit": 1},
        },
        "task CLI creation/MCP page connection",
    )
    fixture.receipt.equal(
        "bounded task page returns the exact first created task",
        "ST-1/PD1 pages",
        (
            [task],
            [
                field(row, "record", "taskId")
                for row in array_value(task_page.get("records"))
            ],
        ),
    )
    fixture.receipt.equal(
        "bounded task page retains a continuation",
        "PD1 pages",
        (
            True,
            isinstance(task_page.get("nextCursor"), str),
        ),
    )
    definition = governing_definition(fixture, False)
    plan = "11" * 32
    _ = fixture.client.accepted(
        "project_publish_plan",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "projectPublishPlan",
                "hash": plan,
                "document": evidence_reference("task", task),
                "expectedRevision": 1,
            },
        ),
    )
    subject: JsonObject = {"kind": "task", "subject": task}
    handback = fixture.add_trail(
        {
            "subject": subject,
            "tags": ["handback"],
            "text": "implementation ready for assessment",
        },
        fixture.helper,
    )
    other_handback = fixture.add_trail(
        {
            "subject": {"kind": "task", "subject": foreign},
            "tags": ["handback"],
            "text": "unrelated subject handback",
        },
        fixture.helper,
    )
    question = fixture.add_trail(
        {
            "subject": subject,
            "tags": ["owner-question"],
            "text": "owner choice blocks task",
            "links": [evidence_reference("task", task)],
        }
    )
    _ = fixture.client.read(
        "feed_read",
        {
            "projectId": fixture.project,
            "reader": {"kind": "identified", "reader": fixture.reader},
            "after": {"epoch": 1, "position": 0},
            "limit": 100,
            "includeRedacted": True,
        },
    )
    fixture.runtime.stop()
    marks_before = fixture.offline_rows(
        fixture.project,
        "SELECT reader,scope,through_epoch,through_position FROM read_marks ORDER BY reader,scope",
    )
    fixture.runtime.start()
    own_card_request: JsonObject = {
        "projectId": fixture.project,
        "actor": fixture.helper,
        "subject": subject,
    }
    own = fixture.client.observe("view_card", own_card_request, "G4/S-R2-F4")
    cli_card = fixture.client.cli(
        [
            "view",
            "card",
            "--project-id",
            fixture.project,
            "--actor",
            json.dumps(fixture.helper),
            "--subject",
            json.dumps(subject),
        ],
        "real CLI non-lead card outcome",
    )
    fixture.receipt.equal(
        "CLI and MCP expose the identical complete non-lead card",
        "VW-1/CLI-MCP",
        (own, cli_card.get("record")),
    )
    fixture.receipt.equal(
        "non-lead card shows only applicable pending work",
        "VW-1/WT-6/S-R2-F4",
        (
            [
                definition,
                plan,
                {"kind": "implementer", "subject": fixture.workstream},
                [record_reference("workTrailRow", handback)],
                [record_reference("workTrailRow", question)],
                None,
            ],
            [
                own.get("processDefinition"),
                own.get("planRevision"),
                own.get("assignment"),
                own.get("pendingHandbacks"),
                own.get("pendingDecisions"),
                own.get("nextObligation"),
            ],
        ),
    )
    fixture.receipt.equal(
        "card recent trail is the selected subject's actual rows",
        "VW-1",
        (
            [question, handback],
            [field(row, "rowId") for row in array_value(own.get("recentTrail"))],
        ),
    )
    fixture.receipt.equal(
        "card counts include retained foreign work",
        "VW-1",
        (
            [3, 3, 1],
            [
                field(own, "counts", "task"),
                field(own, "counts", "workTrailRow"),
                field(own, "counts", "subtask"),
            ],
        ),
    )
    hook = fixture.client.observe("hook_card", own_card_request, "G4/VW-7")
    fixture.receipt.equal(
        "hook and ordinary non-lead cards project identical recorded work",
        "VW-7",
        (
            own,
            field(hook, "payload", "payload"),
        ),
    )
    work_cli = fixture.client.cli(
        ["view", "work", "--project-id", fixture.project], "real CLI work view outcome"
    )
    work = object_value(work_cli.get("record"))
    lanes = array_value(work.get("workstreams"))
    fixture.receipt.equal(
        "work view has the declared workstream pair",
        "VW-2",
        (
            [[fixture.workstream, fixture.helper, fixture.reader]],
            [
                [
                    field(lane, "id"),
                    field(lane, "implementer", "actor", "identity"),
                    field(lane, "verifier", "actor", "identity"),
                ]
                for lane in lanes
            ],
        ),
    )
    tasks = array_value(field(lanes[0], "tasks"))
    observed_ids = [field(row, "id") for row in tasks]
    fixture.receipt.equal(
        "work tasks are in dependency order with exact membership",
        "VW-2",
        (
            [blocker, task, foreign],
            observed_ids,
        ),
    )
    fixture.receipt.equal(
        "ready and blocked work reflect actual links",
        "VW-2",
        (
            [
                [blocker, foreign],
                [{"task": task, "blockers": [blocker], "byDropped": False}],
            ],
            [work.get("readyQueue"), work.get("blocked")],
        ),
    )
    selected_task = next(object_value(row) for row in tasks if field(row, "id") == task)
    fixture.receipt.equal(
        "work view shows actual subtask completion",
        "VW-2/TK-6",
        (
            [1, 1],
            [selected_task.get("subtasksChecked"), selected_task.get("subtasksTotal")],
        ),
    )
    owner_read_start = datetime.datetime.now(datetime.UTC)
    owner = fixture.client.observe("view_owner", {}, "G4/VW-3")
    cli_owner = fixture.client.cli(["view", "owner"], "real CLI owner view outcome")
    owner_read_end = datetime.datetime.now(datetime.UTC)
    cli_owner_record = object_value(cli_owner.get("record"))
    fixture.receipt.equal(
        "CLI and MCP expose the identical owner obligations",
        "VW-3/CLI-MCP",
        (
            {key: value for key, value in owner.items() if key != "asOf"},
            {key: value for key, value in cli_owner_record.items() if key != "asOf"},
        ),
    )
    timestamps = [
        datetime.datetime.fromisoformat(string_value(record.get("asOf")))
        for record in [owner, cli_owner_record]
    ]
    fixture.receipt.equal(
        "owner view timestamps are UTC instants within their observed read window",
        "VW-3/asOf",
        (
            True,
            all(
                stamp.utcoffset() == datetime.timedelta(0)
                and owner_read_start <= stamp <= owner_read_end
                for stamp in timestamps
            ),
        ),
    )
    fixture.receipt.equal(
        "owner view names the question and exactly what it blocks",
        "VW-3/WT-6",
        (
            [
                [
                    fixture.project,
                    record_reference("workTrailRow", question),
                    [record_reference("task", task)],
                ]
            ],
            [
                [field(row, "project"), field(row, "subject"), field(row, "blocks")]
                for row in array_value(owner.get("waiting"))
            ],
        ),
    )
    lead_role = next(
        role
        for role in feed_records(fixture, "roleAssignment")
        if role.get("role") == "lead"
    )
    load = fixture.client.observe(
        "view_lead_load", {"lead": lead_role["holder"]}, "G4/VW-4"
    )
    fixture.receipt.equal(
        "lead load retains obligations from both subjects",
        "VW-4/WT-6",
        (
            [handback, other_handback],
            [field(row, "subject", "id") for row in array_value(load.get("owed"))],
        ),
    )
    lead_card = fixture.client.observe(
        "view_card",
        {
            "projectId": fixture.project,
            "actor": fixture.owner,
            "subject": subject,
        },
        "G4/VW-1 lead obligation",
    )
    fixture.receipt.equal(
        "lead card keeps its project-wide oldest obligation",
        "VW-1/PD1 pending",
        (
            [
                {"kind": "lead", "subject": fixture.project},
                record_reference("workTrailRow", handback),
            ],
            [
                lead_card.get("assignment"),
                field(lead_card, "nextObligation", "subject"),
            ],
        ),
    )
    fixture.runtime.stop()
    marks_after = fixture.offline_rows(
        fixture.project,
        "SELECT reader,scope,through_epoch,through_position FROM read_marks ORDER BY reader,scope",
    )
    fixture.receipt.equal(
        "all summary reads leave actual reader marks unchanged",
        "VW-6",
        (
            [list(row) for row in marks_before],
            [list(row) for row in marks_after],
        ),
    )
    fixture.runtime.start()
    _ = fixture.client.accepted(
        "subtask_remove",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "subtaskRemove",
                "subtaskId": subtask,
            },
        ),
    )
    removed = fixture.client.observe(
        "subtask_show",
        {
            "projectId": fixture.project,
            "record": record_reference("subtask", subtask),
        },
        "TK-6/removed checklist readback",
    )
    fixture.receipt.equal(
        "removed checklist item remains explicitly readable as removed",
        "TK-6",
        (
            True,
            field(removed, "record", "removed"),
        ),
    )
    decision = fixture.add_trail(
        {
            "subject": subject,
            "tags": ["decision"],
            "text": "lead settles handback and owner question",
            "links": [
                evidence_reference("workTrailRow", handback),
                evidence_reference("workTrailRow", question),
            ],
        }
    )
    settled = fixture.client.observe(
        "view_card", own_card_request, "G4/WT-6 settlement"
    )
    fixture.receipt.equal(
        "linked lead decision settles both pending rows",
        "WT-6/VW-1",
        (
            [[], []],
            [settled.get("pendingHandbacks"), settled.get("pendingDecisions")],
        ),
    )
    fixture.receipt.equal(
        "settled owner question disappears from owner view",
        "WT-6/VW-3",
        (
            [],
            fixture.client.read("view_owner", {}).get("waiting"),
        ),
    )
    _ = fixture.client.accepted(
        "work_trail_redact",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "workTrailRedact",
                "rowId": decision,
                "redacted": True,
            },
        ),
    )
    revived = fixture.client.observe(
        "view_card", own_card_request, "G7/redaction changes settlement"
    )
    fixture.receipt.equal(
        "redacted settlement revives recorded pending work",
        "WT-6",
        (
            [
                [record_reference("workTrailRow", handback)],
                [record_reference("workTrailRow", question)],
            ],
            [revived.get("pendingHandbacks"), revived.get("pendingDecisions")],
        ),
    )
    register = fixture.client.read(
        "work_trail_show",
        {
            "projectId": fixture.project,
            "reader": {"kind": "anonymous"},
            "selection": {
                "kind": "decisions",
                "after": None,
                "before": None,
                "rangeTop": None,
            },
            "limit": 100,
            "subject": None,
            "tags": [],
            "includeRedacted": False,
        },
    )
    fixture.receipt.equal(
        "decision register leaves the redacted row out",
        "WT-3/WT-4",
        (
            [],
            field(register, "value", "page", "rows"),
        ),
    )
    feed = fixture.feed()
    entries = [
        entry
        for entry in array_value(field(feed, "value", "entries"))
        if field(entry, "record", "id") == decision
    ]
    fixture.receipt.equal(
        "project feed keeps redacted position without content",
        "RC-5/WT-4",
        (
            ["redacted"],
            [field(entry, "content", "kind") for entry in entries],
        ),
    )
    _ = fixture.client.accepted(
        "work_trail_redact",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "workTrailRedact",
                "rowId": decision,
                "redacted": False,
            },
        ),
    )
    restored = fixture.client.read(
        "work_trail_show",
        {
            "projectId": fixture.project,
            "reader": {"kind": "anonymous"},
            "selection": {
                "kind": "decisions",
                "after": None,
                "before": None,
                "rangeTop": None,
            },
            "limit": 100,
            "subject": None,
            "tags": [],
            "includeRedacted": False,
        },
    )
    fixture.receipt.equal(
        "unredaction restores exactly the same decision row",
        "WT-1/WT-3",
        (
            [decision],
            [
                field(row, "rowId")
                for row in array_value(field(restored, "value", "page", "rows"))
            ],
        ),
    )
    next_handback = fixture.add_trail(
        {
            "subject": subject,
            "tags": ["handback"],
            "text": "second actual owed handback",
        },
        fixture.helper,
    )
    before_fault = fixture.client.read("view_card", own_card_request)
    fixture.receipt.equal(
        "fault control contains positive applicable pending work",
        "S-R2-F4",
        (
            [record_reference("workTrailRow", next_handback)],
            before_fault.get("pendingHandbacks"),
        ),
    )
    fixture.runtime.stop()
    fixture.corrupt_record(
        fixture.project,
        record_reference("workTrailRow", other_handback),
        {
            "links": [{"kind": "invalid-evidence-variant"}],
        },
    )
    fixture.runtime.start()
    healthy = fixture.client.observe(
        "view_card", own_card_request, "S-R2-F4/foreign metadata isolation"
    )
    fixture.receipt.equal(
        "foreign pending corruption leaves the complete healthy card unchanged",
        "S-R2-F4",
        (
            before_fault,
            healthy,
        ),
    )
    healthy_hook = fixture.client.observe(
        "hook_card", own_card_request, "S-R2-F4/hook isolation"
    )
    fixture.receipt.equal(
        "hook sees the same healthy recorded card",
        "S-R2-F4/VW-7",
        (
            healthy,
            field(healthy_hook, "payload", "payload"),
        ),
    )
    selected_corrupt = object_value(
        fixture.client.call(
            "view_card",
            {
                "projectId": fixture.project,
                "actor": fixture.helper,
                "subject": {"kind": "task", "subject": foreign},
            },
            "selected foreign pending metadata still refuses",
        )
    )
    fixture.receipt.equal(
        "selecting the malformed pending item refuses it",
        "S-R2-F4/full validation",
        (
            True,
            selected_corrupt.get("isError"),
        ),
    )
