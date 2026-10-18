"""Local mail identity through real CLI/MCP and intentionally lost local answers."""

import contextlib
import http.client
import json
import re
import socket
import sqlite3
import typing as t
import urllib.parse

from .board_journey_runtime import BoardClient
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    field,
    fixture_identity,
    json_value,
    object_value,
    run_command,
)
from .mail_journey_peer import MailPeer


@t.final
class MailMessages:
    def __init__(self, peer: MailPeer) -> None:
        self.peer = peer
        self.runtime = peer.runtime
        self.client = BoardClient(self.runtime)
        self.receipt = self.runtime.receipt

    def input(self, identity: str | None, text: str) -> JsonObject:
        value: JsonObject = {
            "target": self.peer.session,
            "message": {"kind": "humanUser", "text": text},
            "delivery": "auto",
        }
        if identity is not None:
            value["itemId"] = identity
        return value

    def cli(self, arguments: list[str], label: str) -> tuple[int, JsonObject]:
        command = [
            str(self.runtime.target.cli),
            *arguments,
            "--service-directory",
            str(self.runtime.directory),
            "--json",
        ]
        result = run_command(command, self.runtime.target.source_root)
        output = object_value(json_value(result.stdout))
        self.receipt.record(
            {
                "kind": "call",
                "surface": "cli",
                "label": label,
                "command": list[JsonValue](command),
                "exit": result.returncode,
                "raw": {"output": output, "stderr": result.stderr},
            }
        )
        return result.returncode, output

    def replay(self) -> None:
        identity = fixture_identity()
        text = "M2c fixture: same identity, one local peer write"
        command = [
            "message",
            "send",
            "--human-user",
            "--to",
            json.dumps(self.peer.session),
            "--text",
            text,
            "--item-id",
            identity,
        ]
        code, output = self.cli(command, "fixed-ID local message")
        self.receipt.equal("fixed-ID CLI succeeds", "MS-2", (0, code))
        original = object_value(field(output, "result", "record"))
        self.receipt.equal(
            "push identity is the supplied mail identity",
            "MS-2",
            (identity, original.get("pushId")),
        )
        self.peer.wait_for_count(1)
        self.receipt.equal(
            "native peer observed one complete user frame",
            "local carrier",
            (1, self.peer.count_after_barrier()),
        )
        replay = self.client.read("message_send", self.input(identity, text))
        self.receipt.equal(
            "MCP replays the complete original result", "MS-2", (original, replay)
        )
        code, again = self.cli(command, "CLI repeat of fixed-ID local message")
        self.receipt.equal("CLI replay succeeds", "MS-2", (0, code))
        self.receipt.equal(
            "CLI replays the same result",
            "MS-2",
            (original, field(again, "result", "record")),
        )
        conflict = object_value(
            self.client.call(
                "message_send",
                self.input(identity, "different fixture content"),
                "same-ID content conflict",
            )
        )
        self.receipt.equal(
            "changed content is refused before effect",
            "MS-2",
            (
                {"isError": True, "kind": "itemConflict", "effect": "none"},
                {
                    "isError": conflict.get("isError"),
                    "kind": field(conflict, "value", "serviceKind"),
                    "effect": field(conflict, "value", "effect"),
                },
            ),
        )
        conflict_command = command.copy()
        conflict_command[conflict_command.index("--text") + 1] = (
            "different fixture content"
        )
        code, cli_conflict = self.cli(conflict_command, "CLI same-ID content conflict")
        self.receipt.equal("CLI conflict is a decided refusal", "M2c R1 F1", (4, code))
        self.receipt.equal(
            "CLI conflict preserves no effect, no target and the existing digest",
            "M2c R1 F1 / S2-F2",
            (
                ["none", None, field(conflict, "value", "data", "existing")],
                [
                    field(cli_conflict, "error", "effect"),
                    cli_conflict.get("target"),
                    field(cli_conflict, "error", "data", "existing"),
                ],
            ),
        )
        malformed = object_value(
            self.client.call(
                "message_send",
                self.input("not-a-uuid", "never admitted"),
                "malformed mail identity",
            )
        )
        self.receipt.equal(
            "malformed identity is invalidShape without effect",
            "PD4 4a1",
            (
                {"isError": True, "kind": "invalidShape", "effect": "none"},
                {
                    "isError": malformed.get("isError"),
                    "kind": field(malformed, "value", "serviceKind"),
                    "effect": field(malformed, "value", "effect"),
                },
            ),
        )
        malformed_command = command.copy()
        malformed_command[malformed_command.index("--item-id") + 1] = "not-a-uuid"
        code, cli_malformed = self.cli(malformed_command, "CLI malformed identity")
        self.receipt.equal(
            "malformed CLI identity is refused locally", "M2c R1 F1", (2, code)
        )
        self.receipt.equal(
            "malformed CLI identity has no effect",
            "M2c R1 F1",
            ("none", field(cli_malformed, "error", "effect")),
        )
        self.receipt.equal(
            "replay and refusals wrote no second peer input",
            "MS-2",
            (1, self.peer.count_after_barrier()),
        )
        shown = self.client.read(
            "router_show", {"caller": self.peer.session, "reference": original["link"]}
        )
        self.receipt.equal(
            "real show reads the full message",
            "MS-6",
            (text, field(shown, "record", "body")),
        )
        with contextlib.closing(
            sqlite3.connect(self.runtime.directory / "automation.sqlite")
        ) as connection:
            row = t.cast(
                tuple[str, str | None, str] | None,
                connection.execute(
                    "SELECT direction,item_json,status_json FROM mail_items WHERE item_id=?",
                    (identity,),
                ).fetchone(),
            )
            if row is None:
                raise JourneyFailure("real send bypassed Local custody")
            self.receipt.equal(
                "terminal Local row is compact", "C36", ("local", row[0])
            )
            self.receipt.equal("terminal payload is released", "C36", (None, row[1]))
            status = object_value(json_value(row[2]))
            self.receipt.equal(
                "canonical status records carrier acceptance",
                "MS-3",
                ("delivered", status.get("kind")),
            )

    def discard_answer(self, arguments: JsonObject) -> None:
        parsed = urllib.parse.urlsplit(self.runtime.url)
        if parsed.hostname != "127.0.0.1" or parsed.port is None:
            raise JourneyFailure(
                "lost-answer probe requires the owned loopback listener"
            )
        payload = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": "message_send", "arguments": arguments},
            }
        ).encode()
        before = self.peer.count_after_barrier()
        with socket.create_connection(
            ("127.0.0.1", parsed.port), timeout=15
        ) as connection:
            header = (
                f"POST {parsed.path} HTTP/1.1\r\nHost: 127.0.0.1:{parsed.port}\r\n"
                "Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n"
                f"MCP-Protocol-Version: 2025-11-25\r\nContent-Length: {len(payload)}\r\nConnection: close\r\n\r\n"
            )
            connection.sendall(header.encode() + payload)
            response = http.client.HTTPResponse(connection)
            response.begin()
            self.receipt.equal(
                "lost-answer request reached the owned service",
                "local answer loss",
                (200, response.status),
            )
            self.peer.wait_for_count(before + 1)
            # Headers are transport acknowledgement only. Never read or parse
            # the result body; close the caller after the real peer write.
            response.close()
        self.receipt.record(
            {
                "kind": "lostAnswer",
                "surface": "tcpMcp",
                "arguments": arguments,
                "resultBodyRead": False,
                "peerWriteObserved": True,
            }
        )

    def lost_answers(self) -> None:
        identity = fixture_identity()
        text = "M2c fixture: supplied ID with its local answer lost"
        before = self.peer.count_after_barrier()
        self.discard_answer(self.input(identity, text))
        replay = self.client.read("message_send", self.input(identity, text))
        self.receipt.equal(
            "supplied-ID lost answer retains its identity",
            "MS-2",
            (identity, replay.get("pushId")),
        )
        self.receipt.equal(
            "supplied-ID retry caused no second native write",
            "MS-2",
            (before + 1, self.peer.count_after_barrier()),
        )
        before = self.peer.count_after_barrier()
        text = "M2c fixture: omitted ID with its local answer lost"
        self.discard_answer(self.input(None, text))
        with self.peer.condition:
            lost_frame = self.peer.messages[-1]
        match = re.search(r"/push/([0-9a-f-]{36})", lost_frame)
        if match is None:
            raise JourneyFailure(
                "the fixture receiver could not identify its first push"
            )
        observed_first = match.group(1)
        retry = self.client.read("message_send", self.input(None, text))
        self.peer.wait_for_count(before + 2)
        self.receipt.equal(
            "a new omitted-ID call creates a new native write",
            "named local-answer-loss exception",
            (before + 2, self.peer.count_after_barrier()),
        )
        self.receipt.equal(
            "omitted-ID retry is a new identity",
            "named local-answer-loss exception",
            (True, retry.get("pushId") != observed_first),
        )
        self.receipt.record(
            {
                "kind": "proofBoundary",
                "obligation": "omitted-ID local answer loss",
                "firstIdObservedBy": "fixture receiver, not the caller",
                "firstPushId": observed_first,
                "retryPushId": retry.get("pushId"),
                "exactlyOnceClaim": False,
            }
        )
