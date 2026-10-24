"""Preparation never implies execution; unsafe targets fail before any request."""

import json
from pathlib import Path

import pytest
from pydantic import ValidationError

from scripts.verification.central_staging import prepare
from scripts.verification.central_staging_models import Credentials, Plan


def local_plan(temporary: Path) -> dict[str, object]:
    return {
        "repository": str(temporary / "repository"),
        "expected_head": "a" * 40,
        "evidence": str(temporary / "evidence"),
        "runtime_parent": "/private/tmp/central-g4-test-runtime",
        "artifact_root": str(temporary / "artifacts"),
        "target": {
            "kind": "localWorkerd",
            "node": "/synthetic/node",
            "miniflare": "/synthetic/miniflare.mjs",
            "native_probe": {"path": "/synthetic/probe", "sha256": "b" * 64},
            "bundle": "/synthetic/bundle",
            "bundle_js_sha256": "c" * 64,
            "bundle_wasm_sha256": "d" * 64,
        },
        "cases": ["access"],
    }


def test_preparation_accepts_unavailable_runtimes_and_never_claims_observation(
    tmp_path: Path,
) -> None:
    # No executable exists at the declared paths. Any accidental execution or
    # credential resolution would fail; preparation must still save a draft.
    values = local_plan(tmp_path.resolve())
    plan = Plan.model_validate(values)
    assert prepare(plan) == 0
    result = (plan.evidence / "preparation.txt").read_text()
    assert "OB7 UNOBSERVED" in result
    assert "no commands, credentials or requests" in result
    assert list(plan.evidence.glob("*.log")) == []
    assert plan.evidence.stat().st_mode & 0o077 == 0


def test_credentials_accept_environment_names_and_reject_values() -> None:
    assert (
        Credentials(
            machine="G4_A_MACHINE", access_id="G4_A_ID", access_secret="G4_A_SECRET"
        ).machine
        == "G4_A_MACHINE"
    )
    with pytest.raises(ValidationError):
        _ = Credentials(
            machine="Bearer secret material",
            access_id="G4_A_ID",
            access_secret="G4_A_SECRET",
        )


def test_account_state_and_checkout_evidence_are_refused(tmp_path: Path) -> None:
    values = local_plan(tmp_path.resolve())
    values["evidence"] = str(tmp_path.resolve() / "repository/evidence")
    with pytest.raises(ValidationError, match="private outside"):
        _ = Plan.model_validate(values)
    values = local_plan(tmp_path.resolve())
    values["runtime_parent"] = "/Users/example/.codex-router"
    with pytest.raises(ValidationError, match="runtime roots"):
        _ = Plan.model_validate(values)


def test_duplicate_group_cannot_inflate_observation_counts(tmp_path: Path) -> None:
    values = local_plan(tmp_path.resolve())
    values["cases"] = ["access", "access"]
    with pytest.raises(ValidationError, match="once"):
        _ = Plan.model_validate(values)


def test_unknown_target_or_retired_selector_cannot_run(tmp_path: Path) -> None:
    serialized = json.dumps(local_plan(tmp_path.resolve())).replace(
        '"localWorkerd"', '"oldStaging"'
    )
    with pytest.raises(ValidationError):
        _ = Plan.model_validate_json(serialized)
    values = local_plan(tmp_path.resolve())
    values["cases"] = ["staging_driver"]
    with pytest.raises(ValidationError):
        _ = Plan.model_validate(values)
