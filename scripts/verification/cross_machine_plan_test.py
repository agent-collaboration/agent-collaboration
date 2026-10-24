"""Confinement and activation controls, never product-delivery evidence."""

import hashlib
import json
import typing as t
import uuid
from pathlib import Path
from types import SimpleNamespace

import pytest
from pydantic import ValidationError

from scripts.verification.board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    field,
    json_value,
    object_value,
)
from scripts.verification.board_upgrade_external_transport import MachineReadback
from scripts.verification.cross_machine_agent import (
    cli_envelope,
    remote_disk_within_budget,
    serve_installation,
    verify_binary,
)
from scripts.verification.cross_machine_plan import (
    CentralSession,
    CentralTarget,
    CredentialNames,
    CrossMachinePlan,
    InstallationPlan,
)
from scripts.verification.cross_machine_recipients import (
    Recipient,
    agent_output,
    begin_observation,
    recipient_turn_matches,
    validate_observation,
)


def test_initial_provider_observation_accepts_the_public_bounded_gap_flag() -> None:
    target: JsonObject = {
        "endpoint": {"serviceId": "fixture-service", "endpointId": "claude-local"},
        "sessionId": "fixture-session",
    }
    observation: JsonObject = {
        "target": target,
        "generation": {"serviceEpoch": "fixture-epoch", "generation": 1},
        "attached": True,
        "epoch": 0,
        "continuationGap": True,
        "endReason": "deadlineReached",
        "events": [
            {
                "sequence": 1,
                "event": {"kind": "stateChanged", "state": {"kind": "idle"}},
            }
        ],
    }

    def read(_tool: str, _arguments: JsonObject) -> JsonObject:
        return observation

    receiver_object: object = SimpleNamespace(
        target=target,
        confirm_model=lambda: None,
        installation=SimpleNamespace(read=read),
    )
    receiver = t.cast(Recipient, t.cast(object, receiver_object))
    assert begin_observation(receiver) == observation


@pytest.mark.parametrize(
    "invalid",
    [
        None,
        "resync",
        "oversized",
        "epoch",
        "generation",
        "target",
        "detached",
        "duplicates",
        "reversed",
    ],
)
def test_provider_history_accepts_compacted_positions_but_rejects_actual_gaps(
    invalid: str | None,
) -> None:
    target: JsonObject = {
        "endpoint": {"serviceId": "fixture-service", "endpointId": "claude-local"},
        "sessionId": "fixture-session",
    }
    baseline: JsonObject = {
        "epoch": 0,
        "generation": {"serviceEpoch": "fixture-epoch", "generation": 1},
    }
    observation: JsonObject = {
        "target": target,
        "generation": baseline["generation"],
        "attached": True,
        "epoch": 0,
        "continuationGap": True,
        "endReason": "deadlineReached",
        "events": [
            {"sequence": 2, "event": {"kind": "turnStarted"}},
            {"sequence": 5, "event": {"kind": "turnEnded"}},
        ],
    }
    match invalid:
        case "resync":
            observation["events"] = [{"kind": "resyncRequired"}]
        case "oversized":
            observation["events"] = [{"kind": "eventTooLarge", "sequence": 2}]
        case "epoch":
            observation["epoch"] = 1
        case "generation":
            observation["generation"] = {"serviceEpoch": "changed", "generation": 1}
        case "target":
            observation["target"] = {**target, "sessionId": "another-session"}
        case "detached":
            observation["attached"] = False
        case "duplicates":
            observation["events"] = [
                {"sequence": 2, "event": {"kind": "turnStarted"}},
                {"sequence": 2, "event": {"kind": "turnEnded"}},
            ]
        case "reversed":
            observation["events"] = [
                {"sequence": 5, "event": {"kind": "turnStarted"}},
                {"sequence": 2, "event": {"kind": "turnEnded"}},
            ]
        case _:
            pass
    if invalid:
        with pytest.raises(JourneyFailure):
            _ = validate_observation(observation, target, baseline=baseline, after=1)
    else:
        assert (
            validate_observation(observation, target, baseline=baseline, after=1) == 5
        )


@pytest.mark.parametrize("saved", [None, "019a0000-0000-7000-8000-000000000015"])
def test_machine_hello_preserves_explicit_saved_session_or_creates_a_fresh_one(
    monkeypatch: pytest.MonkeyPatch, saved: str | None
) -> None:
    sent: list[JsonObject] = []

    class Stream:
        def close(self) -> None:
            return

    class Wire:
        def send(self, message: str | bytes) -> None:
            sent.append(object_value(json_value(message)))

        def recv(self, timeout: float) -> str:
            assert timeout > 0
            return json.dumps(
                {
                    "kind": "welcome",
                    "value": {"sessionId": field(sent[-1], "value", "sessionId")},
                }
            )

        def close(self) -> None:
            return

    def connect(*_args: object, **_keywords: object) -> Wire:
        return Wire()

    def stream(*_args: object, **_keywords: object) -> Stream:
        return Stream()

    def module(_name: str) -> SimpleNamespace:
        return SimpleNamespace(connect=connect)

    monkeypatch.setattr(
        "scripts.verification.board_upgrade_external_transport.socket.create_connection",
        stream,
    )
    monkeypatch.setattr(
        "scripts.verification.board_upgrade_external_transport.importlib.import_module",
        module,
    )
    if saved is None:
        machine = MachineReadback("http://127.0.0.1:12345", {})
    else:
        machine = MachineReadback("http://127.0.0.1:12345", {}, session_id=saved)
    try:
        session = field(sent[0], "value", "sessionId")
        if saved is not None:
            assert session == saved
        else:
            assert isinstance(session, str)
            assert str(uuid.UUID(session)) == session
            assert session != "019a0000-0000-7000-8000-000000000015"
            second = MachineReadback("http://127.0.0.1:12345", {})
            try:
                assert field(sent[-1], "value", "sessionId") != session
            finally:
                second.close()
    finally:
        machine.close()


def installation(root: Path, **changes: object) -> InstallationPlan:
    binary = {
        "path": str(root / "bin/host"),
        "sha256": "a" * 64,
        "source_head": "b" * 40,
    }
    values: dict[str, object] = {
        "name": "first",
        "task_directory": str(root),
        "temporary_parent": str(root.parent),
        "launcher": binary,
        "cli": binary,
        "port": 0,
    }
    values.update(changes)
    return InstallationPlan.model_validate(values)


@pytest.mark.parametrize("port", [8787, 8788, 43127])
def test_protected_ports_are_refused_before_launch(tmp_path: Path, port: int) -> None:
    with pytest.raises(ValidationError, match="protected"):
        _ = installation(tmp_path / "task", port=port)


@pytest.mark.parametrize(
    "value",
    ["literal-secret-value", "Bearer secret", "op://private/item", "NAME=value"],
)
def test_plan_refuses_secret_values_instead_of_environment_names(value: str) -> None:
    with pytest.raises(ValidationError, match="environment names"):
        _ = CredentialNames(
            machine=value, access_id="ACCESS_ID", access_secret="ACCESS_SECRET"
        )


def test_named_credential_selectors_are_preserved() -> None:
    names = CredentialNames(
        machine="OB4_A_MACHINE",
        access_id="OB4_A_ACCESS_ID",
        access_secret="OB4_A_ACCESS_SECRET",
    )
    assert names.machine == "OB4_A_MACHINE"


@pytest.mark.parametrize(
    "endpoint",
    [
        "https://user:secret@example.invalid",
        "https://example.invalid/admin/v1",
        "https://example.invalid?token=secret",
        "https://example.invalid:8787",
        "http://example.invalid",
    ],
)
def test_owner_matrix_origin_rejects_credentials_paths_and_insecure_access(
    endpoint: str,
) -> None:
    with pytest.raises(ValidationError):
        _ = CentralTarget.model_validate(
            {
                "endpoint": endpoint,
                "authentication": {
                    "kind": "ownerAccess",
                    "token_environment": "OB4_OWNER_TOKEN",
                },
            }
        )


def test_fixture_owner_assertion_cannot_target_an_external_origin() -> None:
    with pytest.raises(ValidationError, match="loopback"):
        _ = CentralTarget.model_validate(
            {
                "endpoint": "https://example.invalid",
                "authentication": {
                    "kind": "localFixture",
                    "assertion_file": "/private-fixture",
                },
            }
        )


def test_matrix_cannot_activate_without_both_enrolled_sessions(tmp_path: Path) -> None:
    with pytest.raises(ValidationError, match="both enrolled"):
        _ = CrossMachinePlan.model_validate(
            {
                "repository": str(tmp_path),
                "output_directory": str(tmp_path / "proof"),
                "installations": [
                    installation(tmp_path / "a").model_dump(),
                    installation(tmp_path / "b", name="second").model_dump(),
                ],
                "mode": "matrix",
                "central_target": {
                    "endpoint": "https://example.invalid",
                    "authentication": {
                        "kind": "ownerAccess",
                        "token_environment": "OB4_OWNER_TOKEN",
                    },
                },
                "allow_external_central": True,
            }
        )


@pytest.mark.parametrize(
    "endpoint",
    [
        "https://example.invalid/other",
        "https://example.invalid/v1/session?token=value",
        "https://user:secret@example.invalid/v1/session",
        "http://127.0.0.1:8788/v1/session",
    ],
)
def test_machine_session_cannot_escape_the_declared_upgrade_route(
    endpoint: str,
) -> None:
    with pytest.raises(ValidationError):
        _ = CentralSession.model_validate(
            {
                "endpoint": endpoint,
                "credentials": {
                    "machine": "MACHINE",
                    "access_id": "ACCESS_ID",
                    "access_secret": "ACCESS_SECRET",
                },
                "peer": "peer",
                "receiver": "receiver",
                "relay": "relay",
            }
        )


@pytest.mark.parametrize(
    "host", ["-oProxyCommand=anything", "host with-space", "host;command"]
)
def test_ssh_destination_cannot_be_an_option_or_shell_command(
    tmp_path: Path, host: str
) -> None:
    with pytest.raises(ValidationError, match="literal destination"):
        _ = installation(tmp_path / "task", ssh_host=host)


def test_remote_binary_cannot_escape_the_task_root(tmp_path: Path) -> None:
    binary = {"path": "/usr/bin/host", "sha256": "a" * 64, "source_head": "b" * 40}
    with pytest.raises(ValidationError, match="remote artifacts"):
        _ = installation(tmp_path / "task", ssh_host="sunclaw", launcher=binary)


def test_readiness_mode_cannot_implicitly_open_a_central(tmp_path: Path) -> None:
    first = installation(tmp_path / "a")
    second = installation(
        tmp_path / "b",
        name="second",
        session={
            "endpoint": "https://example.invalid/v1/session",
            "credentials": {
                "machine": "MACHINE",
                "access_id": "ACCESS_ID",
                "access_secret": "ACCESS_SECRET",
            },
            "peer": "peer",
            "receiver": "receiver",
            "relay": "relay",
        },
    )
    with pytest.raises(ValidationError, match="must not contact a central"):
        _ = CrossMachinePlan(
            repository=tmp_path,
            output_directory=tmp_path / "proof",
            installations=(first, second),
            mode="readiness",
        )


def test_two_names_do_not_make_one_root_two_installations(tmp_path: Path) -> None:
    first = installation(tmp_path / "a")
    second = first.model_copy(update={"name": "second"})
    with pytest.raises(ValidationError, match="distinct"):
        _ = CrossMachinePlan(
            repository=tmp_path,
            output_directory=tmp_path / "proof",
            installations=(first, second),
            mode="readiness",
        )


def test_wrong_artifact_digest_fails_before_code_sign_or_launch(tmp_path: Path) -> None:
    path = tmp_path / "not-a-host"
    _ = path.write_bytes(b"known wrong artifact")
    with pytest.raises(ValueError, match="digest changed"):
        _ = verify_binary(
            {"path": str(path), "sha256": hashlib.sha256(b"right artifact").hexdigest()}
        )


def test_agent_itself_rejects_protected_port_without_trusting_plan_validation(
    tmp_path: Path,
) -> None:
    with pytest.raises(ValueError, match="protected"):
        _ = serve_installation(
            t.cast(
                JsonObject,
                {
                    "task_directory": str(tmp_path / "task"),
                    "temporary_parent": str(tmp_path),
                    "port": 8787,
                },
            )
        )


@pytest.mark.parametrize("kind", ["userMessage", "toolCall", "notice"])
def test_recipient_input_or_acceptance_echo_is_not_agent_output(kind: str) -> None:
    observation: JsonObject = {
        "events": [
            {
                "sequence": 1,
                "event": {
                    "kind": "itemUpdated",
                    "item": {"kind": {"kind": kind}, "text": "OB4_ACK_marker"},
                },
            }
        ]
    }
    assert agent_output(observation) == ""


def test_actual_agent_message_can_supply_the_recipient_marker() -> None:
    observation: JsonObject = {
        "events": [
            {
                "sequence": 1,
                "event": {
                    "kind": "itemUpdated",
                    "item": {
                        "kind": {"kind": "agentMessage"},
                        "text": "OB4_ACK_marker",
                    },
                },
            }
        ]
    }
    assert agent_output(observation) == "OB4_ACK_marker"


def test_cli_progress_does_not_hide_the_final_applied_error() -> None:
    contents = (
        '{"kind":"conversationCreateStarted"}\n{"kind":"error","effect":"applied"}\n'
    )
    assert cli_envelope(contents) == {"kind": "error", "effect": "applied"}


@pytest.mark.parametrize(
    "contents", ['{"kind":"conversationCreateStarted"}', "ok", '{"kind":"ready"}']
)
def test_cli_diagnostics_cannot_become_success(contents: str) -> None:
    assert cli_envelope(contents) is None


def test_pretty_printed_cli_result_is_an_envelope() -> None:
    assert cli_envelope('{\n "kind": "result",\n "result": {}\n}') == {
        "kind": "result",
        "result": {},
    }


@pytest.mark.parametrize(
    "free,footprint,expected",
    [(14, 1, False), (15, 5, True), (20, 6, False), (20, 5, True)],
)
def test_sunclaw_runtime_budget_catches_low_space_and_oversized_task(
    free: int,
    footprint: int,
    expected: bool,
) -> None:
    assert (
        remote_disk_within_budget(
            {"freeBytes": free * 1024**3, "footprintBytes": footprint * 1024**3}
        )
        == expected
    )


def provider(configuration: Path) -> dict[str, object]:
    executable = {"path": "/usr/bin/true", "sha256": "a" * 64, "version": "control"}
    return {
        "adapter": executable,
        "claude": executable,
        "configuration_directory": str(configuration),
    }


def test_two_owned_hosts_can_share_one_declared_auth_directory(tmp_path: Path) -> None:
    first = installation(tmp_path / "a", recipient_provider=provider(tmp_path / "auth"))
    second = installation(
        tmp_path / "b", name="second", recipient_provider=provider(tmp_path / "auth")
    )
    plan = CrossMachinePlan(
        repository=tmp_path,
        output_directory=tmp_path / "proof",
        installations=(first, second),
        mode="readiness",
    )
    assert (
        plan.installations[0].recipient_provider
        == plan.installations[1].recipient_provider
    )


def test_normal_claude_home_cannot_be_used_as_recipient_auth(tmp_path: Path) -> None:
    with pytest.raises(ValidationError, match="private task path"):
        _ = installation(
            tmp_path / "a", recipient_provider=provider(tmp_path / ".claude")
        )


def test_remote_auth_cannot_escape_the_single_authorized_task_root(
    tmp_path: Path,
) -> None:
    root = tmp_path / "task"
    binary = {
        "path": str(root / "bin/host"),
        "sha256": "a" * 64,
        "source_head": "b" * 40,
        "transfer_source": str(tmp_path / "copy"),
    }
    with pytest.raises(ValidationError, match="one task root"):
        _ = installation(
            root,
            ssh_host="sunclaw",
            launcher=binary,
            cli=binary,
            recipient_provider=provider(tmp_path / "another-task/auth"),
        )


def test_remote_upload_cannot_guess_its_source_from_shared_target(
    tmp_path: Path,
) -> None:
    with pytest.raises(ValidationError, match="exact local source"):
        _ = installation(tmp_path / "task", ssh_host="sunclaw")


@pytest.mark.parametrize(
    "incoming,output,ended,expected",
    [
        ("exact-item", "", True, False),
        ("other-item", "acknowledged", True, False),
        ("exact-item", "acknowledged", False, False),
        ("exact-item", "acknowledged", True, True),
    ],
)
def test_notification_needs_its_exact_recipient_input_and_completed_model_turn(
    incoming: str,
    output: str,
    ended: bool,
    expected: bool,
) -> None:
    events: list[JsonValue] = [
        {
            "sequence": 1,
            "event": {
                "kind": "turnStarted",
                "turnId": "literal-turn",
                "inputId": incoming,
            },
        },
        {
            "sequence": 2,
            "event": {
                "kind": "itemUpdated",
                "item": {"kind": {"kind": "agentMessage"}, "text": output},
            },
        },
    ]
    if ended:
        events.append(
            {
                "sequence": 3,
                "event": {
                    "kind": "turnEnded",
                    "turnId": "literal-turn",
                    "outcome": {"kind": "ended"},
                },
            }
        )
    assert recipient_turn_matches(events, "exact-item", notification=True) == expected
