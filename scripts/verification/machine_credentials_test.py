"""Credential inputs produce one Bearer scheme at the real apparatus boundaries."""

import contextlib
import os
from collections.abc import Generator
from pathlib import Path

import pytest

from scripts.verification.board_journey_values import JsonObject
from scripts.verification.central_staging_cases import machine_headers
from scripts.verification.central_staging_models import Plan, StagingTarget
from scripts.verification.central_staging_upgrade import staging_upgrade_requests
from scripts.verification.cross_machine_agent import machine_session_fixture
from scripts.verification.cross_machine_plan import CredentialNames
from scripts.verification.cross_machine_retention import sender_session_headers
from scripts.verification.machine_credentials import normalize_machine_credential


@pytest.mark.parametrize(
    "credential", ["synthetic-machine", "Bearer synthetic-machine"]
)
def test_recipient_fixture_contains_the_complete_authorization_value(
    credential: str,
) -> None:
    names = CredentialNames(
        machine="UNIT_MACHINE",
        access_id="UNIT_ACCESS_ID",
        access_secret="UNIT_ACCESS_SECRET",
    )
    session: JsonObject = {
        "endpoint": "https://fixture.invalid/v1/session",
        "credentials": names.model_dump(),
        "peer": "peer",
        "receiver": "receiver",
        "relay": "relay",
    }
    observed = machine_session_fixture(
        session,
        {
            names.machine: credential,
            names.access_id: "fixture-id",
            names.access_secret: "fixture-secret",
        },
    )
    assert observed["credential"] == "Bearer synthetic-machine"
    assert observed["access"] == {
        "clientId": "fixture-id",
        "clientSecret": "fixture-secret",
    }


@pytest.mark.parametrize(
    "credential", ["synthetic-machine", "Bearer synthetic-machine"]
)
def test_stopped_sender_header_contains_one_bearer_scheme(credential: str) -> None:
    names = CredentialNames(
        machine="UNIT_MACHINE",
        access_id="UNIT_ACCESS_ID",
        access_secret="UNIT_ACCESS_SECRET",
    )
    headers = sender_session_headers(
        names,
        {
            names.machine: credential,
            names.access_id: "fixture-id",
            names.access_secret: "fixture-secret",
        },
    )
    assert headers == {
        "Authorization": "Bearer synthetic-machine",
        "CF-Access-Client-Id": "fixture-id",
        "CF-Access-Client-Secret": "fixture-secret",
    }


@pytest.mark.parametrize(
    "credential", ["synthetic-machine", "Bearer synthetic-machine"]
)
def test_normalization_preserves_exact_raw_material(credential: str) -> None:
    assert normalize_machine_credential(credential) == "synthetic-machine"


@pytest.mark.parametrize("credential", ["", "Bearer ", "bad\rvalue", "bad\nvalue"])
def test_normalization_rejects_empty_or_header_line_breaks(credential: str) -> None:
    with pytest.raises(ValueError, match="unavailable or malformed"):
        _ = normalize_machine_credential(credential)


@contextlib.contextmanager
def synthetic_environment(credential: str) -> Generator[None, None, None]:
    values = {
        "UNIT_G4_MACHINE": credential,
        "UNIT_G4_ACCESS_ID": "fixture-id",
        "UNIT_G4_ACCESS_SECRET": "fixture-secret",
        "UNIT_G4_SECOND_MACHINE": (
            "Bearer " if credential.startswith("Bearer ") else ""
        )
        + "synthetic-second",
        "UNIT_G4_SECOND_ACCESS_ID": "second-id",
        "UNIT_G4_SECOND_ACCESS_SECRET": "second-secret",
    }
    previous = {name: os.environ.get(name) for name in values}
    os.environ.update(values)
    try:
        yield
    finally:
        for name, value in previous.items():
            if value is None:
                _ = os.environ.pop(name, None)
            else:
                os.environ[name] = value


@pytest.fixture
def staging_target(tmp_path: Path) -> StagingTarget:
    installation = {
        "seed_root": str(tmp_path),
        "installation": "00000000-0000-4000-8000-000000000001",
        "peer": "00000000-0000-4000-8000-000000000002",
        "credentials": {
            "machine": "UNIT_G4_MACHINE",
            "access_id": "UNIT_G4_ACCESS_ID",
            "access_secret": "UNIT_G4_ACCESS_SECRET",
        },
    }
    target = StagingTarget.model_validate(
        {
            "kind": "staging",
            "origin": "https://fixture.invalid",
            "script": "unit-fixture",
            "deployed_version": "synthetic",
            "deployed_source_head": "a" * 40,
            "access_policy_id": "synthetic",
            "owner_token_environment": "UNIT_OWNER",
            "receiver": "00000000-0000-7000-8000-000000000003",
            "relay": "00000000-0000-7000-8000-000000000004",
            "installations": [
                installation,
                dict(
                    installation,
                    installation="00000000-0000-4000-8000-000000000005",
                    peer="00000000-0000-4000-8000-000000000006",
                    seed_root=str(tmp_path / "second"),
                    credentials={
                        "machine": "UNIT_G4_SECOND_MACHINE",
                        "access_id": "UNIT_G4_SECOND_ACCESS_ID",
                        "access_secret": "UNIT_G4_SECOND_ACCESS_SECRET",
                    },
                ),
            ],
            "alternate_origins": ["https://alternate.invalid"],
            "address_inventory": str(tmp_path / "not-opened.json"),
            "native_probe": {
                "path": str(tmp_path / "not-executed"),
                "sha256": "b" * 64,
            },
        }
    )
    return target


@pytest.mark.parametrize(
    "credential", ["synthetic-machine", "Bearer synthetic-machine"]
)
def test_ob7_http_headers_contain_one_bearer_scheme(
    credential: str, staging_target: StagingTarget
) -> None:
    with synthetic_environment(credential):
        headers = machine_headers(staging_target, 0)
    assert headers == {
        "Authorization": "Bearer synthetic-machine",
        "cf-access-client-id": "fixture-id",
        "cf-access-client-secret": "fixture-secret",
    }


@pytest.mark.parametrize(
    "credential", ["synthetic-machine", "Bearer synthetic-machine"]
)
def test_ob7_native_probe_requests_bind_each_credential_once(
    credential: str, staging_target: StagingTarget, tmp_path: Path
) -> None:
    plan = Plan(
        repository=tmp_path / "repository",
        expected_head="a" * 40,
        evidence=tmp_path / "evidence",
        runtime_parent=Path("/private/tmp/unit-machine-normalization"),
        artifact_root=tmp_path / "unused-artifacts",
        target=staging_target,
        cases=["access"],
    )
    with synthetic_environment(credential):
        crossed, invalid_access = staging_upgrade_requests(plan)
    assert crossed.bearer == "Bearer synthetic-second"
    assert crossed.access_id == "fixture-id"
    assert crossed.access_secret == "fixture-secret"
    assert invalid_access.bearer == "Bearer synthetic-machine"
    assert invalid_access.access_id == "invalid-ob7-client"
    assert invalid_access.access_secret == "invalid-ob7-secret"
    assert (
        crossed.endpoint
        == invalid_access.endpoint
        == "https://fixture.invalid/v1/session"
    )
