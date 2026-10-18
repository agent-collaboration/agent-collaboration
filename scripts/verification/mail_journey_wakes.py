"""Observe Router-local wake slots, replacement authority and every bounded timer refusal."""

import json
from datetime import datetime
import typing as t

from .board_journey_runtime import BoardClient
from .board_journey_values import (
    JsonObject,
    field,
    fixture_identity,
    object_value,
    string_value,
)
from .mail_journey_messages import MailMessages
from .mail_journey_peer import MailPeer


@t.final
class MailWakes:
    def __init__(self, peer: MailPeer) -> None:
        self.peer = peer
        self.runtime = peer.runtime
        self.client = BoardClient(self.runtime)
        self.receipt = self.runtime.receipt
        self.commands = MailMessages(peer)

    def session(self, name: str) -> JsonObject:
        return {
            "endpoint": {
                "serviceId": self.runtime.manifest.get("serviceId"),
                "endpointId": "codex-local",
            },
            "sessionId": name,
        }

    def request(self, name: str) -> JsonObject:
        sender = self.session(name)
        return {
            "operationId": fixture_identity(),
            "requestedBy": {"kind": "session", **sender},
            "replace": False,
            "message": {
                "target": self.peer.session,
                "content": {
                    "kind": "agent",
                    "sender": sender,
                    "text": "M2c fixture wake reminder",
                },
                "delivery": "auto",
            },
            "timing": {"kind": "after", "seconds": 600},
            "expiry": {"kind": "none"},
        }

    def refuse(self, request: JsonObject, expected: JsonObject) -> None:
        reply = object_value(self.client.call("wake_send", request, "wake refusal"))
        self.receipt.equal(
            "wake refusal is an error", "MS-10", (True, reply.get("isError"))
        )
        value = object_value(reply.get("value"))
        for name, observed in expected.items():
            self.receipt.equal(
                f"wake refusal {name}", "MS-10", (observed, value.get(name))
            )
        self.receipt.equal(
            "refused wake has no mutation",
            "atomic replacement",
            ("none", field(value, "effects", "mutation")),
        )

    def slots_and_limits(self) -> None:
        before = self.peer.count_after_barrier()
        creator = self.session("m2c-fixture-wake-creator")
        operation = fixture_identity()
        command = [
            "wake",
            "send",
            "--to",
            json.dumps(self.peer.session),
            "--from",
            json.dumps(creator),
            "--text",
            "M2c fixture wake reminder",
            "--after",
            "10m",
            "--operation-id",
            operation,
        ]
        code, output = self.commands.cli(command, "create wake through real CLI")
        self.receipt.equal("wake CLI creates a slot", "MS-10", (0, code))
        first = object_value(field(output, "result", "record"))
        wake = string_value(field(first, "definition", "wakeupId"))
        expected_creator: JsonObject = {"kind": "session", **creator}
        self.receipt.equal(
            "wake persists its creator",
            "MS-10",
            (expected_creator, field(first, "definition", "createdBy")),
        )
        unrelated = self.request("m2c-fixture-outsider")
        self.refuse(
            unrelated,
            {"kind": "wakeExists", "wakeup": wake, "creator": expected_creator},
        )
        unrelated["operationId"] = fixture_identity()
        unrelated["replace"] = True
        self.refuse(
            unrelated,
            {"kind": "wakeExists", "wakeup": wake, "creator": expected_creator},
        )
        replacement = self.request("m2c-fixture-wake-creator")
        replacement["replace"] = True
        created = self.client.read("wake_send", replacement)
        next_id = string_value(field(created, "definition", "wakeupId"))
        self.receipt.equal(
            "creator replacement gets a distinct wake", "MS-10", (True, next_id != wake)
        )
        old = self.client.read("wake_show", {"wakeupId": wake})
        self.receipt.equal(
            "creator replacement cancels the old wake",
            "MS-10",
            ("cancelled", old.get("state")),
        )
        target = self.request("m2c-fixture-target-author")
        target["replace"] = True
        target["requestedBy"] = {"kind": "session", **self.peer.session}
        target["message"] = {
            "target": self.peer.session,
            "content": {
                "kind": "agent",
                "sender": self.peer.session,
                "text": "M2c fixture wake reminder",
            },
            "delivery": "auto",
        }
        target_wake = self.client.read("wake_send", target)
        current_id = string_value(field(target_wake, "definition", "wakeupId"))
        self.receipt.equal(
            "target can replace the slot",
            "MS-10",
            (
                {"kind": "session", **self.peer.session},
                field(target_wake, "definition", "createdBy"),
            ),
        )
        self.receipt.equal(
            "target replacement cancels its predecessor",
            "MS-10",
            (
                "cancelled",
                self.client.read("wake_show", {"wakeupId": next_id}).get("state"),
            ),
        )
        replay = self.request("m2c-fixture-wake-creator")
        replay["operationId"] = operation
        self.receipt.equal(
            "creation replay returns its first result after replacement",
            "MS-2 / MS-10",
            (first, self.client.read("wake_send", replay)),
        )
        cases: list[tuple[JsonObject, JsonObject, JsonObject]] = [
            (
                {"kind": "after", "seconds": 86401},
                {"kind": "none"},
                {"kind": "tooFarAhead", "max": 86400},
            ),
            (
                {"kind": "after", "seconds": 600},
                {"kind": "after", "seconds": 87001},
                {"kind": "lifetimeTooLong", "max": 86400},
            ),
            (
                {"kind": "interval", "seconds": 300},
                {"kind": "none"},
                {"kind": "expiryRequired"},
            ),
            (
                {"kind": "interval", "seconds": 300},
                {"kind": "after", "seconds": 604801},
                {"kind": "lifetimeTooLong", "max": 604800},
            ),
            (
                {"kind": "interval", "seconds": 299},
                {"kind": "after", "seconds": 7200},
                {"kind": "intervalTooShort", "min": 300},
            ),
            (
                {"kind": "cron", "expression": "* * * * *", "timezone": "UTC"},
                {"kind": "after", "seconds": 7200},
                {"kind": "intervalTooShort", "min": 300},
            ),
        ]
        for timing, expiry, expected in cases:
            request = self.request("m2c-fixture-bounded-replacement")
            request.update(
                {
                    "requestedBy": {"kind": "owner"},
                    "replace": True,
                    "timing": timing,
                    "expiry": expiry,
                }
            )
            self.refuse(request, expected)
            current = self.client.read("wake_show", {"wakeupId": current_id})
            # A show cursor has a per-read observation time; compare every
            # persisted scheduling field rather than that observation marker.
            for name in [
                "definition",
                "state",
                "nextDueAt",
                "firstFire",
                "pendingDeliveryId",
            ]:
                self.receipt.equal(
                    f"invalid replacement preserves {name}",
                    "MS-10",
                    (target_wake.get(name), current.get(name)),
                )
        owner = self.request("m2c-fixture-owner-replacement")
        owner.update({"requestedBy": {"kind": "owner"}, "replace": True})
        owner_wake = self.client.read("wake_send", owner)
        self.receipt.equal(
            "declared owner can replace the slot",
            "MS-10",
            ({"kind": "owner"}, field(owner_wake, "definition", "createdBy")),
        )
        self.receipt.equal(
            "wake admission did not send an immediate peer message",
            "scheduling versus delivery",
            (before, self.peer.count_after_barrier()),
        )

    def due_relative_end(self) -> None:
        command = [
            "wake",
            "send",
            "--human-user",
            "--to",
            json.dumps(self.peer.session),
            "--text",
            "M2c R1 due-relative reminder",
            "--replace",
            "--after",
            "24h",
        ]
        code, cli = self.commands.cli(command, "CLI after 24h default end")
        self.receipt.equal("after 24h is admitted", "PD4 §7 at 723ad984", (0, code))
        record = object_value(field(cli, "result", "record"))
        created = datetime.fromisoformat(
            string_value(field(record, "definition", "createdAt"))
        )
        due = datetime.fromisoformat(string_value(record.get("nextDueAt")))
        end = datetime.fromisoformat(
            string_value(field(record, "definition", "expiresAt"))
        )
        self.receipt.equal(
            "after 24h retains its due time",
            "PD4 §7",
            (86400, int((due - created).total_seconds())),
        )
        self.receipt.equal(
            "default end is due plus 24h",
            "PD4 §7",
            (86400, int((end - due).total_seconds())),
        )
        code, exact = self.commands.cli(
            [*command, "--for", "48h"], "CLI exact due plus 24h expiry"
        )
        self.receipt.equal("exact due-relative expiry is accepted", "PD4 §7", (0, code))
        exact_end = datetime.fromisoformat(
            string_value(field(exact, "result", "record", "definition", "expiresAt"))
        )
        exact_start = datetime.fromisoformat(
            string_value(field(exact, "result", "record", "definition", "createdAt"))
        )
        self.receipt.equal(
            "exact end keeps the explicit value",
            "PD4 §7",
            (172800, int((exact_end - exact_start).total_seconds())),
        )
        code, refused = self.commands.cli(
            [*command, "--for", "172801s"], "CLI expiry one second too late"
        )
        self.receipt.equal("later expiry is refused", "PD4 §7", (4, code))
        self.receipt.equal(
            "later expiry names the lifetime limit",
            "PD4 §7",
            (
                ["lifetimeTooLong", 86400, "none"],
                [
                    field(refused, "error", "kind"),
                    field(refused, "error", "max"),
                    field(refused, "error", "effects", "mutation"),
                ],
            ),
        )
        # Exact 24h firing/paused-expiry clocks are permanent storage proofs.
        # This short real timer checks the unchanged native delivery connection.
        count = self.peer.count_after_barrier()
        short = command.copy()
        short[short.index("--after") + 1] = "1s"
        code, short_wake = self.commands.cli(short, "CLI short one-shot real firing")
        self.receipt.equal("short one-shot admitted", "real local timer", (0, code))
        self.peer.wait_for_count(count + 1)
        shown = self.client.read(
            "wake_show",
            {
                "wakeupId": field(
                    short_wake, "result", "record", "definition", "wakeupId"
                )
            },
        )
        fire = object_value(shown.get("firstFire"))
        self.receipt.equal(
            "real one-shot records a firing", "real local timer", (True, bool(fire))
        )
        self.receipt.equal(
            "only one new peer write",
            "real local timer",
            (count + 1, self.peer.count_after_barrier()),
        )
