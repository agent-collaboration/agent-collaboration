"""Replay original frozen mail through the existing authenticated Session edge.

Only a closed owning Host permits frozen-body inspection. No central row is
edited and no recipient outcome is manufactured. Terminal retirement uses the
normal SDK delivery operation; actual arrival remains a separate observation.
"""

import contextlib
import hashlib
import os
import time
import uuid
from collections.abc import Generator, Mapping
from dataclasses import dataclass

from .board_journey_runtime import JourneyReceipt, project_request
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    array_value,
    field,
    fixture_identity,
    json_value,
    object_value,
    string_value,
)
from .board_store_readback import read_stopped_store
from .board_upgrade_external_transport import MachineReadback
from .cross_machine_cases import await_accepted
from .cross_machine_lifecycle import Installation
from .cross_machine_notifications import freeze_notification, notification_project
from .cross_machine_plan import CredentialNames
from .cross_machine_recipients import (
    Recipient,
    begin_observation,
    input_observed,
    observe_action,
    validate_observation,
)
from .machine_credentials import normalize_machine_credential


@dataclass(frozen=True)
class FrozenOriginal:
    project: str
    epoch: int
    item: str
    sequence: int
    envelope: JsonObject

    @property
    def mailbox(self) -> str:
        return string_value(field(self.envelope, "value", "recipient", "peer"))

    @property
    def digest(self) -> str:
        return string_value(field(self.envelope, "value", "digest"))


def frozen_original(
    source: Installation, project: str, item: str, epoch: int = 1
) -> FrozenOriginal:
    if source.plan.ssh_host is not None:
        raise JourneyFailure("stopped frozen-body readback needs the local sender")
    if source.child is not None:
        raise JourneyFailure("frozen-body inspection requires owning Host exit")
    if str(uuid.UUID(project)) != project or epoch < 1:
        raise JourneyFailure("invalid declared project/epoch")
    database = (
        source.plan.task_directory / "service/projects" / project / f"e{epoch}.sqlite"
    )
    snapshot = read_stopped_store(database)
    table = snapshot.tables.get("mail_handoffs")
    matches = (
        []
        if table is None
        else [row for row in table.rows if row.get("item_id") == item]
    )
    if len(matches) != 1:
        raise JourneyFailure("exact frozen original is absent or duplicated")
    row = matches[0]
    stored = object_value(json_value(string_value(row["item"])))
    envelope = object_value(stored.get("value"))
    if (
        stored.get("kind") != "delivery"
        or envelope.get("kind") != "item"
        or field(envelope, "value", "id") != item
        or field(envelope, "value", "sender") != {"kind": "project", "project": project}
        or not isinstance(row.get("handoff_seq"), int)
    ):
        raise JourneyFailure("stored handoff is not the exact original project mail")
    source.receipt.record(
        {
            "kind": "stoppedFrozenOriginal",
            "project": project,
            "item": item,
            "row": row,
            "storeDigests": dict(snapshot.file_digests),
        }
    )
    return FrozenOriginal(project, epoch, item, int(str(row["handoff_seq"])), envelope)


def sender_session_headers(
    names: CredentialNames, environment: Mapping[str, str]
) -> dict[str, str]:
    headers: dict[str, str] = {}
    for header, name in [
        ("Authorization", names.machine),
        ("CF-Access-Client-Id", names.access_id),
        ("CF-Access-Client-Secret", names.access_secret),
    ]:
        value = environment.get(name, "")
        if not value or "\r" in value or "\n" in value:
            raise JourneyFailure("declared sender credential environment unavailable")
        headers[header] = (
            "Bearer " + normalize_machine_credential(value)
            if header == "Authorization"
            else value
        )
    return headers


@contextlib.contextmanager
def open_sender_session(source: Installation) -> Generator[MachineReadback, None, None]:
    if source.child is not None:
        raise JourneyFailure("normal probe must not replace a live Host connection")
    session = source.plan.session
    if session is None or source.plan.ssh_host is not None:
        raise JourneyFailure("normal replay needs the local enrolled sender")
    names = session.credentials
    environment = source.environment if source.environment is not None else os.environ
    headers = sender_session_headers(names, environment)
    path = source.plan.task_directory / "service/machine-session/state.json"
    metadata = path.lstat()
    if (
        path.resolve(strict=True) != path
        or not path.is_file()
        or metadata.st_uid != os.getuid()
        or metadata.st_mode & 0o077
        or metadata.st_size > 4096
        or (source.plan.task_directory / "service/service.json").exists()
    ):
        raise JourneyFailure(
            "saved machine session is not a private stopped-owner file"
        )
    contents = path.read_bytes()
    saved = object_value(json_value(contents))
    session_id = string_value(saved.get("sessionId"))
    if str(uuid.UUID(session_id)) != session_id:
        raise JourneyFailure("saved machine session identity is not canonical")
    source.receipt.record(
        {
            "kind": "retainedSenderSession",
            "sessionId": session_id,
            "stateSha256": hashlib.sha256(contents).hexdigest(),
            "noticeCursor": saved.get("noticeCursor"),
        }
    )
    client = MachineReadback(session.endpoint, headers, session_id=session_id)
    try:
        yield client
    finally:
        client.close()
        source.receipt.equal(
            "diagnostic replay leaves saved session and notice cursor untouched",
            "OB4 retained session / Lead01a167a1",
            (
                hashlib.sha256(contents).hexdigest(),
                hashlib.sha256(path.read_bytes()).hexdigest(),
            ),
        )


def delivery_operation(kind: str, original: FrozenOriginal) -> JsonObject:
    return {
        "kind": "delivery",
        "value": {
            "kind": kind,
            "value": {
                "call": {"operation": fixture_identity(), "step": "ob4-" + kind},
                "mailbox": original.mailbox,
                "items": [original.item],
            },
        },
    }


def query_original(session: MachineReadback, original: FrozenOriginal) -> JsonObject:
    answer = session.operation(delivery_operation("query", original))
    if (answer.get("kind"), field(answer, "value", "kind")) != ("delivery", "query"):
        raise JourneyFailure("normal original-mail query was not answered")
    statuses = field(answer, "value", "value", "statuses")
    if not isinstance(statuses, list) or len(statuses) != 1:
        raise JourneyFailure("normal query did not return the exact original")
    entry = object_value(statuses[0])
    if entry.get("item") != original.item:
        raise JourneyFailure("normal query returned another item")
    state = object_value(entry.get("state"))
    if (
        state.get("kind") != "known"
        or field(state, "value", "digest") != original.digest
    ):
        raise JourneyFailure("normal query lost the original identity/digest")
    return object_value(state["value"])


def replay_original(session: MachineReadback, original: FrozenOriginal) -> JsonObject:
    answer = session.operation(
        {
            "kind": "consumer",
            "value": {
                "kind": "mailSubmit",
                "value": {"item": original.envelope, "originEpoch": original.epoch},
            },
        }
    )
    if (
        answer.get("kind"),
        field(answer, "value", "kind"),
        field(answer, "value", "value", "kind"),
        field(answer, "value", "value", "value", "kind"),
    ) != ("consumer", "mailSubmit", "submitted", "replayed"):
        raise JourneyFailure(
            "original normal submission did not replay retained custody"
        )
    return object_value(field(answer, "value", "value", "value", "value"))


def retire_and_replay(
    source: Installation, original: FrozenOriginal, receipt: JourneyReceipt
) -> bool:
    """Source remains stopped; return False rather than retiring unsettled mail."""
    with open_sender_session(source) as session:
        before = query_original(session, original)
        status = field(before, "status", "kind")
        if status not in {"delivered", "uncertain", "expired", "cancelled", "refused"}:
            receipt.record(
                {"kind": "retirementPending", "item": original.item, "query": before}
            )
            return False
        retired = session.operation(delivery_operation("retire", original))
        receipt.equal(
            "normal Session retirement names exactly the original item",
            "MS-5 / decision1100",
            (
                [
                    "delivery",
                    "retire",
                    {"retired": [original.item], "unknown": [], "notCurrentTarget": []},
                ],
                [
                    retired.get("kind"),
                    field(retired, "value", "kind"),
                    field(retired, "value", "value"),
                ],
            ),
        )
        replayed = replay_original(session, original)
        after = query_original(session, original)
        receipt.equal(
            "retired original retains exact status/version", "MS-5", (before, after)
        )
        receipt.equal(
            "retired original submission returns the same terminal receipt",
            "MS-5",
            (
                {key: before.get(key) for key in ["status", "version", "cancel"]},
                replayed,
            ),
        )
        receipt.record(
            {
                "kind": "normalRetirementReplay",
                "item": original.item,
                "retire": retired,
                "before": before,
                "replay": replayed,
                "after": after,
                "compactRowCredit": False,
            }
        )
        return True


def qualify_normal_replay(
    source: Installation, project: str, item: str, receipt: JourneyReceipt
) -> None:
    """Model-free qualification of exact-byte replay; never recipient credit."""
    source.close()
    try:
        original = frozen_original(source, project, item)
        with open_sender_session(source) as session:
            before = query_original(session, original)
            replayed = replay_original(session, original)
            after = query_original(session, original)
            # Live delivery may advance between calls. A terminal receipt is
            # immutable; an unsettled replay must stay within observed versions.
            versions = [
                before.get("version"),
                replayed.get("version"),
                after.get("version"),
            ]
            receipt.equal(
                "normal Session replays existing original custody without resetting its version",
                "OB4 recovery transport",
                (
                    True,
                    all(
                        isinstance(value, int) and not isinstance(value, bool)
                        for value in versions
                    )
                    and int(str(versions[0]))
                    <= int(str(versions[1]))
                    <= int(str(versions[2])),
                ),
            )
        retired = retire_and_replay(source, original, receipt)
        receipt.record(
            {
                "kind": "normalReplayQualification",
                "item": item,
                "retired": retired,
                "recipientArrivalCredit": False,
            }
        )
    finally:
        source.start()

    source.close()
    try:
        reopened = frozen_original(source, project, item)
        receipt.equal(
            "owning reopen preserves original frozen bytes",
            "NI-4 / MS-5",
            (original.envelope, reopened.envelope),
        )
    finally:
        source.start()


def wait_sender_status(
    source: Installation, caller: JsonObject, item: str, *, delivered: bool
) -> JsonObject:
    deadline = time.monotonic() + 30
    while True:
        shown = source.read("router_show", {"caller": caller, "reference": item})
        kind = field(shown, "mail", "status", "kind")
        ready = kind == "delivered" if delivered else kind not in {None, "queued"}
        if ready:
            return shown
        if time.monotonic() >= deadline:
            raise JourneyFailure(
                "sender original status never reached the required boundary"
            )
        time.sleep(0.1)


def actual_retirement_recovery(
    sender: Recipient,
    recipient: Recipient,
    context: JsonObject,
    receipt: JourneyReceipt,
) -> None:
    source = sender.installation
    sender.confirm_model()
    project = notification_project(source, context)
    baseline = begin_observation(recipient)
    recipient.installation.pause()
    try:
        item = freeze_notification(source, recipient.target, project, context)
        actor = object_value(context["actor"])
        holder: JsonObject = {"kind": "session", "session": sender.caller}
        grant = fixture_identity()
        recorded = await_accepted(
            source,
            "owner_grant_record",
            project_request(
                actor,
                project,
                {
                    "kind": "ownerGrantRecord",
                    "grantId": grant,
                    "grant": {
                        "kind": "replaceLead",
                        "project": project,
                        "newHolder": holder,
                        "reason": "Lead01a16924 authorizes the same controller to inspect this owned fixture's outgoing status",
                    },
                },
            ),
        )
        replaced = await_accepted(
            source,
            "role_replace_lead",
            project_request(
                actor,
                project,
                {
                    "kind": "roleReplaceLead",
                    "newHolder": holder,
                    "ownerGrant": grant,
                    "reason": "Owned fixture status-read grant; no identity switching",
                    "expectedAssignmentRevision": 1,
                },
            ),
        )
        receipt.record(
            {
                "kind": "fixtureStatusReadGrant",
                "project": project,
                "controller": sender.caller,
                "ownerGrant": grant,
                "recorded": recorded,
                "installed": replaced,
                "authority": "Lead01a16924-e50d / correction01a16925",
            }
        )
        _ = wait_sender_status(source, sender.caller, item, delivered=False)
        # This is the last source-recipient case: close its provider operation
        # before reopening its Host. The remote recipient stays genuinely live.
        sender.close()
        source.close()
        try:
            original = frozen_original(source, project, item)
        finally:
            source.start()
        # A real owning reopen must retain the frozen original. Pause persists
        # only on the recipient, so the sender can recover its unsettled work.
        _ = wait_sender_status(source, sender.caller, item, delivered=False)
        source.close()
        try:
            reopened = frozen_original(source, project, item)
            receipt.equal(
                "unsettled owning reopen preserves frozen item/digest/body",
                "NI-4 / MS-2",
                (original.envelope, reopened.envelope),
            )
            with open_sender_session(source) as session:
                before = query_original(session, original)
                receipt.equal(
                    "recovery is driven while recipient remains unsettled",
                    "OB4 recovery",
                    (
                        False,
                        field(before, "status", "kind")
                        in {
                            "delivered",
                            "uncertain",
                            "expired",
                            "cancelled",
                            "refused",
                        },
                    ),
                )
                replay = replay_original(session, original)
                receipt.record(
                    {
                        "kind": "unsettledOriginalRecovery",
                        "item": item,
                        "before": before,
                        "replay": replay,
                    }
                )
        finally:
            source.start()
    finally:
        recipient.installation.resume()
    _ = observe_action(
        recipient,
        receipt,
        lambda: {"marker": item, "project": project},
        notification=True,
        baseline=baseline,
    )
    final_before = wait_sender_status(source, sender.caller, item, delivered=True)
    quiet = begin_observation(recipient)
    source.close()
    try:
        original = frozen_original(source, project, item)
        if not retire_and_replay(source, original, receipt):
            raise JourneyFailure(
                "observed recipient mail was not terminal for retirement"
            )
    finally:
        source.start()
    final_after = wait_sender_status(source, sender.caller, item, delivered=True)
    receipt.equal(
        "retirement/replay preserves final public sender status",
        "MS-5",
        (final_before, final_after),
    )
    after = max(
        (
            int(str(field(event, "sequence")))
            for event in array_value(quiet.get("events"))
        ),
        default=0,
    )
    observed = recipient.installation.read(
        "events_observe",
        {
            "target": recipient.target,
            "epoch": quiet["epoch"],
            "afterSequence": after,
            "timeoutSeconds": 3,
            "maxEvents": 1024,
            "maxBytes": 1048576,
        },
    )
    _ = validate_observation(observed, recipient.target, baseline=quiet, after=after)
    if observed.get("endReason") != "deadlineReached":
        raise JourneyFailure("retirement negative window did not reach its deadline")
    receipt.equal(
        "retired original replay causes no second recipient injection in bounded window",
        "MS-2 / MS-5",
        (False, input_observed(observed, item)),
    )
    receipt.record(
        {
            "kind": "retirementRecipientQuietWindow",
            "item": item,
            "baseline": quiet,
            "observed": observed,
        }
    )
