"""Observe reduced repository/default lists against real Host workspace rows."""

import argparse
import contextlib
import http.client
import json
import sqlite3
import subprocess
import sys
import traceback
import typing as t
from pathlib import Path

from .board_journey_fixture import BoardFixture, human_actor, thread_join_header
from .board_journey_record_fixture import feed_records
from .board_journey_runtime import (
    BoardRuntime,
    JourneyReceipt,
    project_request,
    write_header,
)
from .board_journey_target import JourneyTarget
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    file_digest,
    fixture_identity,
    json_value,
    object_value,
    run_command,
)


@t.final
class ListSubjectScenario:
    def __init__(self, fixture: BoardFixture) -> None:
        self.fixture = fixture
        self.repository = fixture.runtime.root / "list-repository"
        self.subjects: dict[str, JsonObject] = {}
        self.authors: dict[str, JsonObject] = {}
        self.last_positions: dict[str, JsonValue] = {}
        self.root_bodies: list[str] = []

    def cli(self, arguments: list[str], label: str) -> JsonObject:
        command = [
            str(self.fixture.runtime.target.cli),
            *arguments,
            "--service-directory",
            str(self.fixture.runtime.directory),
            "--json",
        ]
        result = run_command(command, self.repository)
        output = object_value(json_value(result.stdout)) if result.stdout else {}
        self.fixture.receipt.record(
            {
                "kind": "call",
                "surface": "cli",
                "label": label,
                "command": list[JsonValue](command),
                "cwd": str(self.repository),
                "exit": result.returncode,
                "raw": {"output": output, "stderr": result.stderr},
            }
        )
        if result.returncode or output.get("kind") != "result":
            raise JourneyFailure(f"{label}: {output}")
        return object_value(output["result"])

    def attach_repository(self) -> None:
        self.repository.mkdir(mode=0o700)
        initialized = run_command(["git", "init", "--quiet"], self.repository)
        if initialized.returncode:
            raise JourneyFailure(initialized.stderr)
        result = self.cli(
            [
                "repository",
                "attach",
                "--workspace-id",
                self.fixture.workspace,
                "--repository-path",
                str(self.repository),
                "--actor",
                json.dumps(self.fixture.owner),
            ],
            "attach the owned repository through real CLI discovery",
        )
        self.fixture.receipt.equal(
            "repository association is actually accepted",
            "S-M2b-R1-F1 repository scope",
            ("accepted", field(result, "record", "result", "decision", "kind")),
        )

    def create_thread(self, subject: JsonObject, label: str, actor: JsonObject) -> str:
        fixture = self.fixture
        thread = fixture_identity()
        body = f"R1 root BODY {label}; deliberately unlike the subject title"
        self.root_bodies.append(body)
        _ = fixture.client.accepted(
            "thread_create",
            {
                **write_header(actor),
                "observedEpoch": 1,
                "projectId": fixture.project,
                "postId": thread,
                "about": subject["about"],
                "text": body,
                "references": [],
                "addressees": [],
                "role": "orchestrator",
                "watch": True,
            },
        )
        self.subjects[thread], self.authors[thread] = subject, actor
        _ = fixture.client.accepted(
            "thread_join",
            {
                **thread_join_header(fixture.helper),
                "observedEpoch": 1,
                "thread": {"project": fixture.project, "thread": thread},
                "role": "implementer",
                "watch": True,
                "replace": None,
                "note": None,
            },
        )
        _ = fixture.client.accepted(
            "thread_watch",
            {
                **write_header(fixture.owner),
                "observedEpoch": 1,
                "thread": {"project": fixture.project, "thread": thread},
            },
        )
        post = fixture.reply_request(thread, f"R1 known last activity {label}")
        post["actor"] = fixture.helper
        _ = fixture.client.accepted("thread_post", post)
        shown = fixture.client.read("post_show", {"postId": post["postId"]})
        self.last_positions[thread] = shown["position"]
        return thread

    def snapshot(self, stage: str, protected: list[str]) -> None:
        fixture = self.fixture
        reader = json.dumps(fixture.owner)
        mcp = fixture.client.read(
            "thread_list",
            {
                "workspaceId": fixture.workspace,
                "reader": fixture.owner,
                "watchedOnly": False,
                "page": {"limit": 100, "cursor": None},
            },
        )
        workspace = self.cli(
            ["thread", "list", "--workspace-id", fixture.workspace, "--reader", reader],
            f"{stage}: workspace CLI",
        )
        responses: dict[str, JsonObject] = {"mcp": mcp, "workspaceCli": workspace}
        for name, arguments in [
            (
                "repositoryPath",
                ["--repository-path", str(self.repository), "--reader", reader],
            ),
            ("noSelector", ["--reader", reader]),
            ("plainDefault", []),
        ]:
            responses[name] = self.cli(
                ["thread", "list", *arguments], f"{stage}: {name}"
            )
        receipt = fixture.receipt
        receipt.equal(
            f"{stage}: workspace CLI and TCP MCP return complete identical rows",
            "S-M2b-R1-F1/V8",
            (workspace, mcp),
        )
        indexed: dict[str, dict[str, JsonObject]] = {}
        for name, response in responses.items():
            rows = [
                object_value(row)
                for row in array_value(field(response, "page", "records"))
            ]
            receipt.equal(
                f"{stage}/{name}: exact threads and no continuation",
                "S-M2b-R1-F1 scope/IDs",
                (
                    [list[JsonValue](sorted(self.subjects)), None],
                    [
                        list[JsonValue](
                            sorted(str(row.get("threadId")) for row in rows)
                        ),
                        field(response, "page", "nextCursor"),
                    ],
                ),
            )
            indexed[name] = {str(row["threadId"]): row for row in rows}
            serialized = json.dumps(response)
            for text in [*self.root_bodies, *protected]:
                receipt.equal(
                    f"{stage}/{name}: list contains no root body or protected subject text",
                    "S-M2b-R1-F1 privacy",
                    (False, text in serialized),
                )
        for thread, expected_subject in self.subjects.items():
            reference = indexed["mcp"][thread]
            shown = fixture.client.read(
                "thread_show",
                {
                    "thread": {"project": fixture.project, "thread": thread},
                    "reader": {"kind": "identified", "reader": fixture.owner},
                },
            )
            receipt.equal(
                f"{stage}/{thread}: watch control is actually watching",
                "S-M2b-R1-F1 metadata",
                (True, field(shown, "watchStatus", "watching")),
            )
            for name, rows in indexed.items():
                row = rows[thread]
                receipt.equal(
                    f"{stage}/{name}: computed subject and explicit imported=false preserved",
                    "S-M2b-R1-F1 subject view",
                    (
                        [expected_subject, False, False],
                        [row.get("subject"), row.get("imported"), "title" in row],
                    ),
                )
                receipt.equal(
                    f"{stage}/{name}: both participant identities retained",
                    "S-M2b-R1-F1 participants",
                    (
                        [self.authors[thread], fixture.helper],
                        [
                            field(row, "orchestrator", "identity"),
                            field(row, "implementer", "identity"),
                        ],
                    ),
                )
                if name in {"mcp", "workspaceCli"}:
                    continue
                receipt.equal(
                    f"{stage}/{name}: exact location participants activity and watch projection",
                    "S-M2b-R1-F1 metadata",
                    (
                        [
                            fixture.workspace,
                            fixture.board,
                            fixture.project,
                            thread,
                            reference.get("orchestrator"),
                            reference.get("implementer"),
                            self.last_positions[thread],
                            None
                            if name == "plainDefault"
                            else shown.get("watchStatus"),
                        ],
                        [
                            row.get("workspaceId"),
                            row.get("boardId"),
                            row.get("projectId"),
                            row.get("threadId"),
                            row.get("orchestrator"),
                            row.get("implementer"),
                            row.get("lastActivity"),
                            row.get("watchStatus"),
                        ],
                    ),
                )
        receipt.record(
            {
                "kind": "projectionSnapshot",
                "stage": stage,
                "raw": dict[str, JsonValue](responses),
            }
        )

    def remove_cached_adr(self, adr: str) -> None:
        fixture = self.fixture
        service, epoch = (
            fixture.runtime.manifest["serviceId"],
            fixture.runtime.manifest["serviceEpoch"],
        )
        fixture.runtime.stop()
        # The debug fleet stand-in replaces its cache from fleet_records at boot.
        # Omit only this ADR from that fixture snapshot source as well as the cache;
        # the actual Thread.about, project feed, participants and watch rows stay intact.
        for filename, table in [
            ("fleet.sqlite", "fleet_records"),
            ("fleet-cache.sqlite", "cached_fleet_records"),
        ]:
            path = fixture.runtime.directory / filename
            if not path.resolve().is_relative_to(fixture.runtime.root.resolve()):
                raise JourneyFailure("cache fault escaped owned root")
            with (
                contextlib.closing(
                    sqlite3.connect(f"file:{path}?mode=rw", uri=True)
                ) as connection,
                connection,
            ):
                _ = connection.execute("PRAGMA foreign_keys=ON")
                before = t.cast(
                    list[tuple[str, str, str]],
                    connection.execute(
                        f"SELECT kind,id,payload FROM {table} ORDER BY kind,id"
                    ).fetchall(),
                )
                if (
                    sum(
                        kind == "adr" and identity == adr
                        for kind, identity, _ in before
                    )
                    != 1
                ):
                    raise JourneyFailure("fault must reach one actual admitted ADR")
                changed = connection.execute(
                    f"DELETE FROM {table} WHERE kind='adr' AND id=?", (adr,)
                )
                if changed.rowcount != 1:
                    raise JourneyFailure(
                        "ADR fault must remove exactly the selected row"
                    )
                after = t.cast(
                    list[tuple[str, str, str]],
                    connection.execute(
                        f"SELECT kind,id,payload FROM {table} ORDER BY kind,id"
                    ).fetchall(),
                )
            fixture.receipt.equal(
                f"{filename}: only the selected ADR is omitted from the fixture snapshot",
                "S-M2b-R1-F1 exact fault qualification",
                (
                    list[JsonValue](
                        [
                            list[JsonValue](row)
                            for row in before
                            if row[:2] != ("adr", adr)
                        ]
                    ),
                    list[JsonValue]([list[JsonValue](row) for row in after]),
                ),
            )
            fixture.receipt.record(
                {
                    "kind": "fault",
                    "database": str(path),
                    "table": table,
                    "id": adr,
                    "sql": f"DELETE FROM {table} WHERE kind='adr' AND id=?",
                    "rowsChanged": 1,
                    "mode": "offline owned stand-in snapshot and cache; no project/reference edit",
                    "reason": "native cache file is locked live; boot must not repopulate this selected cache entry",
                    "observerClosedBeforeRestart": True,
                }
            )
        fixture.runtime.start()
        fixture.receipt.equal(
            "missing-cache restart retains service identity and changes epoch",
            "S-M2b-R1-F1 real Host restart",
            (
                [service, True],
                [
                    fixture.runtime.manifest["serviceId"],
                    fixture.runtime.manifest["serviceEpoch"] != epoch,
                ],
            ),
        )

    def run(self) -> None:
        fixture = self.fixture
        self.attach_repository()
        about: JsonObject = {"kind": "workstream", "subject": fixture.workstream}
        first = self.create_thread(
            {"about": about, "title": {"kind": "shown", "title": "verification lane"}},
            "workstream",
            fixture.owner,
        )
        self.snapshot("initial", [])
        _ = fixture.client.accepted(
            "workstream_rename",
            project_request(
                fixture.owner,
                fixture.project,
                {
                    "kind": "workstreamRename",
                    "workstreamId": fixture.workstream,
                    "name": "R1 renamed subject",
                    "expectedRevision": 0,
                },
            ),
        )
        self.subjects[first] = {
            "about": about,
            "title": {"kind": "shown", "title": "R1 renamed subject"},
        }
        self.snapshot("renamed", [])
        refiled: JsonObject = {
            "kind": "project",
            "subject": {"line": "R1 refiled list subject"},
        }
        _ = self.cli(
            [
                "thread",
                "refile",
                "--thread-id",
                f"{fixture.project}/{first}",
                "--actor",
                json.dumps(fixture.owner),
                "--about",
                "project:R1 refiled list subject",
            ],
            "refile through real CLI",
        )
        fixture.receipt.equal(
            "real feed retains exact refile event",
            "S-M2b-R1-F1 refile",
            (
                [[about, refiled]],
                [
                    [row.get("from"), row.get("to")]
                    for row in feed_records(fixture, "threadRefiled")
                ],
            ),
        )
        self.subjects[first] = {
            "about": refiled,
            "title": {"kind": "shown", "title": "R1 refiled list subject"},
        }
        self.snapshot("refiled", [])
        row, adr = fixture_identity(), fixture_identity()
        row_title, row_detail = "R1 protected row title", "R1 protected row detail"
        adr_title, adr_body = "R1 protected ADR title", "R1 protected ADR body"
        _ = fixture.client.accepted(
            "work_trail_add",
            project_request(
                fixture.owner,
                fixture.project,
                {
                    "kind": "workTrailAdd",
                    "rowId": row,
                    "subject": {"kind": "project"},
                    "text": f"{row_title}\n{row_detail}",
                    "tags": [],
                    "links": [],
                },
            ),
        )
        row_thread = self.create_thread(
            {
                "about": {"kind": "workTrailRow", "subject": row},
                "title": {"kind": "shown", "title": row_title},
            },
            "row",
            human_actor("r1-row-author"),
        )
        _ = fixture.client.accepted(
            "adr_create",
            {
                **write_header(fixture.owner),
                "workspaceId": fixture.workspace,
                "adrId": adr,
                "title": adr_title,
                "text": adr_body,
                "supersedes": None,
                "madeIn": None,
                "repositoryFiles": [],
            },
        )
        adr_thread = self.create_thread(
            {
                "about": {"kind": "workspaceAdr", "subject": adr},
                "title": {"kind": "shown", "title": adr_title},
            },
            "ADR",
            human_actor("r1-adr-author"),
        )
        self.snapshot("before privacy faults", [])
        _ = fixture.client.accepted(
            "work_trail_redact",
            project_request(
                fixture.owner,
                fixture.project,
                {"kind": "workTrailRedact", "rowId": row, "redacted": True},
            ),
        )
        self.subjects[row_thread] = {
            "about": {"kind": "workTrailRow", "subject": row},
            "title": {"kind": "redacted"},
        }
        self.remove_cached_adr(adr)
        self.subjects[adr_thread] = {
            "about": {"kind": "workspaceAdr", "subject": adr},
            "title": {"kind": "unavailable"},
        }
        self.snapshot(
            "redacted row and missing cached ADR",
            [row_title, row_detail, adr_title, adr_body],
        )


class ListJourneyOptions(argparse.Namespace):
    def __init__(self) -> None:
        super().__init__()
        self.expected_head: str = ""
        self.manifest: Path = Path()
        self.output_directory: Path = Path()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("--expected-head", required=True)
    _ = parser.add_argument("--manifest", type=Path, required=True)
    _ = parser.add_argument("--output-directory", type=Path, required=True)
    options = parser.parse_args(namespace=ListJourneyOptions())
    repository = Path(__file__).resolve().parents[2]
    receipt = JourneyReceipt(Path(options.output_directory).resolve())
    target = JourneyTarget(repository, str(options.expected_head), None)
    runtime = BoardRuntime(target, receipt)
    passed = False
    try:
        manifest = object_value(json_value(Path(options.manifest).read_bytes()))
        if manifest.get("head") != options.expected_head:
            raise JourneyFailure("artifact manifest names a different source head")
        receipt.target = target.inspect(repository)
        receipt.target.update(
            {
                "manifest": str(options.manifest),
                "manifestSha256": file_digest(Path(options.manifest)),
                "driverFiles": {
                    str(p.relative_to(repository)): file_digest(p)
                    for p in sorted(Path(__file__).parent.glob("*.py"))
                },
                "invocation": list[JsonValue](sys.argv),
            }
        )
        for stage in ["before", "after"]:
            if stage == "after":
                runtime.start()
                fixture = BoardFixture(runtime)
                fixture.create()
                ListSubjectScenario(fixture).run()
                runtime.stop()
            for row in array_value(manifest["artifacts"]):
                artifact = object_value(row)
                path = Path(str(artifact["path"]))
                receipt.equal(
                    f"{stage}: frozen artifact hash {path.name}",
                    "S-M2b-R1-F1 exact build",
                    (
                        [artifact["sha256"], artifact["size"]],
                        [file_digest(path), path.stat().st_size],
                    ),
                )
        passed = True
    except (
        JourneyFailure,
        OSError,
        ValueError,
        KeyError,
        sqlite3.Error,
        subprocess.SubprocessError,
        http.client.HTTPException,
    ) as error:
        receipt.record(
            {
                "kind": "caseFailure",
                "case": "repository-subject-list",
                "error": str(error),
                "raw": {"traceback": traceback.format_exc()},
            }
        )
    finally:
        if runtime.child is not None:
            runtime.stop()
        receipt.record(
            {
                "kind": "result",
                "casesPassed": int(passed),
                "casesFailed": int(not passed),
            }
        )
        receipt.close()
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
