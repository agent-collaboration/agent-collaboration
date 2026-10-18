"""Declared board data and narrowly scoped offline faults for real-app journeys."""

import contextlib
import datetime
import json
import sqlite3
import typing as t
from pathlib import Path

from .board_journey_peer import qualified_peer_entry

from .board_journey_runtime import (
    BoardClient,
    BoardRuntime,
    decision_values,
    project_request,
    write_header,
)
from .board_journey_values import (
    JsonObject,
    JsonValue,
    JourneyFailure,
    array_value,
    field,
    fixture_identity,
    file_digest,
    json_value,
    object_value,
    run_command,
    string_value,
)


def human_actor(name: str) -> JsonObject:
    return {"kind": "human", "humanId": f"m2-verification-{name}"}


def project_thread_subject(line: str) -> JsonObject:
    return {"kind": "project", "subject": {"line": line}}


def thread_join_header(actor: JsonObject) -> JsonObject:
    """ThreadJoinRequest carries its actor directly and has no actingFor field."""
    return {
        "requestId": fixture_identity(),
        "protocolVersion": 1,
        "resubmits": None,
        "actor": actor,
    }


def record_reference(kind: str, identity: str) -> JsonObject:
    return {"kind": kind, "id": identity}


def evidence_reference(kind: str, identity: str) -> JsonObject:
    return {"kind": "record", "record": record_reference(kind, identity)}


def accepted_decision(reply: JsonObject) -> JsonObject:
    decisions = decision_values(reply)
    if len(decisions) != 1 or decisions[0].get("kind") != "accepted":
        raise JourneyFailure(f"expected exactly one accepted decision: {reply}")
    return decisions[0]


@t.final
class BoardFixture:
    def __init__(self, runtime: BoardRuntime) -> None:
        self.runtime = runtime
        self.client = BoardClient(runtime)
        self.receipt = runtime.receipt
        self.owner = human_actor("lead")
        self.reader = human_actor("reader")
        self.helper = human_actor("implementer")
        self.workspace = fixture_identity()
        self.board = fixture_identity()
        self.project = fixture_identity()
        self.workstream = fixture_identity()
        self.project_titles: dict[str, str] = {}
        self.main_author_revisions: dict[str, int] = {}

    def create(self) -> None:
        reply = self.client.cli(
            [
                "workspace",
                "create",
                "--workspace-id",
                self.workspace,
                "--name",
                self.workspace,
                "--actor",
                json.dumps(self.owner),
            ],
            "create owned workspace through real CLI",
        )
        _ = accepted_decision(reply)
        _ = self.client.accepted(
            "board_create",
            {
                **write_header(self.owner),
                "workspaceId": self.workspace,
                "boardId": self.board,
                "name": self.board,
                "description": "Owned board verification fixture",
                "kind": {"kind": "area"},
            },
        )
        self.create_project(self.project)
        _ = accepted_decision(
            self.client.cli(
                [
                    "workstream",
                    "create",
                    "--project-id",
                    self.project,
                    "--workstream-id",
                    self.workstream,
                    "--name",
                    "verification lane",
                    "--goal",
                    "observe recorded outcomes",
                    "--actor",
                    json.dumps(self.owner),
                ],
                "create owned workstream through real CLI",
            )
        )

    def create_project(self, project: str, board: str | None = None) -> None:
        title = f"verification {project}"
        _ = accepted_decision(
            self.client.cli(
                [
                    "project",
                    "create",
                    "--board-id",
                    board or self.board,
                    "--project-id",
                    project,
                    "--title",
                    title,
                    "--actor",
                    json.dumps(self.owner),
                ],
                "create owned project through real CLI",
            )
        )
        self.project_titles[project] = title
        _ = self.client.accepted(
            "role_grant",
            project_request(
                self.owner,
                project,
                {
                    "kind": "roleGrant",
                    "subject": {"kind": "project", "subject": project},
                    "role": "lead",
                    "holder": self.owner,
                    "model": None,
                    "lineage": None,
                    "expectedRevision": 0,
                    "ownerGrant": None,
                },
            ),
        )

    def foreign_project(self) -> tuple[str, str]:
        workspace, board, project = [fixture_identity() for _ in range(3)]
        _ = self.client.accepted(
            "workspace_create",
            {
                **write_header(self.owner),
                "workspaceId": workspace,
                "name": workspace,
                "description": "Owned foreign-scope fixture",
            },
        )
        _ = self.client.accepted(
            "board_create",
            {
                **write_header(self.owner),
                "workspaceId": workspace,
                "boardId": board,
                "name": board,
                "description": "Owned foreign-scope fixture",
                "kind": {"kind": "area"},
            },
        )
        self.create_project(project, board)
        return workspace, project

    def grant_implementer(self, holder: JsonObject | None = None) -> None:
        _ = self.client.accepted(
            "role_grant",
            project_request(
                self.owner,
                self.project,
                {
                    "kind": "roleGrant",
                    "subject": {"kind": "workstream", "subject": self.workstream},
                    "role": "implementer",
                    "holder": holder or self.helper,
                    "model": None,
                    "lineage": None,
                    "expectedRevision": 0,
                    "ownerGrant": None,
                },
            ),
        )

    def create_task(self, title: str) -> str:
        task = fixture_identity()
        _ = accepted_decision(
            self.client.cli(
                [
                    "task",
                    "create",
                    "--project-id",
                    self.project,
                    "--task-id",
                    task,
                    "--workstream-id",
                    self.workstream,
                    "--title",
                    title,
                    "--actor",
                    json.dumps(self.owner),
                ],
                "create owned task through real CLI",
            )
        )
        return task

    def create_thread(self, project: str | None = None) -> str:
        thread = fixture_identity()
        root_text = self.runtime.root / f"root-{thread}.txt"
        _ = root_text.write_text(f"root {thread}", encoding="utf-8")
        _ = accepted_decision(
            self.client.cli(
                [
                    "thread",
                    "create",
                    "--project-id",
                    project or self.project,
                    "--post-id",
                    thread,
                    "--about",
                    f"project:verification thread {thread}",
                    "--actor",
                    json.dumps(self.owner),
                    "--no-watch",
                    "--text-file",
                    str(root_text),
                ],
                "create owned thread through real CLI",
            )
        )
        return thread

    def reply_request(
        self, thread: str, text: str, project: str | None = None
    ) -> JsonObject:
        return {
            **write_header(self.owner),
            "observedEpoch": 1,
            "postId": fixture_identity(),
            "placement": {
                "kind": "thread",
                "thread": {
                    "project": project or self.project,
                    "thread": thread,
                },
            },
            "text": text,
            "references": [],
            "addressees": [],
        }

    def reply(self, thread: str, text: str, project: str | None = None) -> JsonObject:
        request = self.reply_request(thread, text, project)
        _ = accepted_decision(
            self.client.cli(
                [
                    "post",
                    "post",
                    "--placement",
                    "thread",
                    "--thread-id",
                    f"{project}/{thread}" if project is not None else thread,
                    "--post-id",
                    string_value(request["postId"]),
                    "--request-id",
                    string_value(request["requestId"]),
                    "--observed-epoch",
                    "1",
                    "--text",
                    text,
                    "--actor",
                    json.dumps(self.owner),
                ],
                "post exact owned reply through real CLI",
            )
        )
        return request

    def initialize(self, project: str | None = None) -> None:
        project = project or self.project
        answer = self.client.cli(
            [
                "inbox",
                "fetch",
                "--scope",
                "project",
                "--project-id",
                project,
                "--reader",
                json.dumps(self.reader),
            ],
            "initialize owned reader through real CLI",
        )
        self.receipt.equal(
            "new main inbox starts empty",
            "G3/inbox initialization",
            (
                [],
                field(answer, "page", "records"),
            ),
        )

    def add_trail(self, fields: JsonObject, actor: JsonObject | None = None) -> str:
        identity = fixture_identity()
        subject = fields.get("subject", {"kind": "project"})
        text = string_value(fields.get("text", "verification observation"))
        arguments = [
            "work-trail",
            "add",
            text,
            "--project-id",
            self.project,
            "--row-id",
            identity,
            "--subject",
            json.dumps(subject),
            "--actor",
            json.dumps(actor or self.owner),
        ]
        for tag in array_value(fields.get("tags", [])):
            arguments.extend(["--tag", string_value(tag)])
        for link in array_value(fields.get("links", [])):
            arguments.extend(["--link", json.dumps(link)])
        _ = accepted_decision(
            self.client.cli(arguments, "append declared work through real CLI")
        )
        return identity

    def feed(self) -> JsonObject:
        return self.client.read(
            "feed_read",
            {
                "projectId": self.project,
                "reader": {"kind": "anonymous"},
                "after": {"epoch": 1, "position": 0},
                "limit": 100,
                "includeRedacted": False,
            },
        )

    def inbox(self, fields: JsonObject | None = None) -> JsonObject:
        return self.client.read(
            "inbox_fetch",
            {
                "protocolVersion": 1,
                "actingFor": None,
                "reader": self.reader,
                "scope": {"kind": "workspace", "workspaceId": self.workspace},
                "readMode": "unread",
                "initializations": [],
                "page": {"limit": 100},
                **(fields or {}),
            },
        )

    def summaries(self) -> list[JsonValue]:
        answer = self.client.read(
            "inbox_workspaces",
            {
                "reader": self.reader,
                "unreadOnly": False,
                "page": {"limit": 100},
            },
        )
        return array_value(field(answer, "page", "records"))

    def restart(self) -> None:
        old_id = self.runtime.manifest.get("serviceId")
        old_epoch = self.runtime.manifest.get("serviceEpoch")
        self.runtime.stop()
        self.runtime.start()
        self.receipt.equal(
            "service ID persists across restart",
            "RC-4/durability",
            (
                old_id,
                self.runtime.manifest.get("serviceId"),
            ),
        )
        self.receipt.equal(
            "service epoch changes across restart",
            "API-8",
            (
                True,
                self.runtime.manifest.get("serviceEpoch") != old_epoch,
            ),
        )

    def metadata_session(self) -> tuple[JsonObject, str]:
        """Use only the Lead-approved peer-entry stand-in; no native turn is claimed."""
        qualification_path = self.runtime.target.peer_qualification
        if qualification_path is None:
            raise JourneyFailure(
                "G1 requires --peer-qualification from the actual producer-byte check"
            )
        qualification = object_value(json_value(qualification_path.read_bytes()))
        producer_module = Path(__file__).with_name("board_journey_peer.py")
        if (
            qualification.get("qualified") is not True
            or qualification.get("actualProducerConstructorAndSerializer") is not True
            or qualification.get("fixtureModuleSha256") != file_digest(producer_module)
        ):
            raise JourneyFailure(
                "G1 producer qualification does not apply to this fixture module"
            )
        repository = self.runtime.root / "context-repository"
        repository.mkdir()
        initialized = run_command(["git", "init", "--quiet"], repository)
        if initialized.returncode:
            raise JourneyFailure(
                f"cannot initialize owned context repository: {initialized.stderr}"
            )
        common = str((repository / ".git").resolve())
        peer_directory = self.runtime.directory / "absent-peer-registry"
        peer_directory.mkdir(mode=0o700, exist_ok=True)
        serialized, reference = qualified_peer_entry(
            repository, peer_directory / "unavailable.sock"
        )
        peer = object_value(json_value(serialized))
        session_id = string_value(peer.get("sessionId"))
        peer_path = peer_directory / f"{peer['pid']}.json"
        _ = peer_path.write_text(serialized, encoding="utf-8")
        peer_path.chmod(0o600)
        self.receipt.equal(
            "published metadata bytes match the qualified producer form",
            "G1 producer boundary",
            (
                reference.get("entrySha256"),
                file_digest(peer_path),
            ),
        )
        self.receipt.record(
            {
                "kind": "standIn",
                "scope": "G1 session metadata only",
                "path": str(peer_path),
                "livePid": peer["pid"],
                "producerReference": reference,
                "nativeSessionSocketAndTurn": "unverified until M4",
            }
        )
        service = string_value(self.runtime.manifest.get("serviceId"))
        session: JsonObject = {
            "endpoint": {"serviceId": service, "endpointId": "claude-local"},
            "sessionId": session_id,
        }
        _ = self.client.accepted(
            "repository_attach",
            {
                **write_header(self.owner),
                "workspaceId": self.workspace,
                "repository": {
                    "kind": "local",
                    "serviceId": service,
                    "commonDirectory": common,
                },
            },
        )
        self.grant_implementer({"kind": "session", "session": session})
        scope_fact = fixture_identity()
        _ = self.client.accepted(
            "fact_record",
            project_request(
                self.owner,
                self.project,
                {
                    "kind": "factRecord",
                    "fact": {
                        "factId": scope_fact,
                        "kind": {
                            "kind": "scopeAcknowledgement",
                            "fact": {
                                "session": session,
                                "subject": {
                                    "kind": "workstream",
                                    "subject": self.workstream,
                                },
                                "paths": ["src/verification.rs"],
                                "reason": "verified owned scope",
                            },
                        },
                        "links": [record_reference("workstream", self.workstream)],
                        "producerTime": datetime.datetime.now(
                            datetime.timezone.utc
                        ).isoformat(),
                        "anchor": None,
                    },
                },
            ),
        )
        return session, scope_fact

    def database_path(self, project: str | None = None) -> Path:
        path = self.runtime.directory / "projects" / f"{project or self.project}.sqlite"
        if not path.resolve().is_relative_to(self.runtime.root.resolve()):
            raise JourneyFailure("fault escaped the owned runtime root")
        return path

    def offline_rows(self, project: str, query: str) -> list[tuple[JsonValue, ...]]:
        if self.runtime.child is not None:
            raise JourneyFailure(
                "offline inspection requires closed native connections"
            )
        with contextlib.closing(
            sqlite3.connect(self.database_path(project))
        ) as connection:
            return t.cast(
                list[tuple[JsonValue, ...]], connection.execute(query).fetchall()
            )

    def corrupt_record(
        self, project: str, reference: JsonObject, change: JsonObject
    ) -> None:
        if self.runtime.child is not None:
            raise JourneyFailure("offline fault requires closed native connections")
        path = self.database_path(project)
        with contextlib.closing(sqlite3.connect(path)) as connection, connection:
            row = t.cast(
                tuple[str] | None,
                connection.execute(
                    "SELECT payload FROM record_identities WHERE kind=? AND id=?",
                    (
                        string_value(reference.get("kind")),
                        string_value(reference.get("id")),
                    ),
                ).fetchone(),
            )
            if row is None:
                raise JourneyFailure(f"fault target is absent: {reference}")
            payload = object_value(json_value(row[0]))
            stored = object_value(payload.get("record"))
            for key, value in change.items():
                stored[key] = value
            updated = connection.execute(
                "UPDATE record_identities SET payload=? WHERE kind=? AND id=?",
                (
                    json.dumps(payload),
                    string_value(reference.get("kind")),
                    string_value(reference.get("id")),
                ),
            )
            if updated.rowcount != 1:
                raise JourneyFailure("fault did not mutate exactly one owned row")
        self.receipt.record(
            {
                "kind": "fault",
                "mode": "offline",
                "database": str(path),
                "table": "record_identities",
                "reference": reference,
                "changedFields": change,
                "lifetime": "until this owned fixture is repaired or retired",
            }
        )
