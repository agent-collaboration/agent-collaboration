"""Board notification fixtures and actual-recipient/frozen-item correlation."""

import time

from .board_journey_runtime import JourneyReceipt, project_request, write_header
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
    object_value,
)
from .cross_machine_cases import await_accepted
from .cross_machine_lifecycle import Installation
from .cross_machine_recipients import Recipient, begin_observation, observe_action


def notification_project(source: Installation, context: JsonObject) -> str:
    installation = source
    actor = object_value(context["actor"])
    board, project = fixture_identity(), fixture_identity()
    _ = await_accepted(
        installation,
        "board_create",
        {
            **write_header(actor),
            "workspaceId": context["workspace"],
            "boardId": board,
            "name": board,
            "description": "Private OB4 notification fixture",
            "kind": {"kind": "area"},
        },
    )
    _ = await_accepted(
        installation,
        "project_create",
        {
            **write_header(actor),
            "boardId": board,
            "projectId": project,
            "title": "OB4 actual recipient",
            "description": "Synthetic disposable fixture",
        },
    )
    _ = await_accepted(
        installation,
        "role_grant",
        project_request(
            actor,
            project,
            {
                "kind": "roleGrant",
                "subject": {"kind": "project", "subject": project},
                "role": "lead",
                "holder": actor,
                "model": None,
                "lineage": None,
                "expectedRevision": 0,
                "ownerGrant": None,
            },
        ),
    )
    return project


def contains_identity(value: JsonValue, expected: JsonObject) -> bool:
    if value == expected:
        return True
    if isinstance(value, dict):
        return any(contains_identity(child, expected) for child in value.values())
    if isinstance(value, list):
        return any(contains_identity(child, expected) for child in value)
    return False


def frozen_intents(
    source: Installation, project: str, post: str, count: int
) -> list[JsonObject]:
    deadline = time.monotonic() + 30
    while True:
        page = source.read(
            "intent_list",
            {
                "projectId": project,
                "page": {"limit": 100, "cursor": None},
            },
        )
        records = [
            object_value(field(record, "record"))
            for record in array_value(page.get("records"))
            if field(record, "kind") == "notifyIntent"
            and field(record, "record", "cause", "id") == post
        ]
        if len(records) == count and all(
            field(record, "state", "kind") == "handedOff" for record in records
        ):
            return records
        if time.monotonic() >= deadline:
            raise JourneyFailure(
                "public intents did not freeze exactly the declared recipients"
            )
        time.sleep(0.2)


def notification(
    source: Recipient,
    recipients: list[Recipient],
    project: str,
    context: JsonObject,
    receipt: JourneyReceipt,
) -> list[str]:
    source.confirm_model()
    baselines = [begin_observation(recipient) for recipient in recipients]
    post = fixture_identity()
    actor = object_value(context["actor"])
    request: JsonObject = {
        **write_header(actor),
        "postId": post,
        "projectId": project,
        "about": {"kind": "project", "subject": {"line": "OB4 real recipient arrival"}},
        "text": "Labelled disposable test automation. Read this notification and acknowledge it; change nothing and delegate nothing.",
        "references": [],
        "addressees": [
            {
                "kind": "actor",
                "recipient": {"kind": "session", "session": recipient.target},
            }
            for recipient in recipients
        ],
        "role": None,
        "watch": False,
    }
    decision = await_accepted(source.installation, "thread_create", request)
    frozen = frozen_intents(source.installation, project, post, len(recipients))
    ids: list[str] = []
    for recipient, baseline in zip(recipients, baselines, strict=True):
        matches = [
            record
            for record in frozen
            if contains_identity(
                record.get("recipient"),
                {
                    "kind": "session",
                    "session": recipient.target,
                },
            )
        ]
        if len(matches) != 1:
            raise JourneyFailure(
                "frozen notification recipient does not match its actual SessionRef"
            )
        item = field(matches[0], "state", "item")
        if not isinstance(item, str):
            raise JourneyFailure("frozen handoff lacks the public item identity")
        ids.append(item)
        _ = observe_action(
            recipient,
            receipt,
            lambda marker=item: {"marker": marker, "post": post, "decision": decision},
            notification=True,
            baseline=baseline,
        )
    receipt.equal(
        "one cause freezes a distinct item per recipient",
        "MS-2 / NI-4",
        (len(ids), len(set(ids))),
    )
    replay = source.installation.read("thread_create", request)
    receipt.equal(
        "notification cause replays its exact decision",
        "RC-4 / NI-4",
        (decision["result"], replay["result"]),
    )
    receipt.equal(
        "notification retries preserve the frozen item IDs",
        "MS-2",
        (
            list[JsonValue](sorted(ids)),
            list[JsonValue](
                sorted(
                    str(field(record, "state", "item"))
                    for record in frozen_intents(
                        source.installation, project, post, len(recipients)
                    )
                )
            ),
        ),
    )
    receipt.record(
        {
            "kind": "notificationCause",
            "post": post,
            "project": project,
            "itemIds": list[JsonValue](ids),
            "intents": list[JsonValue](frozen),
        }
    )
    return ids


def freeze_notification(
    source: Installation,
    target: JsonObject,
    project: str,
    context: JsonObject,
    *,
    post: str | None = None,
) -> str:
    """Commit a fresh cause and read its original item without waiting for arrival."""
    actor = object_value(context["actor"])
    post = post if post is not None else fixture_identity()
    _ = await_accepted(
        source,
        "thread_create",
        {
            **write_header(actor),
            "projectId": project,
            "postId": post,
            "about": {"kind": "project", "subject": {"line": "OB4 retained original"}},
            "text": "Labelled disposable test automation. Read this notification and acknowledge it; change nothing and delegate nothing.",
            "references": [],
            "addressees": [
                {"kind": "actor", "recipient": {"kind": "session", "session": target}}
            ],
            "role": None,
            "watch": False,
        },
    )
    item = field(frozen_intents(source, project, post, 1)[0], "state", "item")
    if not isinstance(item, str):
        raise JourneyFailure("public frozen original lacks its item identity")
    return item
