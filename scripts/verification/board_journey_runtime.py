"""Own one isolated board runtime, its clients and inspectable proof receipts."""

import http.client
import json
import os
import selectors
import signal
import socket
import subprocess
import tempfile
import time
import typing as t
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from .board_journey_target import JourneyTarget
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
    json_value,
    object_value,
    run_command,
    string_value,
)


@t.final
class JourneyReceipt:
    def __init__(self, directory: Path) -> None:
        directory.mkdir(parents=True, exist_ok=False)
        self.directory = directory
        self.records: list[JsonObject] = []
        self.target: JsonObject = {}
        self.log = (directory / "journey.log").open("x", encoding="utf-8")

    def record(self, record: JsonObject) -> None:
        self.records.append(record)
        _ = self.log.write(json.dumps(record, ensure_ascii=True) + "\n")
        self.log.flush()
        summary = {key: value for key, value in record.items() if key != "raw"}
        print(json.dumps(summary, ensure_ascii=True), flush=True)
        self.save()

    def save(self) -> None:
        checks = [
            record for record in self.records if record.get("kind") == "assertion"
        ]
        result: JsonObject = {
            "target": self.target,
            "passed": sum(record.get("passed") is True for record in checks),
            "failed": sum(record.get("passed") is False for record in checks),
            "records": t.cast(list[JsonValue], self.records),
        }
        _ = (self.directory / "journey.json").write_text(
            json.dumps(result, indent=2, ensure_ascii=True) + "\n", encoding="utf-8"
        )

    def equal(
        self, label: str, obligation: str, values: tuple[JsonValue, JsonValue]
    ) -> None:
        expected, observed = values
        passed = observed == expected
        self.record(
            {
                "kind": "assertion",
                "label": label,
                "obligation": obligation,
                "expected": expected,
                "observed": observed,
                "passed": passed,
            }
        )
        if not passed:
            raise JourneyFailure(
                f"{label}: expected {expected!r}, observed {observed!r}"
            )

    def close(self) -> None:
        self.save()
        self.log.close()


@t.final
class BoardRuntime:
    def __init__(
        self,
        target: JourneyTarget,
        receipt: JourneyReceipt,
        runtime_parent: Path | None = None,
    ) -> None:
        self.target = target
        self.receipt = receipt
        self.root = Path(tempfile.mkdtemp(prefix="b-", dir=runtime_parent or "/tmp"))
        self.root.chmod(0o700)
        self.directory = self.root / "service"
        self.directory.mkdir(mode=0o700)
        self.child: subprocess.Popen[bytes] | None = None
        self.stderr: t.BinaryIO | None = None
        self.manifest: JsonObject = {}
        self.launch_count = 0
        self.fixture_session: Path | None = None

    @property
    def url(self) -> str:
        return string_value(field(self.manifest, "mcp", "url"))

    def start(self) -> None:
        if self.child is not None:
            raise JourneyFailure("runtime already owned")
        self.target.require_unchanged_binaries()
        self.launch_count += 1
        stderr_path = (
            self.receipt.directory / f"{self.root.name}-runtime-{self.launch_count}.log"
        )
        self.stderr = stderr_path.open("xb")
        environment = os.environ.copy()
        _ = environment.pop("CODEX_ROUTER_HOST_INHERITED_LOCK", None)
        arguments = [str(self.target.launcher), str(self.directory), "0"]
        if self.fixture_session is not None:
            arguments.append(str(self.fixture_session))
        self.child = subprocess.Popen(
            arguments,
            env=environment,
            stdout=subprocess.PIPE,
            stderr=self.stderr,
            start_new_session=True,
        )
        self.receipt.record(
            {
                "kind": "lifecycle",
                "action": "spawn",
                "command": list[JsonValue](arguments),
                "ownedPid": self.child.pid,
                "ownedRoot": str(self.root),
                "stderr": str(stderr_path),
            }
        )
        if self.child.stdout is None:
            raise JourneyFailure("runtime stdout pipe unavailable")
        with selectors.DefaultSelector() as selector:
            _ = selector.register(self.child.stdout, selectors.EVENT_READ)
            if not selector.select(timeout=45):
                raise JourneyFailure(
                    "runtime did not publish its ready fact within 45 seconds"
                )
            ready_line = t.cast(bytes, self.child.stdout.readline())
            if not ready_line:
                self.receipt.record(
                    {
                        "kind": "startupFailure",
                        "ownedPid": self.child.pid,
                        "exit": self.child.poll(),
                        "stderr": str(stderr_path),
                        "raw": {
                            "stderr": stderr_path.read_text(
                                encoding="utf-8", errors="replace"
                            )
                        },
                    }
                )
                raise JourneyFailure(
                    "owned runtime exited before its ready fact; inspect the retained startup failure"
                )
            ready = object_value(json_value(ready_line))
        if ready.get("kind") != "boardDebugRuntimeReady":
            raise JourneyFailure(f"unexpected runtime ready fact: {ready}")
        self.manifest = object_value(
            json_value((self.directory / "service.json").read_bytes())
        )
        parsed = urllib.parse.urlsplit(self.url)
        if (
            self.manifest.get("version") != 3
            or parsed.scheme != "http"
            or parsed.hostname != "127.0.0.1"
            or parsed.port is None
            or parsed.port in {8787, 8788, 43127}
        ):
            raise JourneyFailure(
                f"unexpected private service manifest: {self.manifest}"
            )
        if (
            Path(string_value(ready.get("directory"))).resolve()
            != self.directory.resolve()
            or ready.get("mcpUrl") != self.url
        ):
            raise JourneyFailure(
                "ready fact and owned manifest identify different listeners"
            )
        self.receipt.record(
            {
                "kind": "lifecycle",
                "action": "ready",
                "ready": ready,
                "manifest": self.manifest,
                "unixListener": str(self.directory / "control.sock"),
            }
        )

    def stop(self) -> None:
        child = self.child
        if child is None:
            return
        if child.poll() is None:
            child.send_signal(signal.SIGTERM)
        exit_code = child.wait(timeout=45)
        if child.stdout is not None:
            child.stdout.close()
        if self.stderr is not None:
            self.stderr.close()
            self.stderr = None
        self.child = None
        parsed = urllib.parse.urlsplit(self.url) if self.manifest else None
        tcp_closed = True
        if parsed is not None and parsed.port is not None:
            with socket.socket() as probe:
                tcp_closed = probe.connect_ex(("127.0.0.1", parsed.port)) != 0
        with socket.socket(socket.AF_UNIX) as probe:
            unix_closed = probe.connect_ex(str(self.directory / "control.sock")) != 0
        self.receipt.equal(
            "owned runtime graceful shutdown",
            "runtime ownership",
            (
                {
                    "exit": 0,
                    "tcpClosed": True,
                    "unixClosed": True,
                    "manifestAbsent": True,
                },
                {
                    "exit": exit_code,
                    "tcpClosed": tcp_closed,
                    "unixClosed": unix_closed,
                    "manifestAbsent": not (self.directory / "service.json").exists(),
                },
            ),
        )


@t.final
class BoardClient:
    def __init__(self, runtime: BoardRuntime) -> None:
        self.runtime = runtime
        self.receipt = runtime.receipt
        self.request_number = 0

    def cli(self, arguments: list[str], label: str) -> JsonObject:
        command = [
            str(self.runtime.target.cli),
            *arguments,
            "--service-directory",
            str(self.runtime.directory),
            "--json",
        ]
        result = run_command(command, self.runtime.target.source_root)
        output = object_value(json_value(result.stdout)) if result.stdout else {}
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
        if result.returncode or output.get("kind") != "result":
            raise JourneyFailure(f"{label}: CLI call failed: {output}")
        return object_value(output.get("result"))

    def call(self, tool: str, arguments: JsonObject, label: str) -> JsonValue:
        self.request_number += 1
        payload: JsonObject = {
            "jsonrpc": "2.0",
            "id": self.request_number,
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }
        request = urllib.request.Request(
            self.runtime.url,
            data=json.dumps(payload).encode(),
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json, text/event-stream",
                "MCP-Protocol-Version": "2025-11-25",
            },
        )
        try:
            response = t.cast(
                http.client.HTTPResponse, urllib.request.urlopen(request, timeout=45)
            )
            with response:
                status, body = response.status, response.read().decode()
                content_type = response.headers.get("Content-Type", "")
        except urllib.error.HTTPError as error:
            status, body = error.code, error.read().decode()
            content_type = error.headers.get("Content-Type", "")
        try:
            packets = self.decode_packets(body, content_type)
        except (ValueError, JourneyFailure) as error:
            self.receipt.record(
                {
                    "kind": "call",
                    "surface": "tcpMcp",
                    "label": label,
                    "tool": tool,
                    "arguments": arguments,
                    "httpStatus": status,
                    "raw": {"body": body, "decodeError": str(error)},
                }
            )
            raise JourneyFailure(
                f"{label}: response is not the declared MCP protocol"
            ) from error
        reply = next(
            (packet for packet in packets if packet.get("id") == self.request_number),
            None,
        )
        self.receipt.record(
            {
                "kind": "call",
                "surface": "tcpMcp",
                "label": label,
                "tool": tool,
                "arguments": arguments,
                "httpStatus": status,
                "raw": {"packets": t.cast(list[JsonValue], packets), "body": body},
            }
        )
        if reply is None or "error" in reply:
            raise JourneyFailure(f"{label}: RPC call failed: {reply}")
        result = object_value(reply.get("result"))
        structured = result.get("structuredContent")
        for content in array_value(result.get("content")):
            if field(content, "type") == "text":
                rendered = json_value(string_value(field(content, "text")))
                if rendered != structured:
                    raise JourneyFailure(
                        f"{label}: MCP text and structured payload disagree"
                    )
        return {"isError": result.get("isError", False), "value": structured}

    @staticmethod
    def decode_packets(body: str, content_type: str) -> list[JsonObject]:
        if "text/event-stream" not in content_type:
            return [object_value(json_value(body))]
        packets: list[JsonObject] = []
        data: list[str] = []
        for line in [*body.splitlines(), ""]:
            if line.startswith("data:"):
                data.append(line.removeprefix("data:").lstrip())
            elif not line and data:
                packets.append(object_value(json_value("\n".join(data))))
                data.clear()
        return packets

    def read(self, tool: str, arguments: JsonObject) -> JsonObject:
        response = object_value(self.call(tool, arguments, tool))
        if response.get("isError") is not False:
            raise JourneyFailure(
                f"{tool}: expected successful read, observed {response}"
            )
        return object_value(response.get("value"))

    def observe(self, tool: str, arguments: JsonObject, obligation: str) -> JsonObject:
        response = object_value(self.call(tool, arguments, tool))
        self.receipt.equal(
            f"{tool} preserves the expected readable outcome",
            obligation,
            (
                False,
                response.get("isError"),
            ),
        )
        return object_value(response.get("value"))

    def accepted(self, tool: str, arguments: JsonObject) -> JsonObject:
        deadline = time.monotonic() + 30
        while True:
            reply = self.read(tool, arguments)
            result = object_value(reply.get("result"))
            if result.get("kind") not in {
                "pendingUnreachable",
                "projectCreationPending",
            }:
                break
            if "requestId" not in arguments or time.monotonic() >= deadline:
                raise JourneyFailure(f"{tool}: stable request did not settle: {reply}")
            time.sleep(0.2)
        decisions = decision_values(reply)
        if not decisions or any(
            decision.get("kind") != "accepted" for decision in decisions
        ):
            raise JourneyFailure(
                f"{tool}: expected accepted fixture operation: {reply}"
            )
        return reply


def decision_values(value: JsonValue) -> list[JsonObject]:
    found: list[JsonObject] = []
    if isinstance(value, dict):
        if "decision" in value:
            found.append(object_value(value["decision"]))
        for child in value.values():
            found.extend(decision_values(child))
    elif isinstance(value, list):
        for child in value:
            found.extend(decision_values(child))
    return found


def write_header(actor: JsonObject) -> JsonObject:
    return {
        "requestId": fixture_identity(),
        "protocolVersion": 1,
        "actingFor": None,
        "actor": actor,
    }


def project_request(
    actor: JsonObject, project: str, operation: JsonObject
) -> JsonObject:
    return {
        **write_header(actor),
        "projectId": project,
        "operation": operation,
    }
