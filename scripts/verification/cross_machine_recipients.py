"""Actual Haiku recipients: one create, model inspection, observed input and close.

No peer-registry stand-in or sender receipt counts as recipient arrival. Create
uses a caller-supplied real controller identity and a fresh inspectable operation;
an uncertain create is never replayed. Model selection is verified before input.
"""

import json
import time
import typing as t
from collections.abc import Callable
from dataclasses import dataclass

from pydantic import BaseModel, ConfigDict, Field

from .board_journey_runtime import JourneyReceipt
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
    object_value,
)
from .cross_machine_lifecycle import Installation
from .interaction_readback import require_advertised_settings
from .typed_observations import (
    EndpointInventory,
    GenerationView,
    OperationView,
    ProviderRecord,
    SessionView,
)

MODEL = "claude-haiku-5-5"
MODEL_SELECTOR = "haiku"


class ObservationPage(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(strict=True, extra="forbid")
    target: SessionView
    generation: GenerationView
    attached: bool
    events: list[JsonValue]
    endReason: str
    continuationGap: bool
    epoch: int = Field(ge=0)


def validate_observation(
    observation: JsonObject,
    target: JsonObject,
    *,
    baseline: JsonObject | None = None,
    after: int = 0,
) -> int:
    """Check the public scoped history; bounded calls always report a gap flag.

    Provider history compacts item updates, so sequence holes are valid. Resync
    markers, lost binding/epoch, missing events and reversed/repeated positions
    cannot qualify a scoped input or a negative observation window.
    """
    page = ObservationPage.model_validate(observation)
    if (
        page.target.model_dump(mode="json") != target
        or not page.attached
        or page.endReason not in {"deadlineReached", "resultLimitReached"}
    ):
        raise JourneyFailure(
            "provider observation is detached, resyncing or belongs to another target"
        )
    if baseline is not None and (
        page.epoch != baseline.get("epoch")
        or page.generation.model_dump(mode="json") != baseline.get("generation")
    ):
        raise JourneyFailure("provider observation changed binding or history epoch")
    for raw in page.events:
        event = object_value(raw)
        if event.get("kind") in {"resyncRequired", "eventTooLarge"} or field(
            event, "event", "kind"
        ) in {"resyncRequired", "eventTooLarge"}:
            raise JourneyFailure(
                "provider history has a resync or oversized-event marker"
            )
        sequence = event.get("sequence")
        if (
            not isinstance(sequence, int)
            or isinstance(sequence, bool)
            or sequence <= after
            or not isinstance(event.get("event"), dict)
        ):
            raise JourneyFailure(
                "provider events are missing, repeated or out of order"
            )
        after = sequence
    return after


@dataclass
class Recipient:
    installation: Installation
    target: JsonObject
    caller: JsonObject
    closed: bool = False

    def confirm_model(self) -> None:
        inspected = self.installation.read(
            "provider_session_inspect", {"target": self.target}
        )
        record = ProviderRecord.model_validate(inspected.get("record", inspected))
        _ = require_advertised_settings(
            record.model_dump(mode="json"), {"model": MODEL_SELECTOR}
        )
        self.installation.receipt.record(
            {
                "kind": "modelBinding",
                "target": self.target,
                "selectedAdvertisedAlias": MODEL_SELECTOR,
                "nativeHaikuModelEnvironment": MODEL,
                "providerExecutable": self.installation.plan.recipient_provider.model_dump(
                    mode="json"
                )
                if self.installation.plan.recipient_provider
                else None,
            }
        )
        if (
            record.capabilities.authStatus is not None
            and record.capabilities.authStatus.kind == "loggedOut"
        ):
            raise JourneyFailure("isolated Claude recipient reports loggedOut")
        if record.state != "idle":
            raise JourneyFailure("recipient is not idle; no new input")

    def close(self) -> None:
        if self.closed:
            return
        operation = fixture_identity()
        _ = self.installation.read(
            "conversation_close",
            {
                "operationId": operation,
                "target": self.target,
                "requestedBy": self.caller,
                "approver": self.caller,
            },
        )
        result = self.installation.read(
            "conversation_operation_wait",
            {"operationId": operation, "timeoutSeconds": 30},
        )
        operation_state = OperationView.model_validate(result["operation"])
        if (
            operation_state.stage,
            operation_state.effect,
            operation_state.reconciliation,
        ) != ("terminal", "applied", "confirmed"):
            raise JourneyFailure("recipient close is not terminal/applied/confirmed")
        self.installation.receipt.record(
            {"kind": "recipientClose", "target": self.target, "settlement": result}
        )
        self.closed = True


def create_recipient(installation: Installation, caller: JsonObject) -> Recipient:
    if installation.plan.recipient_provider is None:
        raise JourneyFailure("declared launcher has no recipient provider binding")
    # Bind the actual harness session through the supported CLI; do not mint
    # an opaque session ID or substitute a Human for the local creator.
    identity = installation.cli(["whoami"])
    local_caller = object_value(field(identity, "result", "record", "session"))
    if (
        field(local_caller, "endpoint", "serviceId")
        != installation.manifest["serviceId"]
    ):
        raise JourneyFailure("whoami returned another service; no create")
    installation.receipt.record(
        {
            "kind": "controllerBinding",
            "providedCaller": caller,
            "localWhoami": local_caller,
        }
    )
    caller = local_caller
    inventory = EndpointInventory.model_validate(
        installation.read("endpoints_list", {})
    )
    endpoints = [
        endpoint
        for endpoint in inventory.endpoints
        if endpoint.endpoint.endpointId == "claude-local"
        and endpoint.availability.state == "available"
    ]
    if len(endpoints) != 1:
        raise JourneyFailure("actual isolated Claude endpoint is unavailable")
    workspace = installation.plan.task_directory / "recipient-workspace"
    prepared = installation.request({"kind": "prepareRecipientWorkspace"})
    if prepared.get("directory") != str(workspace):
        raise JourneyFailure(
            "recipient workspace receipt points outside its declared installation"
        )
    operation = fixture_identity()
    installation.receipt.record(
        {
            "kind": "recipientCreateIntent",
            "operationId": operation,
            "controller": caller,
            "selectedModel": MODEL,
            "workspace": str(workspace),
        }
    )
    creation = installation.request(
        {
            "kind": "cli",
            "arguments": [
                "conversation",
                "create",
                "--endpoint",
                "claude-local",
                "--cwd",
                str(workspace),
                "--access",
                "write-restricted",
                "--from",
                json.dumps(caller),
                "--approver",
                json.dumps(caller),
                "--model",
                MODEL_SELECTOR,
                "--operation-id",
                operation,
                "--timeout-seconds",
                "60",
            ],
        }
    )
    operation_view = installation.read(
        "conversation_operation_show", {"operationId": operation}
    )
    state = OperationView.model_validate(operation_view)
    if (state.stage, state.effect, state.reconciliation) != (
        "terminal",
        "applied",
        "confirmed",
    ):
        raise JourneyFailure(
            "recipient create is not terminal/applied/confirmed; do not replay"
        )
    target = t.cast(JsonObject, state.target.model_dump(mode="json"))
    recipient = Recipient(installation, target, caller)
    installation.receipt.record(
        {
            "kind": "recipientCreateResult",
            "exit": creation.get("exit"),
            "operation": operation_view,
        }
    )
    try:
        recipient.confirm_model()
    except Exception:
        try:
            recipient.close()
        except Exception as cleanup_error:  # noqa: BLE001 -- preserve the original model/authentication failure
            installation.receipt.record(
                {"kind": "recipientCloseGap", "detail": str(cleanup_error)}
            )
        raise
    return recipient


def item_text(observation: JsonObject, kind: str) -> str:
    """Read only the named native item kind from recipient event history."""
    texts: list[str] = []
    for wrapped in array_value(observation.get("events")):
        event = object_value(field(wrapped, "event"))
        if event.get("kind") not in {"itemStarted", "itemUpdated"}:
            continue
        item = object_value(event.get("item"))
        if field(item, "kind", "kind") == kind and isinstance(item.get("text"), str):
            texts.append(t.cast(str, item["text"]))
    return "\n".join(texts)


def agent_output(observation: JsonObject) -> str:
    """Accepted input and tool echoes cannot pass a model-output assertion."""
    return item_text(observation, "agentMessage")


def recipient_turn_matches(
    events: list[JsonValue],
    marker: str,
    *,
    notification: bool,
    input_id: str | None = None,
) -> bool:
    expected_input = (
        input_id if input_id is not None else (marker if notification else None)
    )
    turn: str | None = None
    scoped: list[JsonValue] = []
    for wrapped in events:
        event = object_value(field(wrapped, "event"))
        if event.get("kind") == "turnStarted":
            if expected_input is None or event.get("inputId") == expected_input:
                turn = str(event["turnId"])
                scoped = []
            elif turn is not None:
                return False
        elif turn is not None:
            scoped.append(wrapped)
            if event.get("kind") == "turnEnded" and event.get("turnId") == turn:
                output = agent_output({"events": scoped})
                completed = field(event, "outcome", "kind") == "ended"
                return completed and (
                    bool(output) if notification else marker in output
                )
    return False


def input_observed(observation: JsonObject, item: str) -> bool:
    return any(
        field(wrapped, "event", "kind") == "turnStarted"
        and field(wrapped, "event", "inputId") == item
        for wrapped in array_value(observation.get("events"))
    )


def observe_direct_message(
    sender: Recipient, receiver: Recipient, receipt: JourneyReceipt
) -> JsonObject:
    sender.confirm_model()
    receiver.confirm_model()
    marker = "OB4_ACK_" + fixture_identity().replace("-", "")
    request: JsonObject = {
        "target": receiver.target,
        "itemId": fixture_identity(),
        "delivery": "auto",
        "message": {
            "kind": "agent",
            "sender": sender.target,
            "text": marker
            + "\nTest automation: reply exactly the first line. No tools, delegation or changes.",
        },
    }
    receipt.record({"kind": "directMessageIntent", "request": request})
    return observe_action(
        receiver,
        receipt,
        lambda: {
            "marker": marker,
            "reply": sender.installation.read("message_send", request),
            "request": request,
        },
    )


def begin_observation(receiver: Recipient) -> JsonObject:
    receiver.confirm_model()
    baseline = receiver.installation.read(
        "events_observe",
        {
            "target": receiver.target,
            "timeoutSeconds": 1,
            "maxEvents": 1024,
            "maxBytes": 1048576,
        },
    )
    _ = validate_observation(baseline, receiver.target)
    return baseline


def observe_action(
    receiver: Recipient,
    receipt: JourneyReceipt,
    action: Callable[[], JsonObject],
    *,
    notification: bool = False,
    baseline: JsonObject | None = None,
) -> JsonObject:
    """Attach before the real action; require correlated recipient activity."""
    # Provider hub history owns this cursor. Obtain it before input, then resume
    # after it: a thread scheduled before send would not establish attachment.
    baseline = baseline if baseline is not None else begin_observation(receiver)
    epoch = baseline.get("epoch")
    after = validate_observation(baseline, receiver.target)
    sent = action()
    marker = sent.get("marker")
    if not isinstance(marker, str) or not marker:
        raise JourneyFailure("action has no independently correlated item/marker")
    captures: list[JsonValue] = []
    events: list[JsonValue] = []
    deadline = time.monotonic() + 60
    while True:
        capture = receiver.installation.read(
            "events_observe",
            {
                "target": receiver.target,
                "timeoutSeconds": 3,
                "maxEvents": 1024,
                "maxBytes": 1048576,
                "epoch": epoch,
                "afterSequence": after,
            },
        )
        after = validate_observation(
            capture, receiver.target, baseline=baseline, after=after
        )
        captures.append(capture)
        received = array_value(capture.get("events"))
        events.extend(received)
        request = sent.get("request")
        input_id = request.get("itemId") if isinstance(request, dict) else None
        if notification:
            input_id = marker
        matched = recipient_turn_matches(
            events,
            marker,
            notification=notification,
            input_id=str(input_id) if input_id is not None else None,
        )
        if matched:
            break
        if time.monotonic() >= deadline:
            raise JourneyFailure(
                "actual recipient output/turn end not observed before deadline"
            )
    receipt.record(
        {
            "kind": "actualRecipientObservation",
            "action": sent,
            "notification": notification,
            "baseline": baseline,
            "captures": captures,
        }
    )
    receipt.equal(
        "correlated recipient notification and model turn observed"
        if notification
        else "actual recipient model output contains the exact marker",
        "OB4 / V10",
        (True, matched),
    )
    deadline = time.monotonic() + 30
    while True:
        record = receiver.installation.read(
            "provider_session_inspect", {"target": receiver.target}
        )
        if field(record.get("record", record), "state") == "idle":
            break
        if time.monotonic() >= deadline:
            raise JourneyFailure("actual recipient turn did not settle")
        time.sleep(0.2)
    return {"action": sent, "events": events}


def qualify_recipients(
    installations: list[Installation],
    caller: JsonObject | None,
    receipt: JourneyReceipt,
) -> tuple[list[Recipient], dict[str, str]]:
    recipients: list[Recipient] = []
    gaps: dict[str, str] = {}
    if caller is None:
        return recipients, {
            item.plan.name: "real controller SessionRef not declared"
            for item in installations
        }
    for installation in installations:
        try:
            recipient = create_recipient(installation, caller)
            recipients.append(recipient)
        except Exception as error:  # noqa: BLE001 -- a missing recipient is a named setup gap
            gaps[installation.plan.name] = str(error)
            receipt.record(
                {
                    "kind": "recipientGap",
                    "installation": installation.plan.name,
                    "detail": str(error),
                }
            )
    return recipients, gaps
