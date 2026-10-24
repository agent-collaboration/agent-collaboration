"""Own a single declared board Host over local/SSH JSON-lines stdin/stdout.

This standard-library-only agent runs with Python 3.13 or newer. The initial
request is an InstallationPlan JSON object. Commands are mcp, cli and stop.
Secrets are resolved only from the named process environment into an owner-only
fixture file; request/response logs never contain them. EOF always reaps the
recorded Host. It performs no enrollment or central request of its own.
"""

import hashlib
import http.client
import json
import os
import selectors
import shutil
import signal
import socket
import stat
import subprocess
import sys
import threading
import typing as t
import urllib.parse
import urllib.request
from collections.abc import Mapping
from pathlib import Path

from .board_journey_values import (
    JsonObject,
    JsonValue,
    field,
    object_value,
    string_value,
)
from .cross_machine_session_gate import SessionGate
from .machine_credentials import normalize_machine_credential

HARNESS_VARIABLES = (
    "CODEX_THREAD_ID",
    "CLAUDE_CODE_SESSION_ID",
    "CURSOR_CONVERSATION_ID",
)


def emit_response(value: JsonObject) -> None:
    print(json.dumps(value), flush=True)


def remote_disk_snapshot(root: Path) -> JsonObject:
    footprint = 0
    for path in root.rglob("*"):
        try:
            metadata = path.stat(follow_symlinks=False)
        except FileNotFoundError:
            continue
        if stat.S_ISREG(metadata.st_mode):
            footprint += metadata.st_size
    return {"freeBytes": shutil.disk_usage(root).free, "footprintBytes": footprint}


def remote_disk_within_budget(snapshot: JsonObject) -> bool:
    free, footprint = snapshot.get("freeBytes"), snapshot.get("footprintBytes")
    return (
        isinstance(free, int)
        and isinstance(footprint, int)
        and free >= 15 * 1024**3
        and footprint <= 5 * 1024**3
    )


def cli_envelope(contents: str) -> JsonObject | None:
    """Return a typed CLI envelope; diagnostic text cannot become success."""
    result: JsonObject | None = None
    for candidate in [contents, *contents.splitlines()]:
        try:
            decoded = t.cast(JsonValue, json.loads(candidate))
        except (ValueError, TypeError):
            continue
        if isinstance(decoded, dict) and decoded.get("kind") in {"result", "error"}:
            result = decoded
    return result


def verify_binary(binding: JsonObject) -> Path:
    path = Path(string_value(binding.get("path")))
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise ValueError("binary is not a canonical absolute file")
    if hashlib.sha256(path.read_bytes()).hexdigest() != binding.get("sha256"):
        raise ValueError("declared binary digest changed")
    checked = subprocess.run(
        ["codesign", "--verify", "--strict", str(path)],
        capture_output=True,
        check=False,
    )
    if checked.returncode:
        raise ValueError("binary strict signature failed")
    return path


def mcp_call(url: str, tool: str, arguments: JsonObject) -> JsonObject:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    }
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "MCP-Protocol-Version": "2025-11-25",
        },
    )
    response = t.cast(
        http.client.HTTPResponse, urllib.request.urlopen(request, timeout=45)
    )
    with response:
        body = response.read().decode()
        if "text/event-stream" in response.headers.get("Content-Type", ""):
            lines = [
                line.removeprefix("data:").lstrip()
                for line in body.splitlines()
                if line.startswith("data:")
            ]
            packets = [
                object_value(t.cast(JsonValue, json.loads(line))) for line in lines
            ]
        else:
            packets = [object_value(t.cast(JsonValue, json.loads(body)))]
    packet = next((item for item in packets if item.get("id") == 1), None)
    if packet is None or "error" in packet:
        raise ValueError("actual MCP request failed")
    result = object_value(packet.get("result"))
    return {
        "isError": result.get("isError", False),
        "value": result.get("structuredContent"),
    }


def session_gate_control(gate: SessionGate | None, action: JsonValue) -> JsonObject:
    """The local and SSH child expose the same passive audit and outage controls."""
    if (
        gate is None
        or not isinstance(action, str)
        or action not in {"pause", "resume", "observe"}
    ):
        raise ValueError("session gate/control unavailable")
    if action == "observe":
        return gate.audit.snapshot()
    if action == "pause":
        gate.pause()
    else:
        gate.resume()
    return {"paused": gate.paused.is_set()}


def machine_session_fixture(
    session: JsonObject, environment: Mapping[str, str]
) -> JsonObject:
    names = object_value(session.get("credentials"))
    return {
        "endpoint": session["endpoint"],
        "credential": "Bearer "
        + normalize_machine_credential(environment[string_value(names.get("machine"))]),
        "access": {
            "clientId": environment[string_value(names.get("access_id"))],
            "clientSecret": environment[string_value(names.get("access_secret"))],
        },
        "peer": session["peer"],
        "receiver": session["receiver"],
        "relay": session["relay"],
    }


def serve_installation(plan: JsonObject) -> int:
    root = Path(string_value(plan.get("task_directory")))
    parent = Path(string_value(plan.get("temporary_parent")))
    port = plan.get("port")
    if not isinstance(port, int) or port in {8787, 8788, 43127}:
        raise ValueError("invalid or protected port")
    if not root.is_absolute() or root == parent or not root.is_relative_to(parent):
        raise ValueError("invalid task root")
    if any(part in root.parts for part in ["..", ".codex", ".codex-router"]):
        raise ValueError("normal account state cannot be a task root")
    if not root.exists():
        root.mkdir(mode=0o700, parents=False)
    if (
        root.resolve(strict=True) != root
        or root.stat().st_uid != os.getuid()
        or root.stat().st_mode & 0o077
    ):
        raise ValueError("task root must be canonical and owner-private")
    service = root / "service"
    service.mkdir(mode=0o700, exist_ok=True)
    if (
        service.resolve() != service
        or service.stat().st_mode & 0o077
        or (service / "service.json").exists()
    ):
        raise ValueError("service is unsafe or has an existing publication")
    launcher = verify_binary(object_value(plan.get("launcher")))
    cli = verify_binary(object_value(plan.get("cli")))
    arguments = [str(launcher), str(service), str(port)]
    provider = plan.get("recipient_provider")
    if provider is not None:
        provider = object_value(provider)
        if provider.get("model") != "claude-haiku-5-5":
            raise ValueError("only the commissioned Haiku model is permitted")
        for name in ["adapter", "claude"]:
            binding = object_value(provider.get(name))
            executable = Path(string_value(binding.get("path")))
            if executable.resolve(strict=True) != executable or hashlib.sha256(
                executable.read_bytes()
            ).hexdigest() != binding.get("sha256"):
                raise ValueError("provider executable binding changed")
        provider_file = service / "recipient-provider.json"
        configuration = Path(string_value(provider.get("configuration_directory")))
        if plan.get("ssh_host") is not None and not configuration.is_relative_to(root):
            raise ValueError("remote recipient configuration escaped its one task root")
        if (
            not configuration.is_relative_to(parent)
            or configuration == parent
            or configuration.resolve(strict=True) != configuration
            or configuration.stat().st_uid != os.getuid()
            or configuration.stat().st_mode & 0o077
            or any(
                part in configuration.parts
                for part in [".claude", ".codex", ".codex-router"]
            )
        ):
            raise ValueError(
                "recipient configuration is not a private declared task directory"
            )
        with provider_file.open("w", encoding="utf-8") as stream:
            os.chmod(provider_file, 0o600)
            _ = stream.write(
                json.dumps(
                    {
                        "adapter": field(provider, "adapter", "path"),
                        "claude_executable": field(provider, "claude", "path"),
                        "configuration_directory": str(configuration),
                        "api_key_environment": provider.get("api_key_environment"),
                    }
                )
            )
        arguments.extend(["--provider-plan", str(provider_file)])
    harness = object_value(plan.get("harness_environment", {}))
    if any(
        name not in HARNESS_VARIABLES
        or not isinstance(value, str)
        or not value
        or "\0" in value
        for name, value in harness.items()
    ):
        raise ValueError(
            "only actual reported harness identities may cross the controller transport"
        )
    cli_environment = {
        name: value
        for name, value in os.environ.items()
        if name not in HARNESS_VARIABLES
    }
    cli_environment.update(
        {name: string_value(value) for name, value in harness.items()}
    )
    session = plan.get("session")
    fixture: JsonObject | None = None
    session_path: Path | None = None
    if session is not None:
        session = object_value(session)
        fixture = machine_session_fixture(session, os.environ)
        session_path = service / "cross-machine-session.json"
        with session_path.open("w", encoding="utf-8") as stream:
            os.chmod(session_path, 0o600)
            _ = stream.write(json.dumps(fixture))
        # Both native apparatus launchers accept the optional session before
        # the recipient launcher's provider-plan arguments.
        arguments.insert(3, str(session_path))
    environment = dict(os.environ, TMPDIR=str(root))
    if provider is not None:
        # The native provider must report its own harness, not inherit this
        # controller's identity. CLI operations get that real identity separately.
        for variable in HARNESS_VARIABLES:
            _ = environment.pop(variable, None)
        # ACP advertises the alias, while the native CLI consumes this exact
        # model binding. It is a model identifier, never a credential value.
        environment["ANTHROPIC_DEFAULT_HAIKU_MODEL"] = "claude-haiku-5-5"
    child: subprocess.Popen[bytes] | None = None
    stop_monitor, disk_violation = threading.Event(), threading.Event()
    monitor: threading.Thread | None = None
    gate: SessionGate | None = None
    url = ""
    cli_number = 0
    try:
        if fixture is not None and session_path is not None:
            gate = SessionGate(
                string_value(fixture["endpoint"]),
                paused=plan.get("session_paused") is True,
            )
            fixture["endpoint"] = gate.endpoint
            _ = session_path.write_text(json.dumps(fixture))
        with (root / "host.stderr").open("ab") as log:
            child = subprocess.Popen(
                arguments,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=log,
                start_new_session=True,
            )
            if plan.get("ssh_host") is not None:
                owned_child = child

                def watch_disk() -> None:
                    while not stop_monitor.wait(1):
                        snapshot = remote_disk_snapshot(root)
                        if not remote_disk_within_budget(snapshot):
                            disk_violation.set()
                            if owned_child.poll() is None:
                                owned_child.send_signal(signal.SIGTERM)
                            emit_response(
                                {
                                    "kind": "failure",
                                    "errorType": "RemoteDiskBudget",
                                    "disk": snapshot,
                                }
                            )
                            return

                monitor = threading.Thread(target=watch_disk, daemon=True)
                monitor.start()
            if child.stdout is None:
                raise ValueError("Host readiness pipe absent")
            with selectors.DefaultSelector() as selector:
                _ = selector.register(child.stdout, selectors.EVENT_READ)
                if not selector.select(45):
                    raise ValueError("Host did not publish readiness in 45 seconds")
                ready = object_value(
                    t.cast(
                        JsonValue, json.loads(t.cast(bytes, child.stdout.readline()))
                    )
                )
            manifest = object_value(
                t.cast(JsonValue, json.loads((service / "service.json").read_text()))
            )
            url = string_value(field(manifest, "mcp", "url"))
            parsed = urllib.parse.urlsplit(url)
            if (
                ready.get("kind") != "boardDebugRuntimeReady"
                or ready.get("mcpUrl") != url
                or ready.get("directory") != str(service)
            ):
                raise ValueError("operation readiness and service publication disagree")
            if (
                parsed.hostname != "127.0.0.1"
                or parsed.port in {None, 8787, 8788, 43127}
                or (port and parsed.port != port)
            ):
                raise ValueError("Host selected another/protected endpoint")
            emit_response(
                {
                    "kind": "ready",
                    "pid": child.pid,
                    "manifest": manifest,
                    "operation": ready,
                    "diskBefore": remote_disk_snapshot(root)
                    if plan.get("ssh_host") is not None
                    else None,
                }
            )
            for line in sys.stdin:
                reply: JsonObject
                request = object_value(t.cast(JsonValue, json.loads(line)))
                _ = verify_binary(object_value(plan.get("launcher")))
                _ = verify_binary(object_value(plan.get("cli")))
                if request.get("kind") == "stop":
                    break
                if request.get("kind") == "mcp":
                    reply = mcp_call(
                        url,
                        string_value(request.get("tool")),
                        object_value(request.get("arguments")),
                    )
                elif request.get("kind") == "prepareRecipientWorkspace":
                    workspace = root / "recipient-workspace"
                    workspace.mkdir(mode=0o700, exist_ok=True)
                    if (
                        workspace.resolve() != workspace
                        or workspace.stat().st_uid != os.getuid()
                        or workspace.stat().st_mode & 0o077
                    ):
                        raise ValueError(
                            "recipient workspace must be owner-private inside the task"
                        )
                    reply = {"directory": str(workspace)}
                elif request.get("kind") == "sessionGate":
                    reply = session_gate_control(gate, request.get("action"))
                elif request.get("kind") == "cli":
                    values = request.get("arguments")
                    if not isinstance(values, list) or not all(
                        isinstance(value, str) for value in values
                    ):
                        raise ValueError("CLI arguments must be strings")
                    command = [
                        str(cli),
                        *t.cast(list[str], values),
                        "--service-directory",
                        str(service),
                        "--json",
                    ]
                    timed_out = False
                    try:
                        result = subprocess.run(
                            command,
                            capture_output=True,
                            text=True,
                            timeout=75,
                            check=False,
                            env=cli_environment,
                        )
                    except subprocess.TimeoutExpired as error:
                        timed_out = True

                        def decoded(value: bytes | str | None) -> str:
                            return (
                                value.decode(errors="replace")
                                if isinstance(value, bytes)
                                else value or ""
                            )

                        result = subprocess.CompletedProcess(
                            command, 124, decoded(error.stdout), decoded(error.stderr)
                        )
                    cli_number += 1
                    for suffix, contents in [
                        ("stdout", result.stdout),
                        ("stderr", result.stderr),
                    ]:
                        diagnostic = root / f"cli-{cli_number}.{suffix}"
                        with diagnostic.open("w", encoding="utf-8") as stream:
                            os.chmod(diagnostic, 0o600)
                            _ = stream.write(contents)
                    reply = {
                        "exit": result.returncode,
                        "timedOut": timed_out,
                        "output": cli_envelope(result.stdout),
                        "diagnosticPrefix": str(root / f"cli-{cli_number}"),
                        "stdoutSha256": hashlib.sha256(
                            result.stdout.encode()
                        ).hexdigest(),
                        "stderrSha256": hashlib.sha256(
                            result.stderr.encode()
                        ).hexdigest(),
                    }
                else:
                    raise ValueError("unknown installation command")
                emit_response({"kind": "reply", "value": reply})
    finally:
        stop_monitor.set()
        if monitor is not None:
            monitor.join(timeout=3)
        if child is not None:
            try:
                if child.poll() is None:
                    child.send_signal(signal.SIGTERM)
                code = child.wait(timeout=45)
            finally:
                # Closing the transport first can make a healthy Host report
                # SessionEnded before its own graceful signal is processed.
                if gate is not None:
                    gate.close()
            if child.stdout is not None:
                child.stdout.close()
            closed = True
            if url:
                parsed = urllib.parse.urlsplit(url)
                with socket.socket() as probe:
                    closed = probe.connect_ex(("127.0.0.1", parsed.port or 0)) != 0
            publication_absent = not (service / "service.json").exists()
            emit_response(
                {
                    "kind": "stopped",
                    "pid": child.pid,
                    "exit": code,
                    "tcpClosed": closed,
                    "publicationAbsent": publication_absent,
                    "diskAfter": remote_disk_snapshot(root)
                    if plan.get("ssh_host") is not None
                    else None,
                    "diskViolation": disk_violation.is_set(),
                    "sessionGateClosed": gate is None or gate.closed,
                    "sessionAudit": None if gate is None else gate.audit.snapshot(),
                }
            )
            if code or not closed or not publication_absent:
                raise ValueError("owned Host did not settle")
        elif gate is not None:
            gate.close()
    return 0


def main() -> int:
    def terminate(_number: int, _frame: object) -> t.NoReturn:
        raise SystemExit(143)

    _ = signal.signal(signal.SIGTERM, terminate)
    _ = signal.signal(signal.SIGHUP, terminate)
    try:
        plan = object_value(t.cast(JsonValue, json.loads(sys.stdin.readline())))
        return serve_installation(plan)
    except Exception as error:  # noqa: BLE001 -- never expose secret-bearing request/config data
        emit_response({"kind": "failure", "errorType": type(error).__name__})
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
