"""One JSON-lines controller for local and SSH-owned installations."""

import base64
import json
import os
import selectors
import shlex
import subprocess
import sys
import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field

from .board_journey_runtime import JourneyReceipt
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    file_digest,
    object_value,
)
from .cross_machine_plan import InstallationPlan


def ssh_arguments(plan: InstallationPlan, command: list[str]) -> list[str]:
    if plan.ssh_host is None:
        raise ValueError("SSH destination absent")
    return [
        "ssh",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        plan.ssh_host,
        shlex.join(command),
    ]


def stage_remote(
    plan: InstallationPlan, repository: Path, receipt: JourneyReceipt
) -> None:
    """Stage declared private copies; reuse only the owned root and exact hashes."""
    for binding in [plan.launcher, plan.cli]:
        source = binding.transfer_source
        if (
            source is None
            or source.resolve(strict=True) != source
            or file_digest(source) != binding.sha256
        ):
            raise JourneyFailure(
                "remote transfer source is absent, noncanonical or changed"
            )
    files: dict[str, str] = {
        "scripts/__init__.py": "",
        "scripts/verification/__init__.py": "",
    }
    for name in [
        "cross_machine_agent.py",
        "machine_credentials.py",
        "cross_machine_session_gate.py",
        "cross_machine_session_audit.py",
        "board_journey_values.py",
    ]:
        files["scripts/verification/" + name] = base64.b64encode(
            (repository / "scripts/verification" / name).read_bytes()
        ).decode()
    setup = """import sys,json,os,pathlib,base64,shutil
p=json.load(sys.stdin); r=pathlib.Path(p['root'])
assert r.is_absolute() and not r.is_symlink()
free=shutil.disk_usage(r.parent).free
assert free >= 20*1024**3
footprint=sum(f.stat(follow_symlinks=False).st_size for f in r.rglob('*') if f.is_file() and not f.is_symlink()) if r.exists() else 0
extra=sum(a['bytes'] for a in p['artifacts'] if not pathlib.Path(a['path']).exists())
assert footprint+extra <= 5*1024**3 and free-extra >= 15*1024**3
r.mkdir(mode=0o700,exist_ok=True)
assert r.resolve()==r and r.stat().st_uid==os.getuid() and r.stat().st_mode&0o077==0
assert not (r/'service/service.json').exists(), 'owned Host must settle before staging'
(r/'driver').mkdir(mode=0o700,exist_ok=True); (r/'bin').mkdir(mode=0o700,exist_ok=True)
for name,value in p['files'].items():
 f=r/'driver'/name; f.parent.mkdir(parents=True,exist_ok=True); f.write_bytes(base64.b64decode(value)); f.chmod(0o600)
print(json.dumps({'kind':'prepared','root':str(r),'freeBytes':free,'footprintBytes':footprint,'additionalBytes':extra}))
"""
    result = subprocess.run(
        ssh_arguments(plan, [plan.remote_python, "-c", setup]),
        input=json.dumps(
            {
                "root": str(plan.task_directory),
                "files": files,
                "artifacts": [
                    {
                        "path": str(binding.path),
                        "bytes": binding.transfer_source.stat().st_size,
                    }
                    for binding in [plan.launcher, plan.cli]
                    if binding.transfer_source is not None
                ],
            }
        ),
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    for suffix, contents in [("stdout", result.stdout), ("stderr", result.stderr)]:
        diagnostic = receipt.directory / (plan.name + "-stage." + suffix)
        _ = diagnostic.write_text(contents)
        diagnostic.chmod(0o600)
    if result.returncode:
        raise JourneyFailure("remote task-root preparation failed; no launch")
    try:
        prepared = object_value(t.cast(JsonValue, json.loads(result.stdout)))
    except (ValueError, TypeError) as error:
        raise JourneyFailure(
            "remote preflight has no typed receipt; inspect private stage stderr"
        ) from error
    if prepared.get("kind") != "prepared" or prepared.get("root") != str(
        plan.task_directory
    ):
        raise JourneyFailure("remote preparation receipt names another operation/root")
    receipt.record(
        {
            "kind": "remoteStage",
            "name": plan.name,
            "result": prepared,
        }
    )
    upload = """import sys,pathlib,hashlib,os,json
p=pathlib.Path(sys.argv[1]); root=pathlib.Path(sys.argv[3]).resolve()
assert p.parent.resolve()==root/'bin' and not p.is_symlink()
if p.exists():
 assert hashlib.sha256(p.read_bytes()).hexdigest()==sys.argv[2], 'use a new hash-named artifact path'
 print(json.dumps({'kind':'transferred','path':str(p),'sha256':sys.argv[2],'bytes':p.stat().st_size}))
 sys.exit(0)
with p.open('xb') as f:
 os.chmod(p,0o700)
 while data:=sys.stdin.buffer.read(1048576): f.write(data)
assert hashlib.sha256(p.read_bytes()).hexdigest()==sys.argv[2]
print(json.dumps({'kind':'transferred','path':str(p),'sha256':sys.argv[2],'bytes':p.stat().st_size}))
"""
    for remote in [plan.launcher, plan.cli]:
        local = remote.transfer_source
        if local is None:
            raise JourneyFailure("declared transfer source absent")
        with local.open("rb") as stream:
            result = subprocess.run(
                ssh_arguments(
                    plan,
                    [
                        plan.remote_python,
                        "-c",
                        upload,
                        str(remote.path),
                        remote.sha256,
                        str(plan.task_directory),
                    ],
                ),
                stdin=stream,
                capture_output=True,
                timeout=120,
                check=False,
            )
        if result.returncode:
            raise JourneyFailure("remote artifact transfer/hash verification failed")
        try:
            transferred = object_value(t.cast(JsonValue, json.loads(result.stdout)))
        except (ValueError, TypeError) as error:
            raise JourneyFailure(
                "remote upload has no typed receipt; no launch"
            ) from error
        if (
            transferred.get("kind"),
            transferred.get("path"),
            transferred.get("sha256"),
        ) != (
            "transferred",
            str(remote.path),
            remote.sha256,
        ):
            raise JourneyFailure(
                "remote upload receipt does not match its declared artifact"
            )
        receipt.record(
            {
                "kind": "remoteArtifactTransferred",
                "name": plan.name,
                "receipt": transferred,
            }
        )


class SessionAuditSnapshot(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(strict=True, extra="forbid")
    connections: int = Field(ge=0)
    activeConnections: list[int]
    welcomes: int = Field(ge=0)
    closed: int = Field(ge=0)
    frames: dict[str, int]
    errors: list[str]
    events: list[JsonValue]
    eventsDropped: int = Field(ge=0)


@t.final
class Installation:
    def __init__(
        self,
        plan: InstallationPlan,
        repository: Path,
        receipt: JourneyReceipt,
        environment: dict[str, str] | None = None,
    ) -> None:
        self.plan, self.repository, self.receipt = plan, repository, receipt
        self.manifest: JsonObject = {}
        self.child: subprocess.Popen[str] | None = None
        self.environment = environment
        self.stderr: t.TextIO | None = None
        self.session_paused = False

    def receive(self) -> JsonObject:
        if self.child is None or self.child.stdout is None:
            raise JourneyFailure("installation controller is not live")
        with selectors.DefaultSelector() as selector:
            _ = selector.register(self.child.stdout, selectors.EVENT_READ)
            if not selector.select(85):
                raise JourneyFailure("installation operation exceeded its bound")
            line = t.cast(str, self.child.stdout.readline())
        if not line:
            raise JourneyFailure("installation ended without its operation result")
        response = object_value(t.cast(JsonValue, json.loads(line)))
        self.receipt.record(
            {"kind": "installationResult", "name": self.plan.name, "response": response}
        )
        if response.get("kind") == "failure":
            raise JourneyFailure(
                "installation operation failed: " + str(response.get("errorType"))
            )
        return response

    def send(self, request: JsonObject) -> None:
        if self.child is None or self.child.stdin is None:
            raise JourneyFailure("installation controller input is not live")
        _ = self.child.stdin.write(json.dumps(request) + "\n")
        self.child.stdin.flush()

    def start(self) -> None:
        if self.child is not None:
            raise JourneyFailure("installation already owned")
        if self.plan.ssh_host is None:
            command = [
                sys.executable,
                "-u",
                "-m",
                "scripts.verification.cross_machine_agent",
            ]
        else:
            # The remote module contains only standard-library dependencies.
            script = (
                "import os,runpy;os.chdir("
                + repr(str(self.plan.task_directory / "driver"))
                + ");runpy.run_module('scripts.verification.cross_machine_agent',run_name='__main__')"
            )
            command = ssh_arguments(
                self.plan, [self.plan.remote_python, "-u", "-c", script]
            )
        log = self.receipt.directory / (self.plan.name + "-agent.stderr")
        self.stderr = log.open("a", encoding="utf-8")
        self.child = subprocess.Popen(
            command,
            cwd=self.repository,
            env=self.environment or dict(os.environ),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr,
            text=True,
            start_new_session=True,
        )
        declaration = t.cast(JsonObject, self.plan.model_dump(mode="json"))
        declaration["harness_environment"] = {
            name: os.environ[name]
            for name in (
                "CODEX_THREAD_ID",
                "CLAUDE_CODE_SESSION_ID",
                "CURSOR_CONVERSATION_ID",
            )
            if os.environ.get(name)
        }
        declaration["session_paused"] = self.session_paused
        self.send(declaration)
        ready = self.receive()
        if ready.get("kind") != "ready":
            raise JourneyFailure("installation did not report its own readiness")
        self.manifest = object_value(ready.get("manifest"))

    def request(self, value: JsonObject) -> JsonObject:
        self.receipt.record(
            {"kind": "installationCall", "name": self.plan.name, "request": value}
        )
        self.send(value)
        response = self.receive()
        if response.get("kind") != "reply":
            raise JourneyFailure("installation returned another operation's result")
        return object_value(response.get("value"))

    def read(self, tool: str, arguments: JsonObject) -> JsonObject:
        answer = self.request({"kind": "mcp", "tool": tool, "arguments": arguments})
        if answer.get("isError") is not False:
            raise JourneyFailure("public MCP rejected " + tool)
        return object_value(answer.get("value"))

    def cli(self, arguments: list[str]) -> JsonObject:
        answer = self.request({"kind": "cli", "arguments": list[JsonValue](arguments)})
        if answer.get("exit") != 0:
            raise JourneyFailure("public CLI did not exit zero")
        return object_value(answer.get("output"))

    def session_audit(self) -> JsonObject:
        response = self.request({"kind": "sessionGate", "action": "observe"})
        snapshot = SessionAuditSnapshot.model_validate(response)
        return t.cast(JsonObject, snapshot.model_dump(mode="json"))

    def pause(self) -> None:
        answer = self.request({"kind": "sessionGate", "action": "pause"})
        if answer.get("paused") is not True:
            raise JourneyFailure("owned Session gate did not pause")
        self.session_paused = True

    def resume(self) -> None:
        answer = self.request({"kind": "sessionGate", "action": "resume"})
        if answer.get("paused") is not False:
            raise JourneyFailure("owned Session gate did not resume")
        self.session_paused = False

    def close(self) -> None:
        child = self.child
        if child is None:
            return
        try:
            if child.poll() is None and child.stdin is not None:
                self.send({"kind": "stop"})
                child.stdin.close()
                while (stopped := self.receive()).get("kind") != "stopped":
                    pass
                self.receipt.equal(
                    self.plan.name + " operation-owned shutdown",
                    "OB4 lifecycle",
                    (
                        {
                            "exit": 0,
                            "tcpClosed": True,
                            "publicationAbsent": True,
                            "sessionGateClosed": True,
                        },
                        {
                            key: stopped.get(key)
                            for key in [
                                "exit",
                                "tcpClosed",
                                "publicationAbsent",
                                "sessionGateClosed",
                            ]
                        },
                    ),
                )
                if (
                    self.plan.ssh_host is not None
                    and stopped.get("diskViolation") is not False
                ):
                    raise JourneyFailure("remote disk budget stopped the run")
            code = child.wait(timeout=50)
            if code:
                raise JourneyFailure("installation agent did not settle successfully")
        finally:
            if child.poll() is None:
                if child.stdin is not None and not child.stdin.closed:
                    child.stdin.close()
                # Only this recorded controller is signalled. Its handler reaps
                # its own Host; remote SSH hangup follows the same owned path.
                child.terminate()
                _ = child.wait(timeout=50)
            if child.stdout is not None:
                child.stdout.close()
            if self.stderr is not None:
                self.stderr.close()
            if child.poll() is not None:
                self.child = None


class SessionControl(t.Protocol):
    def pause(self) -> None: ...
    def resume(self) -> None: ...
