"""Publish governing record fixtures and inspect their complete public feed."""

import hashlib
import json

from .board_journey_fixture import BoardFixture, accepted_decision
from .board_journey_runtime import (
    project_request,
    write_header,
)
from .board_journey_values import (
    JsonObject,
    array_value,
    field,
    fixture_identity,
    object_value,
)


def feed_records(fixture: BoardFixture, kind: str) -> list[JsonObject]:
    feed = fixture.feed()
    fixture.receipt.equal(
        "fixture feed fits its explicit observed range",
        "RC-5",
        (
            field(feed, "value", "highWater"),
            field(feed, "value", "coveredThrough"),
        ),
    )
    return [
        object_value(field(entry, "content", "record", "record"))
        for entry in array_value(field(feed, "value", "entries"))
        if field(entry, "content", "record", "kind") == kind
    ]


def governing_definition(fixture: BoardFixture, guarded: bool) -> JsonObject:
    source = json.dumps(
        {
            "initialStatus": "draft",
            "edges": [
                {
                    "name": "finish",
                    "from": "draft",
                    "to": "done",
                    "requiredRole": "lead",
                    "guards": [{"name": "proof", "evaluator": fixture.owner}]
                    if guarded
                    else [],
                }
            ],
        },
        separators=(",", ":"),
    )
    _ = fixture.client.accepted(
        "process_register",
        {
            **write_header(fixture.owner),
            "workspaceId": fixture.workspace,
            "name": "verification-flow-guarded"
            if guarded
            else "verification-flow-basic",
            "version": "v1",
            "source": source,
        },
    )
    definition: JsonObject = {
        "name": "verification-flow-guarded" if guarded else "verification-flow-basic",
        "version": "v1",
        "sourceHash": hashlib.sha256(source.encode()).hexdigest(),
    }
    _ = fixture.client.accepted(
        "project_pin_definition",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "projectPinDefinition",
                "definition": definition,
                "expectedRevision": 0,
            },
        ),
    )
    return definition


def create_dependency_and_checked_subtask(
    fixture: BoardFixture,
) -> tuple[str, str, str, str]:
    """Create the dependent, prerequisite, unrelated task and checked item."""
    task = fixture.create_task("dependent")
    blocker = fixture.create_task("prerequisite")
    foreign = fixture.create_task("other subject")
    _ = fixture.client.accepted(
        "task_link",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "taskLink",
                "link": {
                    "from": task,
                    "kind": "blockedBy",
                    "to": {"project": fixture.project, "task": blocker},
                },
            },
        ),
    )
    subtask = fixture_identity()
    _ = accepted_decision(
        fixture.client.cli(
            [
                "subtask",
                "add",
                "--project-id",
                fixture.project,
                "--subtask-id",
                subtask,
                "--task-id",
                task,
                "--text",
                "verified checklist item",
                "--place",
                "80",
                "--actor",
                json.dumps(fixture.owner),
            ],
            "add checklist item through real CLI",
        )
    )
    _ = fixture.client.accepted(
        "subtask_check",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "subtaskCheck",
                "subtaskId": subtask,
                "checked": True,
            },
        ),
    )
    return task, blocker, foreign, subtask
