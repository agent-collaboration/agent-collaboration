"""Real recipient actions for OB4; unqualified controls remain explicit gaps.

Every callable requires authenticated exact-model Recipient objects. Notification
arrival correlates the actual provider input to the public frozen handoff ID and
requires model output and turn end after that input. A publisher receipt alone
cannot satisfy it. No production state or credentials are copied.
"""

import time
from collections.abc import Callable, Sequence

from .board_journey_runtime import JourneyReceipt
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    array_value,
    field,
    fixture_identity,
    string_value,
)
from .cross_machine_central import MatrixCentral
from .cross_machine_copy_control import PreApplyCopyHold
from .cross_machine_lifecycle import SessionControl
from .cross_machine_notifications import (
    freeze_notification,
    notification,
    notification_project,
)
from .cross_machine_recipients import (
    Recipient,
    agent_output,
    begin_observation,
    input_observed,
    observe_action,
    observe_direct_message,
    validate_observation,
)
from .cross_machine_retention import actual_retirement_recovery


def local_independence(
    recipient: Recipient,
    edge: SessionControl,
    project: str,
    context: JsonObject,
    receipt: JourneyReceipt,
) -> None:
    edge.pause()
    try:
        _ = notification(recipient, [recipient], project, context, receipt)
    finally:
        edge.resume()


def offline_retry(
    sender: Recipient,
    recipient: Recipient,
    edge: SessionControl,
    receipt: JourneyReceipt,
) -> None:
    sender.confirm_model()
    marker = "OB4_OFFLINE_" + fixture_identity().replace("-", "")
    request: JsonObject = {
        "target": recipient.target,
        "itemId": fixture_identity(),
        "delivery": "auto",
        "message": {
            "kind": "agent",
            "sender": sender.target,
            "text": marker
            + "\nTest automation: reply exactly the first line. No tools or changes.",
        },
    }

    def submit() -> JsonObject:
        edge.pause()
        try:
            first = sender.installation.read("message_send", request)
            retry = sender.installation.read("message_send", request)
            receipt.record(
                {
                    "kind": "offlineSameIdSubmissions",
                    "request": request,
                    "first": first,
                    "retry": retry,
                }
            )
        finally:
            edge.resume()
        return {"marker": marker, "request": request}

    _ = observe_action(recipient, receipt, submit)
    baseline = begin_observation(recipient)
    _ = sender.installation.read("message_send", request)
    observed = recipient.installation.read(
        "events_observe",
        {
            "target": recipient.target,
            "epoch": baseline["epoch"],
            "afterSequence": max(
                (
                    int(str(field(row, "sequence")))
                    for row in array_value(baseline.get("events"))
                ),
                default=0,
            ),
            "timeoutSeconds": 3,
            "maxEvents": 1024,
            "maxBytes": 1048576,
        },
    )
    receipt.equal(
        "same-ID post-delivery retry causes no second input or model reply in bounded observation",
        "MS-2",
        (
            False,
            marker in agent_output(observed)
            or input_observed(observed, str(request["itemId"])),
        ),
    )
    _ = validate_observation(
        observed,
        recipient.target,
        baseline=baseline,
        after=max(
            (
                int(str(field(row, "sequence")))
                for row in array_value(baseline.get("events"))
            ),
            default=0,
        ),
    )
    if observed.get("endReason") != "deadlineReached":
        raise JourneyFailure("offline retry negative window did not reach its deadline")
    receipt.record(
        {
            "kind": "gap",
            "case": "offline-retry",
            "detail": "Provider reconnection, sender crash/unsettled recovery and long-term dedup still need their own controls; a paused Session and 3s no-repeat window prove only this route.",
        }
    )


def wake(sender: Recipient, recipient: Recipient, receipt: JourneyReceipt) -> None:
    sender.confirm_model()
    marker = "OB4_WAKE_" + fixture_identity().replace("-", "")
    request: JsonObject = {
        "operationId": fixture_identity(),
        "requestedBy": {"kind": "session", **sender.target},
        "replace": False,
        "message": {
            "target": recipient.target,
            "content": {
                "kind": "agent",
                "sender": sender.target,
                "text": marker
                + "\nTest automation: reply exactly the first line. No tools or changes.",
            },
            "delivery": "auto",
        },
        "timing": {"kind": "after", "seconds": 600},
        "expiry": {"kind": "after", "seconds": 660},
    }
    slots: dict[str, str] = {}

    def schedule() -> JsonObject:
        first = sender.installation.read("wake_send", request)
        slot = string_value(field(first, "definition", "wakeupId"))
        slots["current"] = slot
        receipt.equal(
            "wake replay preserves its original slot",
            "MS-10",
            (first, sender.installation.read("wake_send", request)),
        )
        outsider = sender.installation.request(
            {
                "kind": "mcp",
                "tool": "wake_send",
                "arguments": {
                    **request,
                    "operationId": fixture_identity(),
                    "replace": True,
                    "requestedBy": {"kind": "session", **sender.caller},
                },
            }
        )
        receipt.equal(
            "unrelated actual controller cannot replace the wake",
            "MS-10",
            (
                [True, "wakeExists", "none"],
                [
                    outsider.get("isError"),
                    field(outsider, "value", "kind"),
                    field(outsider, "value", "effects", "mutation"),
                ],
            ),
        )
        replacement = sender.installation.read(
            "wake_send",
            {
                **request,
                "operationId": fixture_identity(),
                "replace": True,
                "timing": {"kind": "after", "seconds": 2},
                "expiry": {"kind": "after", "seconds": 60},
            },
        )
        receipt.equal(
            "creator replacement cancels the old wake",
            "MS-10",
            (
                "cancelled",
                sender.installation.read("wake_show", {"wakeupId": slot}).get("state"),
            ),
        )
        slots["current"] = string_value(field(replacement, "definition", "wakeupId"))
        return {"marker": marker, "wake": replacement}

    try:
        _ = observe_action(recipient, receipt, schedule)
        slot = slots.get("current")
        if slot is None:
            raise JourneyFailure("actual wake slot was not observed")
        snapshot = sender.installation.read("wake_show", {"wakeupId": slot})
        receipt.equal(
            "real one-shot wake reaches finished",
            "MS-10",
            ("finished", snapshot.get("state")),
        )
    finally:
        slot = slots.get("current")
        if slot is not None:
            _ = sender.installation.read(
                "wake_cancel", {"operationId": fixture_identity(), "wakeupId": slot}
            )
    receipt.record(
        {
            "kind": "gap",
            "case": "wakes",
            "detail": "Actual target replacement and the lifetime/timing refusal matrix are not yet driven by this recipient route.",
        }
    )


def cancellation(
    sender: Recipient, recipient: Recipient, receipt: JourneyReceipt
) -> None:
    delivered = observe_direct_message(sender, recipient, receipt)
    item = field(delivered, "action", "request", "itemId")
    request: JsonObject = {
        "cancelId": fixture_identity(),
        "target": item,
        "sender": {"kind": "agent", "sender": sender.target},
    }
    deadline = time.monotonic() + 30
    while True:
        cancelled = sender.installation.read("message_cancel", request)
        if field(cancelled, "outcome", "kind") != "pending":
            break
        if time.monotonic() >= deadline:
            receipt.record(
                {
                    "kind": "cancellationPendingObservation",
                    "request": request,
                    "lastReply": cancelled,
                    "windowSeconds": 30,
                    "contractDeadline": False,
                }
            )
            raise JourneyFailure(
                "cancellation settlement unobserved in 30s; MS-9 defines no 30s deadline"
            )
        time.sleep(0.2)
    receipt.equal(
        "cancellation after actual recipient injection is tooLate",
        "MS-9",
        (
            {"kind": "tooLate", "at": "delivered"},
            cancelled.get("outcome"),
        ),
    )
    receipt.record(
        {
            "kind": "gap",
            "case": "cancellation",
            "detail": "Pre-injection prevention and notification membership/generation/copy eligibility remain unqualified; delivered direct-mail tooLate is only one case.",
        }
    )


def held_copy_ack(
    sender: Recipient,
    recipient: Recipient,
    context: JsonObject,
    central: MatrixCentral,
    receipt: JourneyReceipt,
) -> None:
    source = sender.installation
    sender.confirm_model()
    project = notification_project(source, context)
    hold = PreApplyCopyHold(source, central, project)
    baseline = begin_observation(recipient)
    post = fixture_identity()
    hold.arm()
    try:
        item = freeze_notification(
            source, recipient.target, project, context, post=post
        )
        _ = hold.await_piece(post)
        view = source.read("view_work", {"projectId": project, "includeRemoved": False})
        receipt.equal(
            "held cause is outside the acknowledged prefix",
            "OB4 / decision1118",
            (True, view.get("replicatedThrough") != view.get("asOf")),
        )
        unrelated = observe_direct_message(sender, recipient, receipt)
        receipt.equal(
            "unrelated actual mail arrives before held notification",
            "OB4",
            (False, input_observed(unrelated, item)),
        )
        quiet = recipient.installation.read(
            "events_observe",
            {
                "target": recipient.target,
                "epoch": baseline["epoch"],
                "afterSequence": max(
                    (
                        int(str(field(event, "sequence")))
                        for event in array_value(baseline.get("events"))
                    ),
                    default=0,
                ),
                "timeoutSeconds": 3,
                "maxEvents": 1024,
                "maxBytes": 1048576,
            },
        )
        _ = validate_observation(
            quiet,
            recipient.target,
            baseline=baseline,
            after=max(
                (
                    int(str(field(row, "sequence")))
                    for row in array_value(baseline.get("events"))
                ),
                default=0,
            ),
        )
        if quiet.get("endReason") != "deadlineReached":
            raise JourneyFailure(
                "held notification negative window did not reach its deadline"
            )
        receipt.equal(
            "remote notification remains withheld during pre-apply Copy hold",
            "OB4",
            (False, input_observed(quiet, item)),
        )
    finally:
        hold.release()
    _ = observe_action(
        recipient,
        receipt,
        lambda: {"marker": item, "project": project},
        notification=True,
        baseline=baseline,
    )


def recipient_cases(
    recipients: list[Recipient],
    edges: list[SessionControl],
    context: JsonObject,
    receipt: JourneyReceipt,
    results: dict[str, str],
    central: MatrixCentral,
    selected: Sequence[str] | None = None,
) -> None:
    first, second = recipients

    def both_directions() -> None:
        for source, target in [(first, second), (second, first)]:
            _ = observe_direct_message(source, target, receipt)
            project = notification_project(source.installation, context)
            _ = notification(source, [target], project, context, receipt)

    def project_for(source: Recipient) -> str:
        # Each cause owns a fresh project, preserving the real post cooldown.
        return notification_project(source.installation, context)

    actions: list[tuple[str, Callable[[], object], str]] = [
        ("both-directions", both_directions, "observed"),
        (
            "local-independence",
            lambda: local_independence(
                first, edges[0], project_for(first), context, receipt
            ),
            "observed",
        ),
        (
            "distinct-handoffs",
            lambda: notification(
                first, recipients, project_for(first), context, receipt
            ),
            "partial",
        ),
        (
            "offline-retry",
            lambda: offline_retry(first, second, edges[1], receipt),
            "partial",
        ),
        ("cancellation", lambda: cancellation(first, second, receipt), "partial"),
        ("wakes", lambda: wake(first, second, receipt), "partial"),
        (
            "held-copy-ack",
            lambda: held_copy_ack(first, second, context, central, receipt),
            "observed",
        ),
        (
            "receipt-retention",
            lambda: actual_retirement_recovery(first, second, context, receipt),
            "partial",
        ),
    ]
    for name, action, coverage in actions:
        if selected is not None and name not in selected:
            continue
        try:
            _ = action()
            results[name] = coverage
        except Exception as error:  # noqa: BLE001 -- each case retains its own failure and the others still run
            results[name] = "failed"
            receipt.record({"kind": "caseFailure", "case": name, "detail": str(error)})
    # No generic network pause is relabelled an acknowledgement-only hold.
    for name, reason in {
        "distinct-handoffs": "Public frozen intent IDs and recipient arrivals are driven; stopped-owner handoff-body readback/reopen remains unqualified.",
        "receipt-retention": "The normal Session retirement/original replay and bounded no-repeat recipient observation are driven when authenticated recipients exist. Compact central row/reopen/conflict proof remains separate Tokio component evidence.",
    }.items():
        receipt.record({"kind": "gap", "case": name, "detail": reason})
