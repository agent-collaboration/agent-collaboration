"""Challenge the independent publication oracle with durable-state counterexamples."""

import json

import pytest

from scripts.verification.board_upgrade_native_oracle import Checks
from scripts.verification.board_upgrade_publication_views import (
    UNKNOWN_SUMMARY,
    CentralReadback,
    verify_published,
)
from scripts.verification.board_upgrade_views import Object, UpgradeReport

PROJECT = "019f0000-0000-7000-8000-000000000501"


def publication_evidence() -> tuple[UpgradeReport, CentralReadback, Object]:
    manifest: Object = {
        "database": PROJECT,
        "position": {"epoch": 1, "sequence": 12},
        "schema": {"version": 1, "fingerprint": "fixture"},
        "tables": [],
    }
    provenance: Object = {
        "installation": "550e8400-e29b-41d4-a716-446655440000",
        "machine": "fixture",
    }
    header: Object = {
        "digest": "a" * 64,
        "declaredOwner": "owner",
        "provenance": provenance,
        "manifest": manifest,
        "pages": 1,
        "summary": UNKNOWN_SUMMARY,
    }
    report = UpgradeReport.model_validate(
        {
            "owner": "owner",
            "fleet": {"provenance": provenance, "records": []},
            "fleetDigest": "b" * 64,
            "collisions": [],
            "projects": [
                {
                    "project": PROJECT,
                    "workspace": "workspace",
                    "board": "board",
                    "title": "project",
                    "description": "",
                    "posts": 1,
                    "feedRows": 2,
                    "oldToNew": {},
                    "digest": "a" * 64,
                    "manifest": manifest,
                }
            ],
            "workspaceCounts": [],
            "boardCounts": [],
            "skipped": [],
            "refused": [],
        }
    )
    summary = json.dumps(
        {
            "database": PROJECT,
            "position": manifest["position"],
            "summary": UNKNOWN_SUMMARY,
        }
    )
    record = {
        "state": "following",
        "copy": {
            "holder": None,
            "fence": 1,
            "acked": manifest["position"],
            "schema": manifest["schema"],
        },
    }
    snapshot = CentralReadback.model_validate(
        {
            "copies": [
                {
                    "database": PROJECT,
                    "record": [json.dumps(record)],
                    "seed": [
                        json.dumps(
                            {
                                "seed": {"generation": 1},
                                "published": {"position": manifest["position"]},
                            }
                        )
                    ],
                    "summary": [summary],
                    "posts": ["1"],
                    "staged": ["0"],
                }
            ],
            "workflows": [
                json.dumps(
                    {
                        "importId": "019f0000-0000-7000-8000-000000000502",
                        "project": PROJECT,
                        "digest": "a" * 64,
                        "header": json.dumps(header),
                        "phase": json.dumps(
                            {"kind": "complete", "position": manifest["position"]}
                        ),
                    }
                )
            ],
            "directory": [summary],
        }
    )
    return report, snapshot, header


def test_declared_success_cannot_hide_a_seeding_copy() -> None:
    # Arrange: the CLI could claim Imported while durable state is still Seeding.
    report, snapshot, _ = publication_evidence()
    wrong = snapshot.model_copy(
        update={
            "copies": [
                snapshot.copies[0].model_copy(
                    update={"record": [json.dumps({"state": "seeding"})]}
                )
            ]
        }
    )
    # Act / Assert: the state oracle, independent of the response, rejects it.
    with pytest.raises(AssertionError, match="copy is actually Following"):
        verify_published(Checks(), report, wrong, {})


def test_no_seed_receipt_cannot_count_as_published() -> None:
    report, snapshot, _ = publication_evidence()
    wrong = snapshot.model_copy(
        update={"copies": [snapshot.copies[0].model_copy(update={"seed": []})]}
    )
    with pytest.raises(AssertionError, match="published seed singleton"):
        verify_published(Checks(), report, wrong, {})


def test_invented_summary_time_fails_even_when_projection_agrees() -> None:
    report, snapshot, _ = publication_evidence()
    summary = {
        "database": PROJECT,
        "position": {"epoch": 1, "sequence": 12},
        "summary": {
            "kind": "projectSummary",
            "value": {
                "status": None,
                "latestRecordAt": {"kind": "router", "value": "1970-01-01T00:00:00Z"},
            },
        },
    }
    wrong = snapshot.model_copy(
        update={
            "copies": [
                snapshot.copies[0].model_copy(update={"summary": [json.dumps(summary)]})
            ],
            "directory": [json.dumps(summary)],
        }
    )
    with pytest.raises(AssertionError, match="no invented imported time"):
        verify_published(Checks(), report, wrong, {})


def test_relabelled_report_keeps_the_first_physical_header() -> None:
    report, snapshot, first_header = publication_evidence()
    changed_project = report.projects[0].model_copy(
        update={
            "manifest": {
                **report.projects[0].manifest,
                "position": {"epoch": 1, "sequence": 13},
            }
        }
    )
    changed = report.model_copy(
        update={"owner": "second-importer", "projects": [changed_project]}
    )
    checks = Checks()
    verify_published(checks, changed, snapshot, {PROJECT: first_header})
    assert checks.records
    assert all(record.passed for record in checks.records)


def test_empty_readback_cannot_pass_the_publication_oracle() -> None:
    report, _, _ = publication_evidence()
    empty = CentralReadback(copies=[], workflows=[], directory=[])
    with pytest.raises(AssertionError, match="published copy identities"):
        verify_published(Checks(), report, empty, {})
