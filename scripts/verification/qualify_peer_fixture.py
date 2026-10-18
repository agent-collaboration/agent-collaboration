"""Compare fixture bytes with the shipped producer constructor and JSON serializer."""

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import typing as t
from pathlib import Path

from .board_journey_peer import qualified_peer_entry
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    json_value,
    object_value,
)


def source_between(source: bytes, start: bytes, end: bytes) -> str:
    first = source.find(start)
    last = source.find(end, first)
    if first < 0 or last < 0:
        raise JourneyFailure(f"producer source boundary is absent: {start!r}")
    return source[first:last].decode()


def producer_oracle(content: bytes) -> str:
    # Keep the actual object expression and serializer untouched. Only their
    # external inputs and tracing resource are supplied by this qualification.
    expression = source_between(
        content, b"M=_({pid:process.pid", b");if(n){let P=await n.write"
    )
    expression = expression.removeprefix("M=") + ")"
    serializer = source_between(
        content, b"function _(e,n,r){using i=Uh", b"function u$("
    )
    constants = source_between(content, b"var f0o=600000", b"function CG(")
    feature_functions = source_between(content, b"function CG(){", b"var Op=")
    update = source_between(content, b"wn({...e,updatedAt:r", b",n);if(h&&y)")
    update = update.removeprefix("wn(")
    return f"""
import fs from 'node:fs';
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
const rendered = [];
for (const feature of [false, true]) {{
  const process = {{pid:input.pid}}, K = () => input.sessionId, Ce = () => input.cwd;
  const Date = {{now:() => input.startedAt}}, ql = async () => input.procStart;
  const KA = async () => input.pidDomain, KMr = () => undefined;
  const s = 'interactive', a = {{CLAUDE_CODE_ENTRYPOINT:'cli'}};
  const ZC = {{CLAUDE_CODE_MESSAGING_SOCKET:input.messagingSocketPath}};
  const R = {{name:input.name,source:'user'}}, h = false;
  const O = () => 'macos', Bun = feature ? {{ant:{{getPeerPid:() => 0}}}} : {{}};
  const $Mo = 'reply_across_default_dirs';
  const Uh = () => ({{[Symbol.dispose](){{}}}}), we = (value) => typeof value === 'string' ? value.length : value;
  {serializer}
  {constants}
  {feature_functions}
  const T = undefined;
  const M = {expression};
  const C = JSON.parse(M);
  let e = {{status:input.status}};
  const r = input.updatedAt;
  e = {update};
  rendered.push({{feature,bytes:_({{...C,...e}})}});
}}
console.log(JSON.stringify(rendered));
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    output = t.cast(Path, options.output)
    if output.exists():
        raise JourneyFailure("qualification receipts are never overwritten")
    executable = shutil.which("claude")
    node = shutil.which("node")
    if executable is None or node is None:
        raise JourneyFailure(
            "qualification needs the pinned Claude executable and Node"
        )
    serialized, reference = qualified_peer_entry(
        Path("/tmp/m2-qualification-cwd"), Path("/tmp/m2-qualification.sock")
    )
    content = Path(executable).resolve().read_bytes()
    result = subprocess.run(
        [node, "--input-type=module", "-e", producer_oracle(content)],
        input=serialized,
        text=True,
        capture_output=True,
        check=False,
        timeout=15,
    )
    if result.returncode:
        raise JourneyFailure(
            f"actual producer expression could not run: {result.stderr}"
        )
    candidates = json_value(result.stdout)
    if not isinstance(candidates, list):
        raise JourneyFailure("producer oracle returned no candidates")
    matches = [
        object_value(candidate)
        for candidate in candidates
        if object_value(candidate).get("bytes") == serialized
    ]
    if len(matches) != 1:
        raise JourneyFailure(
            "fixture bytes do not match either actual producer feature variant"
        )
    missing_version = object_value(json_value(serialized))
    del missing_version["version"]
    malformed = json.dumps(missing_version, separators=(",", ":"), ensure_ascii=False)
    if malformed == serialized or serialized + "\n" == matches[0].get("bytes"):
        raise JourneyFailure("producer comparison failed its negative fixture controls")
    receipt: JsonObject = {
        "qualified": True,
        "reference": reference,
        "fixtureModuleSha256": hashlib.sha256(
            Path(__file__).with_name("board_journey_peer.py").read_bytes()
        ).hexdigest(),
        "actualProducerConstructorAndSerializer": True,
        "controlledInputs": [
            "identity",
            "cwd",
            "time",
            "process metadata",
            "socket path",
            "explicit user name",
        ],
        "matchingProducerFeatureVariant": matches[0].get("feature"),
        "missingProducerFieldRejected": malformed != matches[0].get("bytes"),
        "extraNewlineRejected": serialized + "\n" != matches[0].get("bytes"),
        "nativeEntryPublicationAndTurns": "unverified until M4",
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    _ = output.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))
    return 0


if __name__ == "__main__":
    sys.exit(main())
