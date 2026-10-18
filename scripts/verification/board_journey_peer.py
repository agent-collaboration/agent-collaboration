"""Qualify the metadata stand-in against a pinned shipped registry producer."""

import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from .board_journey_values import (
    JourneyFailure,
    JsonObject,
)

PRODUCER_VERSION = "2.1.301"
PRODUCER_SHA256 = "1f04c69e1c24e4d0a716a863af0aad8fc64d49a499334c29701f073700276b99"


def qualified_peer_entry(repository: Path, socket_path: Path) -> tuple[str, JsonObject]:
    executable = shutil.which("claude")
    if executable is None or sys.platform != "darwin":
        raise JourneyFailure(
            f"G1 needs the qualified macOS Claude Code {PRODUCER_VERSION} producer reference"
        )
    producer = Path(executable).resolve()
    content = producer.read_bytes()
    digest = hashlib.sha256(content).hexdigest()
    if digest != PRODUCER_SHA256:
        raise JourneyFailure(
            "G1 producer changed; qualify its writer/serializer before updating the fixture"
        )
    # These anchors are in the actual shipped writer, not the Router's tolerant reader.
    anchors = [
        b"M=_({pid:process.pid,sessionId:K(),cwd:Ce(),startedAt:Date.now(),procStart:await ql(process.pid)",
        b"peerProtocol:zMr,peerFeatures:TG(),kind:s,entrypoint:a.CLAUDE_CODE_ENTRYPOINT",
        b"hostSessionId:KMr(),pidDomain:await KA()",
        b'zMr=1,VMr="notify_idle",qMr="artifact_yield"',
        b"function TG(){return[VMr,...CG()?[$Mo]:[],qMr]}",
        b'$Mo="reply_across_default_dirs"',
        b"function _(e,n,r){using i=Uh`JSON.stringify(${we(e)})`;return JSON.stringify(e,n,r)}",
        b"wn({...e,updatedAt:r,...e.status!==void 0&&{statusUpdatedAt:r}",
    ]
    if any(anchor not in content for anchor in anchors):
        raise JourneyFailure("G1 producer reference anchors do not match")
    environment = {**os.environ, "LC_ALL": "C", "TZ": "UTC"}
    process = subprocess.run(
        ["ps", "-o", "lstart=", "-p", str(os.getpid())],
        env=environment,
        capture_output=True,
        text=True,
        check=False,
        timeout=5,
    )
    if process.returncode or not process.stdout.strip():
        raise JourneyFailure(
            "cannot obtain producer-equivalent procStart for the owned live PID"
        )
    now = int(datetime.datetime.now(datetime.UTC).timestamp() * 1_000)
    # Registration with an explicit user name, no tmux/job/agent/log/host-session env.
    # Undefined producer properties are absent, exactly as JSON.stringify omits them.
    entry: JsonObject = {
        "pid": os.getpid(),
        "sessionId": f"m2-metadata-{os.getpid()}",
        "cwd": str(repository),
        "startedAt": now,
        "procStart": process.stdout.strip(),
        "version": PRODUCER_VERSION,
        "peerProtocol": 1,
        "peerFeatures": ["notify_idle", "reply_across_default_dirs", "artifact_yield"],
        "kind": "interactive",
        "entrypoint": "cli",
        "pidDomain": "darwin",
        "messagingSocketPath": str(socket_path),
        "name": "m2-verification-context",
        "nameSince": now,
    }
    # The real status updater merges this shape into the registered object.
    entry.update({"status": "idle", "updatedAt": now, "statusUpdatedAt": now})
    serialized = json.dumps(entry, separators=(",", ":"), ensure_ascii=False)
    reference: JsonObject = {
        "producerExecutable": str(producer),
        "producerVersion": PRODUCER_VERSION,
        "producerSha256": digest,
        "writer": "xG registration; wn merge; jje status update",
        "serializer": "one-argument JSON.stringify; compact UTF-8; no newline",
        "entrySha256": hashlib.sha256(serialized.encode()).hexdigest(),
        "producerFields": list(entry),
        "nativeProducerAndTurns": "unverified until M4",
    }
    return serialized, reference
