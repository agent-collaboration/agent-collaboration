"""Select a normal pre-apply Copy dispatch without blocking Mailbox or Registry.

Decision 1118 adopts this existing seam for OB4. A held status-read await is not
this seam. Decode the intended cause Piece before counting any observation.
"""

import json
import time
import typing as t

from .board_journey_runtime import JourneyReceipt, write_header
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    field,
    fixture_identity,
    json_value,
    object_value,
)
from .cross_machine_cases import await_accepted
from .cross_machine_central import MatrixCentral
from .cross_machine_lifecycle import Installation
from .cross_machine_notifications import frozen_intents, notification_project


@t.final
class PreApplyCopyHold:
    """Hold the actual Copy dispatch on one holder installation connection."""

    def __init__(
        self, source: Installation, central: MatrixCentral, project: str
    ) -> None:
        self.source, self.central, self.project = source, central, project
        self.destination: JsonObject = {"kind": "copy", "value": project}
        self.address: JsonObject = {
            "kind": "connection",
            "value": source.manifest["serviceId"],
        }
        self.active = False

    def control(self, kind: str) -> JsonObject:
        command: JsonObject = {"kind": kind}
        if kind in {"holdNextDispatch", "resumeDispatchAndHoldNext"}:
            command["value"] = {"destination": self.destination}
        answer = self.central.owner.control(self.address, command)
        self.source.receipt.record(
            {
                "kind": "preApplyCopyDispatchControl",
                "address": self.address,
                "command": command,
                "observation": answer,
            }
        )
        return answer

    def arm(self) -> None:
        _ = self.control("holdNextDispatch")
        self.active = True

    def await_piece(self, post: str) -> JsonObject:
        for _attempt in range(16):
            observed = self.control("awaitHeldDispatch")
            dispatch = object_value(observed.get("dispatch"))
            if (
                dispatch.get("kind") != "held"
                or field(dispatch, "value", "destination") != self.destination
            ):
                raise JourneyFailure("dispatch hold selected another physical Copy")
            raw = field(dispatch, "value", "request")
            if not isinstance(raw, list) or not all(
                isinstance(value, int)
                and not isinstance(value, bool)
                and 0 <= value <= 255
                for value in raw
            ):
                raise JourneyFailure(
                    "held Copy request is not its recorded byte vector"
                )
            envelope = object_value(json_value(bytes(t.cast(list[int], raw))))
            request = object_value(field(envelope, "request", "request"))
            piece = object_value(request.get("value"))
            if (
                request.get("kind") == "piece"
                and piece.get("database") == self.project
                and post in json.dumps(piece)
            ):
                self.source.receipt.record(
                    {
                        "kind": "heldNormalCopyPiece",
                        "description": "pre-apply Copy dispatch hold, which withholds the normal replication acknowledgement",
                        "project": self.project,
                        "post": post,
                        "piece": piece,
                    }
                )
                return piece
            _ = self.control("resumeDispatchAndHoldNext")
        raise JourneyFailure(
            "selected Copy hold never reached the intended cause Piece"
        )

    def release(self) -> None:
        if self.active:
            observed = self.control("observe")
            if field(observed, "dispatch", "kind") == "armed":
                _ = self.control("awaitHeldDispatch")
            _ = self.control("resumeDispatch")
            self.active = False


def qualify_pre_apply_hold(
    first: Installation,
    second: Installation,
    central: MatrixCentral,
    context: JsonObject,
    receipt: JourneyReceipt,
) -> JsonObject:
    project = notification_project(first, context)
    actor = object_value(context["actor"])
    source = object_value(field(first.cli(["whoami"]), "result", "record", "session"))
    target = object_value(field(second.cli(["whoami"]), "result", "record", "session"))
    hold = PreApplyCopyHold(first, central, project)
    post = fixture_identity()
    hold.arm()
    try:
        _ = await_accepted(
            first,
            "thread_create",
            {
                **write_header(actor),
                "projectId": project,
                "postId": post,
                "about": {"kind": "project", "subject": {"line": "OB4 pre-apply hold"}},
                "text": "Owned transport qualification; no recipient model is used or credited.",
                "references": [],
                "addressees": [
                    {
                        "kind": "actor",
                        "recipient": {"kind": "session", "session": target},
                    }
                ],
                "role": None,
                "watch": False,
            },
        )
        piece = hold.await_piece(post)
        view = first.read("view_work", {"projectId": project, "includeRemoved": False})
        intents = frozen_intents(first, project, post, 1)
        receipt.equal(
            "intended cause is outside acknowledged prefix during pre-apply hold",
            "OB4 / decision1118",
            (True, view.get("asOf") != view.get("replicatedThrough")),
        )
        # This route proves unrelated central Mailbox progress, not model arrival.
        item = fixture_identity()
        _ = first.read(
            "message_send",
            {
                "itemId": item,
                "target": target,
                "delivery": "auto",
                "message": {
                    "kind": "agent",
                    "sender": source,
                    "text": "Owned model-free control qualification; no provider reply is credited.",
                },
            },
        )
        deadline = time.monotonic() + 15
        while True:
            mail = first.read("router_show", {"caller": source, "reference": item})
            if field(mail, "mail", "status", "kind") not in {None, "queued"}:
                break
            if time.monotonic() >= deadline:
                raise JourneyFailure(
                    "unrelated Mailbox made no normal progress during Copy hold"
                )
            time.sleep(0.1)
        registry = central.get("/admin/v1/copies?limit=1")
        receipt.record(
            {
                "kind": "preApplyHoldQualification",
                "project": project,
                "heldPiece": piece,
                "writerDuringHold": view,
                "frozenWhileHeld": list(intents),
                "unrelatedMail": mail,
                "registry": registry,
                "recipientArrivalCredit": False,
            }
        )
    finally:
        hold.release()
    intents = frozen_intents(first, project, post, 1)
    deadline = time.monotonic() + 15
    while True:
        view = first.read("view_work", {"projectId": project, "includeRemoved": False})
        if view.get("asOf") == view.get("replicatedThrough"):
            break
        if time.monotonic() >= deadline:
            raise JourneyFailure(
                "released cause/handoff never entered acknowledged prefix"
            )
        time.sleep(0.1)
    receipt.equal(
        "released pre-apply hold acknowledges cause and frozen handoff",
        "OB4 / decision1118",
        (view["asOf"], view["replicatedThrough"]),
    )
    return {"project": project, "item": field(intents[0], "state", "item")}
