"""Publish a private synthetic unheld project through the real importer."""

import contextlib
import os
import sqlite3
import subprocess
import typing as t

from .board_journey_runtime import JourneyReceipt
from .board_journey_values import field, file_digest, fixture_identity, json_value
from .board_upgrade_external_fixture import clone_fixtures
from .board_upgrade_external_transport import OwnerProxy
from .cross_machine_central import MatrixCentral
from .cross_machine_lifecycle import Installation
from .cross_machine_plan import BinaryBinding


def import_unheld(
    installation: Installation,
    central: MatrixCentral,
    binary: BinaryBinding,
    receipt: JourneyReceipt,
) -> str:
    receipt.equal(
        "declared importer hash before use",
        "OB4 target",
        (binary.sha256, file_digest(binary.path)),
    )
    source = clone_fixtures(
        installation.repository,
        receipt.directory / "unheld-source",
        ["legacy-complete.sqlite"],
    )[0]
    project = fixture_identity()
    with contextlib.closing(sqlite3.connect(source)) as connection, connection:
        board = t.cast(
            tuple[str] | None,
            connection.execute("SELECT board_id FROM board_topics LIMIT 1").fetchone(),
        )
        if board is None:
            raise ValueError("synthetic fixture has no containing board")
        _ = connection.execute(
            "INSERT INTO board_topics(topic_id,board_id,name,description) VALUES(?,?,?,?)",
            (
                project,
                board[0],
                "unheld " + project,
                "Synthetic OB4 private project",
            ),
        )
    source.chmod(0o444)
    before = file_digest(source)
    temporary = receipt.directory / "import-temporary"
    temporary.mkdir(mode=0o700)
    report = receipt.directory / "unheld-import-report.json"
    assertion = receipt.directory / "unheld-proxy-assertion"
    _ = assertion.write_text("local-driver-transport-only")
    assertion.chmod(0o600)
    proxy = OwnerProxy(central.owner)
    command = [
        str(binary.path),
        "--source",
        str(source),
        "--router-directory",
        str(installation.plan.task_directory / "service"),
        "--report",
        str(report),
        "--owner-human-id",
        "m2-verification-lead",
        "--machine-label",
        "verification source",
        "--central-url",
        proxy.url,
    ]
    environment = dict(
        os.environ,
        TMPDIR=str(temporary),
        CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE=str(assertion),
    )
    try:
        with (
            (receipt.directory / "unheld-import.stdout").open("wb") as stdout,
            (receipt.directory / "unheld-import.stderr").open("wb") as stderr,
        ):
            result = subprocess.run(
                command,
                cwd=installation.repository,
                env=environment,
                stdout=stdout,
                stderr=stderr,
                timeout=120,
                check=False,
            )
    finally:
        proxy.close()
        receipt.record(
            {
                "kind": "importProductHttp",
                "requests": list(proxy.responses),
                "proxyFailure": proxy.failure,
            }
        )
    receipt.equal(
        "actual importer publishes unheld fixture",
        "FL-15 prerequisite",
        (0, result.returncode),
    )
    receipt.equal(
        "unheld fixture source stays sealed",
        "FL-15 prerequisite",
        (before, file_digest(source)),
    )
    receipt.equal(
        "unheld fixture reports actual upload",
        "FL-15 prerequisite",
        ("uploaded", field(json_value(report.read_bytes()), "kind")),
    )
    receipt.record(
        {
            "kind": "unheldFixture",
            "project": project,
            "sourceSha256": before,
            "report": str(report),
        }
    )
    return project
