"""Compare one send and a 16-call batch on a private copy of C1's frozen store."""

import argparse
import concurrent.futures
import contextlib
import hashlib
import http.client
import json
import shutil
import sqlite3
import subprocess
import time
import typing as t
import urllib.request
from pathlib import Path

from .board_journey_runtime import BoardClient, BoardRuntime, JourneyReceipt
from .board_journey_target import JourneyTarget
from .board_journey_values import (
    JsonObject,
    JsonValue,
    array_value,
    fixture_identity,
    json_value,
    object_value,
    string_value,
)


class ProbeOptions(argparse.Namespace):
    def __init__(self) -> None:
        super().__init__()
        self.source_copy: Path = Path()
        self.output_directory: Path = Path()
        self.cold_send: bool = False


def file_hash(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def call(url: str, name: str, arguments: JsonObject, identity: int) -> JsonObject:
    request = urllib.request.Request(
        url,
        data=json.dumps(
            {
                "jsonrpc": "2.0",
                "id": identity,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            }
        ).encode(),
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "MCP-Protocol-Version": "2025-11-25",
        },
    )
    start = time.perf_counter()
    try:
        response = t.cast(
            http.client.HTTPResponse, urllib.request.urlopen(request, timeout=45)
        )
        with response:
            raw = response.read().decode()
            packets = BoardClient.decode_packets(
                raw, response.headers.get("Content-Type", "")
            )
        return {
            "requestId": identity,
            "elapsedMs": (time.perf_counter() - start) * 1000,
            "raw": packets[0],
        }
    except (OSError, TimeoutError) as error:
        # A lost answer is unknown; never retry that supplied item identity.
        return {
            "requestId": identity,
            "elapsedMs": (time.perf_counter() - start) * 1000,
            "error": type(error).__name__,
            "message": str(error),
        }


def states(directory: Path) -> list[JsonValue]:
    with contextlib.closing(
        sqlite3.connect(f"file:{directory / 'automation.sqlite'}?mode=ro", uri=True)
    ) as connection:
        rows = t.cast(
            list[tuple[str, int]],
            connection.execute(
                "SELECT delivery_state,COUNT(*) FROM router_pushes GROUP BY delivery_state"
            ).fetchall(),
        )
        return [t.cast(JsonValue, [state, count]) for state, count in rows]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--source-copy", type=Path, required=True)
    _ = parser.add_argument("--output-directory", type=Path, required=True)
    _ = parser.add_argument("--cold-send", action="store_true")
    options = parser.parse_args(namespace=ProbeOptions())
    repository = Path(__file__).resolve().parents[2]
    frozen = object_value(
        json_value((repository / "tmp/m2/m2c-r1-cli-last-artifacts.json").read_text())
    )
    artifacts = [object_value(value) for value in array_value(frozen["artifacts"])]
    for artifact in artifacts:
        if file_hash(Path(string_value(artifact["path"]))) != artifact["sha256"]:
            raise RuntimeError("frozen R1 artifact changed")
    target = JourneyTarget(repository, str(frozen["head"]), None)
    target.digests = {
        "launcherSha256": file_hash(target.launcher),
        "cliSha256": file_hash(target.cli),
    }
    receipt = JourneyReceipt(options.output_directory.resolve())
    runtime = BoardRuntime(target, receipt)
    source = options.source_copy.resolve()
    if not source.is_relative_to(Path("/tmp").resolve()) or source.name != "service":
        raise RuntimeError("copy only the named retained temporary service fixture")
    original_hash = file_hash(source / "automation.sqlite")
    _ = shutil.copytree(source, runtime.directory, dirs_exist_ok=True)
    if (
        file_hash(source / "automation.sqlite") != original_hash
        or file_hash(runtime.directory / "automation.sqlite") != original_hash
    ):
        raise RuntimeError("source changed during the stopped-fixture copy")
    receipt.record(
        {
            "kind": "sourceCopy",
            "source": str(source),
            "ownedCopy": str(runtime.directory),
            "sourceSha256": original_hash,
            "sourceHead": frozen["head"],
            "statesBefore": states(runtime.directory),
        }
    )
    results: list[JsonObject] = []
    try:
        runtime.start()
        with contextlib.closing(
            sqlite3.connect(
                f"file:{runtime.directory / 'automation.sqlite'}?mode=ro", uri=True
            )
        ) as connection:
            row = t.cast(
                tuple[str, str, str] | None,
                connection.execute(
                    "SELECT target_service_id,target_endpoint_id,target_session_id FROM router_pushes WHERE target_session_id='v-m2c-capacity-unloaded' LIMIT 1"
                ).fetchone(),
            )
        if row is None:
            raise RuntimeError("C1 target absent")
        recipient: JsonObject = {
            "endpoint": {"serviceId": row[0], "endpointId": row[1]},
            "sessionId": row[2],
        }
        if row[0] != runtime.manifest.get("serviceId"):
            raise RuntimeError("copy did not retain its installation identity")

        def message_input() -> JsonObject:
            return {
                "itemId": fixture_identity(),
                "target": recipient,
                "message": {"kind": "humanUser", "text": "\t" * 65536},
                "delivery": "auto",
            }

        cold_input = message_input()
        # This is an actual owner-pass acknowledgement, separate from send timings.
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
            barrier = executor.submit(
                call,
                runtime.url,
                "message_send" if options.cold_send else "thread_subscriptions",
                cold_input
                if options.cold_send
                else {"actor": {"kind": "session", "session": recipient}},
                1,
            )
            if runtime.child is None:
                raise RuntimeError("owned runtime absent")
            sample = subprocess.run(
                [
                    "/usr/bin/sample",
                    str(runtime.child.pid),
                    "2",
                    "10",
                    "-file",
                    str(receipt.directory / "warmup-sample.txt"),
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            receipt.record(
                {"kind": "profile", "phase": "warmup", "exit": sample.returncode}
            )
            result = barrier.result()
        results.append(
            {
                "phase": "coldSequentialSingle" if options.cold_send else "ownerWarmup",
                "itemId": cold_input["itemId"] if options.cold_send else None,
                **result,
            }
        )
        receipt.record({"kind": "measurement", **results[-1]})
        if "error" in result:
            return 1

        one = message_input()
        result = call(runtime.url, "message_send", one, 2)
        results.append({"phase": "sequentialSingle", "itemId": one["itemId"], **result})
        receipt.record({"kind": "measurement", **results[-1]})
        if "error" in result:
            return 1
        inputs = [message_input() for _ in range(16)]
        batch_start = time.perf_counter()
        with concurrent.futures.ThreadPoolExecutor(max_workers=16) as executor:
            pending = [
                executor.submit(call, runtime.url, "message_send", request, index + 3)
                for index, request in enumerate(inputs)
            ]
            for request, response in zip(inputs, pending, strict=True):
                result = {
                    "phase": "concurrent16",
                    "itemId": request["itemId"],
                    **response.result(),
                }
                results.append(result)
                receipt.record({"kind": "measurement", **result})
        receipt.record(
            {
                "kind": "batch",
                "elapsedMs": (time.perf_counter() - batch_start) * 1000,
                "calls": 16,
                "statesAfter": states(runtime.directory),
            }
        )
        return int(any("error" in result for result in results))
    finally:
        runtime.stop()
        receipt.record({"kind": "finalStore", "statesAfter": states(runtime.directory)})
        if file_hash(source / "automation.sqlite") != original_hash:
            raise RuntimeError("original source fixture changed")
        for artifact in artifacts:
            if file_hash(Path(string_value(artifact["path"]))) != artifact["sha256"]:
                raise RuntimeError("frozen artifact changed during probe")
        _ = (receipt.directory / "measurements.json").write_text(
            json.dumps(results, indent=2) + "\n"
        )
        receipt.close()


if __name__ == "__main__":
    raise SystemExit(main())
