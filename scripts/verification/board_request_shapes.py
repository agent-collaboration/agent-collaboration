"""Check shared board builders with current Rust DTOs, without starting G3.

Run from the repository root after the nested helper build:
python -m scripts.verification.board_request_shapes --decoder PATH
The check emits representative shared-builder requests and rejects reintroduced
retired fields in every board journey. It starts no Host and opens no store.
"""

import argparse
import ast
import json
import subprocess
import typing as t
from pathlib import Path
from types import SimpleNamespace

from .board_journey_fixture import (
    BoardFixture,
    human_actor,
    project_thread_subject,
    thread_join_header,
)
from .board_journey_runtime import project_request, write_header
from .board_journey_scope import acknowledgement_entry
from .board_journey_subjects import create_subject_request
from .board_journey_values import JsonObject


def shared_requests() -> list[JsonObject]:
    """Use the journey functions themselves; only fixture identities stand in."""
    identity = "019f0000-0000-7000-8000-000000000010"
    actor = human_actor("wire-shape")
    # Only the two attributes used by the pure request builders are supplied;
    # constructing BoardRuntime here would create unrelated runtime state.
    fixture = t.cast(
        BoardFixture, t.cast(object, SimpleNamespace(project=identity, owner=actor))
    )
    thread: JsonObject = {"project": identity, "thread": identity}
    header = write_header(actor)
    cases: list[tuple[str, JsonObject]] = [
        (
            "project",
            project_request(
                actor,
                identity,
                {
                    "kind": "workstreamCreate",
                    "workstreamId": identity,
                    "name": "wire fixture",
                    "goal": "decode without effects",
                },
            ),
        ),
        (
            "projectCreate",
            {
                **header,
                "projectId": identity,
                "boardId": identity,
                "title": "wire fixture",
                "description": None,
            },
        ),
        (
            "threadCreate",
            create_subject_request(
                fixture, actor, project_thread_subject("wire fixture")
            ),
        ),
        ("threadPost", BoardFixture.reply_request(fixture, identity, "wire fixture")),
        (
            "threadJoin",
            {
                **thread_join_header(actor),
                "thread": thread,
                "role": "participant",
                "watch": False,
                "replace": None,
                "note": None,
            },
        ),
        ("threadWatch", {**header, "thread": thread}),
        ("threadUnwatch", {**header, "thread": thread}),
        ("projectWatch", {**header, "projectId": identity}),
        ("threadResolve", {**header, "thread": thread, "reason": "done"}),
        (
            "threadRefile",
            {
                **header,
                "thread": thread,
                "about": project_thread_subject("wire fixture"),
            },
        ),
        (
            "threadSubscribe",
            {
                **header,
                "scope": {"kind": "thread", "thread": thread},
                "policy": {
                    "mode": "poll",
                    "quietSeconds": 0,
                    "capSeconds": 0,
                    "lifetimeSeconds": 86400,
                },
            },
        ),
        (
            "acknowledgement",
            acknowledgement_entry(
                {"kind": "project", "projectId": identity}, {"epoch": 1, "position": 1}
            ),
        ),
    ]
    return [{"decoder": decoder, "request": request} for decoder, request in cases]


def retired_fields(directory: Path) -> list[str]:
    """Catch stale inline payloads and CLI flags as well as shared builders."""
    failures: list[str] = []
    for path in sorted(directory.glob("board_*.py")):
        if path.name in {"board_request_shapes.py", "board_request_shapes_test.py"}:
            continue
        for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"))):
            if isinstance(node, ast.Constant) and node.value in (
                "observedEpoch",
                "resubmits",
                "--observed-epoch",
            ):
                failures.append(f"{path.name}:{node.lineno}: {node.value}")
    return failures


def check_decoder(decoder: Path) -> JsonObject:
    cases = shared_requests()
    result = subprocess.run(
        [str(decoder)],
        input=json.dumps(cases),
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    if result.returncode or json.loads(result.stdout) != {"decoded": len(cases)}:
        raise ValueError(
            f"current board DTOs rejected shared builders: {result.stderr}"
        )
    rejected = 0
    for case in cases:
        request = t.cast(JsonObject, case["request"])
        for field in ("observedEpoch", "resubmits"):
            wrong = {**case, "request": {**request, field: 1}}
            refused = subprocess.run(
                [str(decoder)],
                input=json.dumps([wrong]),
                text=True,
                capture_output=True,
                timeout=30,
                check=False,
            )
            if refused.returncode == 0 or field not in refused.stderr:
                raise ValueError(f"{case['decoder']} did not reject retired {field}")
            rejected += 1
    return {"decoded": len(cases), "retiredFieldControlsRejected": rejected}


class ShapeArguments(argparse.Namespace):
    decoder: Path = Path()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--decoder", type=Path, required=True)
    arguments = parser.parse_args(namespace=ShapeArguments())
    failures = retired_fields(Path(__file__).parent)
    if failures:
        raise ValueError("retired board request fields remain: " + "; ".join(failures))
    print(json.dumps(check_decoder(arguments.decoder.resolve())))


if __name__ == "__main__":
    main()
