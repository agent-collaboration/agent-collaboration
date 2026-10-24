"""Creation-oracle controls; a rejected or different project cannot count green."""

import pytest

from scripts.verification.board_central_journey import creation_succeeded
from scripts.verification.board_journey_values import JsonObject

PROJECT = "019f0000-0000-7000-8000-000000000001"
OTHER = "019f0000-0000-7000-8000-000000000002"


@pytest.mark.parametrize("identity,expected", [(PROJECT, True), (OTHER, False)])
def test_pending_requires_the_created_project(identity: str, expected: bool) -> None:
    reply: JsonObject = {
        "result": {"kind": "projectCreationPending", "decision": {"project": identity}}
    }
    assert creation_succeeded(reply, PROJECT) == expected


@pytest.mark.parametrize("identity,expected", [(PROJECT, True), (OTHER, False)])
def test_accepted_requires_the_actual_project_effect(
    identity: str, expected: bool
) -> None:
    reply: JsonObject = {
        "result": {
            "kind": "decided",
            "decision": {
                "kind": "accepted",
                "effects": {"entries": [[{"kind": "project", "id": identity}, {}]]},
            },
        }
    }
    assert creation_succeeded(reply, PROJECT) == expected


def test_rejected_decision_is_not_a_successful_creation() -> None:
    reply: JsonObject = {
        "result": {"kind": "decided", "decision": {"kind": "rejected", "reasons": []}}
    }
    assert creation_succeeded(reply, PROJECT) is False
