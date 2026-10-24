"""Apparatus sensitivity: unsafe targets and false publication evidence refuse."""

import contextlib
import hashlib
import http.client
import http.server
import json
import sqlite3
import threading
import typing as t
from pathlib import Path

import pytest
from pydantic import ValidationError

from scripts.verification.board_upgrade_external_cases import verify_owner
from scripts.verification.board_upgrade_external_fixture import clone_fixtures
from scripts.verification.board_upgrade_external_plan import ExternalPlan
from scripts.verification.board_upgrade_external_target import (
    qualify_external_target,
    require_short_socket_path,
)
from scripts.verification.board_upgrade_external_transport import (
    MachineReadback,
    OwnerEndpoint,
    OwnerProxy,
)
from scripts.verification.board_upgrade_native_oracle import Checks
from scripts.verification.board_upgrade_source import source_rows
from scripts.verification.board_upgrade_views import (
    Object,
    UpgradeReport,
    json_object,
    text_field,
)


def declared_plan(tmp_path: Path) -> ExternalPlan:
    return ExternalPlan(
        repository=tmp_path,
        expected_head="a" * 40,
        artifact_receipt=tmp_path / "artifacts.json",
        endpoint="https://staging.example.test",
        authentication="ownerAccess",
        runtime_parent=tmp_path,
        output_directory=tmp_path / "output",
        machine_environment={
            key: "OB5_" + key.upper()
            for key in [
                "installation",
                "peer",
                "receiver",
                "relay",
                "credential",
                "access_client_id",
                "access_client_secret",
            ]
        },
    )


@pytest.mark.parametrize(
    "endpoint",
    [
        "http://staging.example.test",
        "https://user:secret@staging.example.test",
        "https://staging.example.test/path",
        "https://staging.example.test?token=secret",
        "https://staging.example.test:8787",
    ],
)
def test_external_target_refuses_credential_urls_and_protected_surfaces(
    tmp_path: Path, endpoint: str
) -> None:
    plan = declared_plan(tmp_path).model_copy(update={"endpoint": endpoint})
    with pytest.raises(ValueError):
        plan.validate_endpoint()


def test_external_target_accepts_https_but_not_a_different_machine_origin(
    tmp_path: Path,
) -> None:
    plan = declared_plan(tmp_path)
    plan.validate_endpoint()
    with pytest.raises(ValueError, match="same declared origin"):
        plan.model_copy(
            update={"machine_endpoint": "https://other.example.test/v1/session"}
        ).validate_endpoint()


def test_credential_values_cannot_be_put_in_the_plan(tmp_path: Path) -> None:
    values = declared_plan(tmp_path).model_dump()
    values["machine_environment"] = {"credential": "literal credential bytes"}
    with pytest.raises(ValidationError):
        _ = ExternalPlan.model_validate(values)


@pytest.mark.parametrize("wrong", ["pin", "sha256", "sourcePin", "sourcePath"])
def test_frozen_target_refuses_wrong_build_receipt_before_execution(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, wrong: str
) -> None:
    repository = tmp_path / "checkout"
    repository.mkdir()
    artifacts = tmp_path / "bin"
    artifacts.mkdir(mode=0o700)
    entries: list[Object] = []
    for name in [
        "codex-router-board-upgrade",
        "board-upgrade-native-observer",
        "board-debug-runtime",
        "agent-collaboration",
    ]:
        binary = artifacts / name
        contents = b"unexecutable apparatus fixture"
        _ = binary.write_bytes(contents)
        normal = (
            "examples/board-debug-runtime" if name == "board-debug-runtime" else name
        )
        entries.append(
            {
                "path": str(binary),
                "sourcePath": str(repository / "target/debug" / normal),
                "sha256": hashlib.sha256(contents).hexdigest(),
                "sourcePin": "a" * 40,
                "identifier": "dev.shravansunder." + name + ".debug",
                "team": "974QD84WVC",
                "strictSignatureVerified": True,
            }
        )
    receipt = {
        "pin": "a" * 40,
        "artifactDirectory": str(artifacts),
        "artifacts": entries,
    }
    if wrong == "pin":
        receipt["pin"] = "b" * 40
    else:
        entries[0][wrong] = (
            "b" * 64
            if wrong == "sha256"
            else "b" * 40
            if wrong == "sourcePin"
            else str(repository / "wrong-product")
        )
    path = tmp_path / "receipt.json"
    _ = path.write_text(json.dumps(receipt))
    plan = declared_plan(tmp_path).model_copy(
        update={"repository": repository, "artifact_receipt": path}
    )

    # This check isolates artifact admission. The real journey independently
    # checks git source and strict signatures; these wrong bytes never execute.
    def source_checked_separately(_plan: ExternalPlan) -> None:
        return

    monkeypatch.setattr(
        "scripts.verification.board_upgrade_external_target.verify_external_source",
        source_checked_separately,
    )
    with pytest.raises(ValueError, match="frozen artifact receipt|declared binary"):
        _ = qualify_external_target(plan)


def test_socket_admission_counts_encoded_bytes_before_host_startup() -> None:
    require_short_socket_path(Path("/private/tmp/ob5/service"))
    with pytest.raises(ValueError, match="macOS limit"):
        require_short_socket_path(Path("/private/tmp/" + "é" * 50))


def test_fixture_remap_preserves_collision_and_reference_integrity(
    tmp_path: Path,
) -> None:
    repository = Path(__file__).resolve().parents[2]
    original = source_rows(
        repository / "crates/board-legacy-upgrade/tests/fixtures/legacy-complete.sqlite"
    )
    first, collision = clone_fixtures(
        repository, tmp_path / "copies", ["legacy-complete.sqlite", "collision.sqlite"]
    )
    cloned = source_rows(first)
    assert len(cloned.tables["board_messages"]) == len(
        original.tables["board_messages"]
    )
    assert {
        text_field(row, "topic_id") for row in cloned.tables["board_topics"]
    }.isdisjoint(
        {text_field(row, "topic_id") for row in original.tables["board_topics"]}
    )
    assert (
        len(
            {text_field(row, "topic_id") for row in cloned.tables["board_messages"]}
            - {text_field(row, "topic_id") for row in cloned.tables["board_topics"]}
        )
        == 0
    )
    collision_rows = source_rows(collision)
    assert {text_field(row, "name") for row in cloned.tables["board_projects"]} & {
        text_field(row, "name") for row in collision_rows.tables["board_projects"]
    }
    for path in [first, collision]:
        with contextlib.closing(
            sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
        ) as connection:
            assert connection.execute("PRAGMA foreign_key_check").fetchall() == []


def test_owner_proxy_injects_only_its_declared_auth_and_does_not_follow_redirects() -> (
    None
):
    received: list[dict[str, str]] = []

    class Redirect(http.server.BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            _ = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            received.append(dict(self.headers))
            self.send_response(302)
            self.send_header("Location", "http://127.0.0.1:1/admin/v1/import/fleet")
            self.send_header("Content-Length", str(len(b'{"kind":"redirect"}')))
            self.end_headers()
            _ = self.wfile.write(b'{"kind":"redirect"}')

        @t.override
        def log_message(self, format: str, *args: object) -> None:
            return

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Redirect)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    proxy = OwnerProxy(
        OwnerEndpoint(
            f"http://127.0.0.1:{server.server_port}",
            "cf-access-token",
            "synthetic-owner-token",
        )
    )
    try:
        port = int(proxy.url.rsplit(":", 1)[-1])
        with contextlib.closing(
            http.client.HTTPConnection("127.0.0.1", port, timeout=5)
        ) as connection:
            connection.request(
                "POST",
                "/admin/v1/import/fleet",
                b"{}",
                {
                    "Cf-Access-Jwt-Assertion": "untrusted-downstream",
                    "Authorization": "Bearer untrusted-downstream",
                },
            )
            response = connection.getresponse()
            assert response.status == 302
            assert json.loads(response.read()) == {"kind": "redirect"}
        assert len(received) == 1
        headers = {key.lower(): value for key, value in received[0].items()}
        assert headers["cf-access-token"] == "synthetic-owner-token"
        assert "cf-access-jwt-assertion" not in headers
        assert "authorization" not in headers
        assert proxy.failure is None
    finally:
        proxy.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def test_publication_checker_cannot_accept_unfinished_workflow() -> None:
    project = "019a0000-0000-7000-8000-000000000015"
    report = UpgradeReport.model_validate(
        {
            "owner": "verification-owner",
            "fleet": {"provenance": {}, "records": []},
            "fleetDigest": "digest",
            "collisions": [],
            "projects": [
                {
                    "project": project,
                    "workspace": project,
                    "board": project,
                    "title": "project",
                    "description": "",
                    "posts": 0,
                    "feedRows": 1,
                    "oldToNew": {},
                    "digest": "content",
                    "manifest": {"position": {"epoch": 1, "sequence": 59}},
                }
            ],
            "workspaceCounts": [],
            "boardCounts": [],
            "skipped": [],
            "refused": [],
        }
    )
    state: Object = {
        project: {
            "import": {
                "kind": "progress",
                "value": {
                    "header": {"digest": "content"},
                    "phase": {
                        "kind": "published",
                        "position": {"epoch": 1, "sequence": 59},
                    },
                },
            },
            "health": {},
        }
    }
    checks = Checks()
    with pytest.raises(AssertionError, match="workflow completed"):
        verify_owner(checks, report, state)
    assert checks.records[-1].passed is False


@t.final
class ScriptedSocket:
    """Literal wire replies exercise the decoder, without claiming a product run."""

    def __init__(self, body: Object, *, wrong_frame: bool = False) -> None:
        self.body = body
        self.wrong_frame = wrong_frame
        self.frame = ""

    def send(self, message: str | bytes) -> None:
        request = json_object.validate_json(message)
        self.frame = text_field(json_object.validate_python(request["value"]), "frame")

    def recv(self, timeout: float) -> str:
        assert timeout > 0
        return json.dumps(
            {
                "kind": "answer",
                "value": {
                    "frame": "wrong-frame" if self.wrong_frame else self.frame,
                    "body": self.body,
                },
            }
        )

    def close(self) -> None:
        return


@pytest.mark.parametrize("wrong_frame", [False, True])
def test_machine_answers_must_correlate_to_the_actual_frame(wrong_frame: bool) -> None:
    machine = MachineReadback.__new__(MachineReadback)
    machine.socket = ScriptedSocket(
        {"kind": "replication", "value": {"kind": "copyStatus"}},
        wrong_frame=wrong_frame,
    )
    if wrong_frame:
        with pytest.raises(ValueError, match="correlation"):
            _ = machine.operation({"kind": "replication"})
    else:
        assert machine.operation({"kind": "replication"}) == {
            "kind": "replication",
            "value": {"kind": "copyStatus"},
        }


@pytest.mark.parametrize("stalled", [False, True])
def test_snapshot_cursor_must_advance_or_end(stalled: bool) -> None:
    machine = MachineReadback.__new__(MachineReadback)
    page: Object = {
        "table": "posts",
        "schemaSteps": [],
        "rows": [],
        "next": {
            "position": {"epoch": 1, "sequence": 1},
            "table": "posts",
            "afterKey": [],
        }
        if stalled
        else None,
    }
    machine.socket = ScriptedSocket(
        {
            "kind": "replication",
            "value": {"kind": "snapshotPage", "value": {"kind": "page", "value": page}},
        }
    )
    if stalled:
        with pytest.raises(ValueError, match="cursor stalled"):
            _ = machine.pages("019a0000-0000-7000-8000-000000000015")
    else:
        assert machine.pages("019a0000-0000-7000-8000-000000000015") == [page]
