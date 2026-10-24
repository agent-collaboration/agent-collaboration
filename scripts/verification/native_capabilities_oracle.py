"""Pure oracles for J2 selection, J3 command lists and invocation, and J4 history.

Expected values come from the adopted design (Specification C2-C4, Verification Design
J2-J4) and from the controlled fixture: its native skill name, file and body-only
canary, and the arguments the driver typed. Nothing here reads the candidate's output to
decide what is expected. Each check raises ValueError for a wrong build and returns a
typed summary otherwise. J1 discovery oracles live in native_capabilities_catalogue_oracle.
"""

import typing as t
from collections import abc
from pathlib import Path

from pydantic import JsonValue

from scripts.verification.acp_metadata_observation import (
    ProviderItem,
    ProviderObservation,
    is_command_catalogue,
)
from scripts.verification.native_capabilities_models import (
    CommandCatalogueCheck,
    CommandCatalogueNotice,
    DisplayCheck,
    FixtureRecord,
    HistoryCheck,
    HistoryThread,
    OperationRow,
    RecordedInput,
    RecordedSelectionCheck,
    RejectedInputCheck,
    SelectedTurnCheck,
)
from scripts.verification.typed_observations import JsonObject

# Router-owned operation kinds that an explicit unloaded resume may add (C4/R7).
RESUME_OPERATION_KINDS = frozenset({"conversationLoad", "conversationResume"})


def same_path(left: str | Path, right: str | Path) -> bool:
    return Path(left).resolve() == Path(right).resolve()


def retained_items(observation: ProviderObservation) -> list[ProviderItem]:
    """Latest version of each item, in first-seen hub order."""
    items: dict[str, ProviderItem] = {}
    for entry in sorted(observation.events, key=lambda entry: entry.sequence):
        item = entry.event.item
        if entry.event.kind in {"itemStarted", "itemUpdated"} and item is not None:
            items[item.itemId] = item
    return list(items.values())


def turn_started_ids(observation: ProviderObservation) -> list[str]:
    return [
        entry.event.turnId
        for entry in observation.events
        if entry.event.kind == "turnStarted" and entry.event.turnId is not None
    ]


def require_complete(observation: ProviderObservation) -> None:
    if observation.endReason != "deadlineReached":
        raise ValueError(
            "observation incomplete: page, reset or disconnect needs reconciliation"
        )


def advertised_command_names(observation: ProviderObservation) -> list[str]:
    """Names from the provider's latest `available_commands_update` (wire1 replacement)."""
    catalogues = [
        item for item in retained_items(observation) if is_command_catalogue(item)
    ]
    if not catalogues or catalogues[-1].text is None:
        raise ValueError("provider advertised no command catalogue")
    return catalogue_names(catalogues[-1].text)


def catalogue_names(text: str) -> list[str]:
    notice = CommandCatalogueNotice.model_validate_json(text)
    return [command.name for command in notice.availableCommands]


def check_command_catalogues(
    observation: ProviderObservation, fixture: FixtureRecord
) -> CommandCatalogueCheck:
    """J3: successive full lists replace earlier state; the latest one is current (C3)."""
    require_complete(observation)
    catalogues = [
        item for item in retained_items(observation) if is_command_catalogue(item)
    ]
    if not catalogues:
        raise ValueError("no command catalogue retained")
    lists = [catalogue_names(item.text or "") for item in catalogues]
    current = lists[-1]
    if fixture.skill_name not in current:
        raise ValueError("current command list lacks the controlled native command")
    earlier = {name for names in lists[:-1] for name in names}
    removed = sorted(earlier - set(current))
    return CommandCatalogueCheck(
        catalogue_item_ids=[item.itemId for item in catalogues],
        current_names=current,
        removed_names=removed,
        # Identical successive lists leave replacement unobserved, never passed.
        replacement="observed" if removed else "unobserved",
    )


def expected_native_reply(canary: str, arguments: str) -> str:
    return f"{canary}::{arguments.strip()}"


def find_native_reply(
    items: abc.Iterable[ProviderItem], canary: str, arguments: str
) -> ProviderItem:
    expected = expected_native_reply(canary, arguments)
    replies = [
        item
        for item in items
        if item.kind.kind == "agentMessage" and (item.text or "").strip() == expected
    ]
    if len(replies) != 1:
        raise ValueError("argument-dependent native effect absent or repeated")
    return replies[0]


def require_canary_not_supplied(canary: str, supplied_texts: abc.Iterable[str]) -> None:
    if any(canary in text for text in supplied_texts):
        raise ValueError("canary was supplied as input; the effect proves nothing")


def new_operations(
    before: list[OperationRow], after: list[OperationRow]
) -> list[OperationRow]:
    known = {row.operation_id for row in before}
    return [row for row in after if row.operation_id not in known]


def check_native_invocation(
    after: ProviderObservation,
    fixture: FixtureRecord,
    arguments: str,
    supplied_texts: list[str],
    operations_before: list[OperationRow],
    operations_after: list[OperationRow],
) -> SelectedTurnCheck:
    """J2 positive and J3 Router invocation: one prompt reaches the native body effect."""
    require_complete(after)
    require_canary_not_supplied(fixture.canary, supplied_texts)
    prompts = [
        row
        for row in new_operations(operations_before, operations_after)
        if row.operation_kind == "conversationPrompt"
    ]
    if len(prompts) != 1:
        raise ValueError("expected exactly one Router prompt operation for the input")
    turns = turn_started_ids(after)
    if not turns:
        raise ValueError("no provider turn started for the input")
    reply = find_native_reply(retained_items(after), fixture.canary, arguments)
    return SelectedTurnCheck(
        turn_ids=turns,
        prompt_operation_ids=[row.operation_id for row in prompts],
        reply_item_id=reply.itemId,
    )


def check_rejected_input(
    after: ProviderObservation,
    fixture: FixtureRecord,
    operations_before: list[OperationRow],
    operations_after: list[OperationRow],
    client_error_seen: bool,
    ordinary_prompts: int = 0,
) -> RejectedInputCheck:
    """J2 negatives (R6, C2): a known-invalid selection fails explicitly and sends nothing.

    A silent drop is not an explicit rejection: the client must show an error (its
    wording is not asserted). `ordinary_prompts` counts unselected prompts the case
    itself submitted, such as the running turn a selected steer targets.
    """
    if not client_error_seen:
        raise ValueError("selection was dropped silently; no explicit rejection shown")
    require_complete(after)
    created = new_operations(operations_before, operations_after)
    prompts = [row for row in created if row.operation_kind == "conversationPrompt"]
    if len(prompts) != ordinary_prompts:
        raise ValueError("rejected selection reached a Router prompt operation")
    if len(turn_started_ids(after)) != ordinary_prompts:
        raise ValueError("rejected selection started a provider turn")
    for item in retained_items(after):
        text = item.text or ""
        if fixture.canary in text or fixture.second_canary in text:
            raise ValueError("rejected selection produced a native skill effect")
    return RejectedInputCheck(
        new_operation_ids=[row.operation_id for row in created],
        client_error_seen=client_error_seen,
    )


def user_text(item_content: list[str]) -> str:
    return "".join(item_content)


def check_selected_history(
    views: list[HistoryThread],
    session_id: str,
    fixture: FixtureRecord,
    arguments: str,
) -> HistoryCheck:
    """J4: read and resume show the canonical native command and the submitted task (C4).

    Only leading/trailing argument whitespace may differ; internal characters,
    dollar literals, UTF-8 and newlines must match exactly. The same Session may hold
    other invocations of the command, so each drive uses distinct arguments and the
    selected one is found by its exact submitted text.
    """
    if len(views) != 2 or any(view.id != session_id for view in views):
        raise ValueError("both read and resume views of the same thread are required")
    command = f"/{fixture.skill_name}"
    exact = f"{command} {arguments}"
    trimmed = f"{command} {arguments.strip()}"
    reply = expected_native_reply(fixture.canary, arguments)
    user_ids: list[str] = []
    reply_ids: list[str] = []
    trimmed_seen = False
    for view in views:
        items = [item for turn in view.turns for item in turn.items]
        users = [
            (item, text)
            for item in items
            if item.type == "userMessage"
            and (
                text := user_text(
                    [part.text or "" for part in item.content if part.type == "text"]
                )
            )
            in {exact, trimmed}
        ]
        if len(users) != 1:
            raise ValueError(
                "history lost, changed or repeated the native command and its task"
            )
        user, text = users[0]
        trimmed_seen = trimmed_seen or text != exact
        replies = [
            item
            for item in items
            if item.type == "agentMessage" and (item.text or "").strip() == reply
        ]
        if len(replies) != 1:
            raise ValueError("native effect missing or repeated in history")
        user_ids.append(user.id)
        reply_ids.append(replies[0].id)
    return HistoryCheck(
        thread_id=session_id,
        user_item_ids=user_ids,
        reply_item_ids=reply_ids,
        trimmed_outer_whitespace=trimmed_seen,
    )


def check_display_operations(
    before: list[OperationRow],
    after: list[OperationRow],
    display_events: ProviderObservation,
    *,
    unloaded_resume: bool,
) -> DisplayCheck:
    """J4: pure loaded display adds no Router operation or turn (R7).

    An explicit unloaded resume may add its existing load, never a prompt. These are
    Router-owned records, not native-provider counters.
    """
    require_complete(display_events)
    allowed = RESUME_OPERATION_KINDS if unloaded_resume else frozenset[str]()
    created = new_operations(before, after)
    if unexpected := sorted(
        {row.operation_kind for row in created if row.operation_kind not in allowed}
    ):
        raise ValueError(f"history display started Router operations: {unexpected}")
    if turn_started_ids(display_events):
        raise ValueError("history display started a provider turn")
    return DisplayCheck(
        allowed_kinds=sorted(allowed),
        new_operation_kinds=sorted(row.operation_kind for row in created),
    )


def recorded_user_turns(records: list[JsonObject]) -> list[list[JsonObject]]:
    """UserInput item lists from the client's `from_tui` op records, in order.

    Codex 0.163 logs the complete AppCommand before dispatch (app/thread_routing.rs
    submit_thread_op) and sends those items unchanged as turn/start or turn/steer input.
    """
    turns: list[list[JsonObject]] = []

    def visit(value: JsonValue) -> None:
        if isinstance(value, dict):
            items = value.get("items")
            if isinstance(items, list) and all(
                isinstance(item, dict) and "type" in item for item in items
            ):
                turns.append([t.cast(JsonObject, item) for item in items])
                return
            for child in value.values():
                visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)

    for record in records:
        if record.get("dir") == "from_tui" and record.get("kind") == "op":
            visit(record.get("payload"))
    return turns


def check_recorded_selection(
    records: list[JsonObject],
    fixture: FixtureRecord,
    expected_text: str,
    expected_skills: list[str],
    expected_turns: int = 1,
) -> RecordedSelectionCheck:
    """The real client sent the intended structured selection, so the case was reached.

    The last of `expected_turns` recorded user turns holds one Text equal to what the
    driver typed (outer whitespace aside) and one Skill per expected fixture skill, each
    with its real file origin. Earlier turns are the case's own ordinary input.
    """
    turns = recorded_user_turns(records)
    if len(turns) != expected_turns:
        raise ValueError(
            f"expected {expected_turns} recorded user turns, found {len(turns)}"
        )
    items = [RecordedInput.model_validate(item) for item in turns[-1]]
    texts = [item for item in items if item.type == "text"]
    skills = [item for item in items if item.type == "skill"]
    if len(texts) != 1 or len(skills) + 1 != len(items):
        raise ValueError("client input is not one Text plus Skill items")
    if (texts[0].text or "").strip() != expected_text.strip():
        raise ValueError("client Text differs from the typed input")
    files = {
        fixture.skill_name: fixture.skill_file,
        fixture.second_skill_name: fixture.second_skill_file,
    }
    if sorted(item.name or "" for item in skills) != sorted(expected_skills):
        raise ValueError("client Skill items differ from the picker selections")
    for item in skills:
        origin = files[item.name or ""]
        if item.path is None or not (
            same_path(item.path, origin) or same_path(item.path, origin.parent)
        ):
            raise ValueError("client Skill item carries a different origin")
    raw_text = next(item for item in turns[-1] if item.get("type") == "text")
    elements = raw_text.get("text_elements", raw_text.get("textElements", []))
    return RecordedSelectionCheck(
        item_types=[item.type for item in items],
        skill_names=[item.name or "" for item in skills],
        text=texts[0].text or "",
        text_element_count=len(elements) if isinstance(elements, list) else 0,
    )


def check_picker_refusal(screen: str, command_name: str) -> None:
    """J3 consumer limit: installed Codex refuses an arbitrary provider slash command."""
    if f"Unrecognized command '/{command_name}'" not in screen:
        raise ValueError("installed Codex did not show its local slash refusal")


def selected_steer_raced(before: list[OperationRow], after: list[OperationRow]) -> bool:
    """True when the selected input was admitted after the ordinary turn had ended: the
    client sent a fresh turn instead of a steer, so the case did not run."""
    prompts = [
        row
        for row in new_operations(before, after)
        if row.operation_kind == "conversationPrompt"
    ]
    if len(prompts) < 2 or prompts[0].terminal_at_ms is None:
        return False
    return prompts[1].admitted_at_ms > prompts[0].terminal_at_ms
