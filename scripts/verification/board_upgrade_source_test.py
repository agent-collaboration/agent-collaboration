"""Literal PD2 interval and cutoff cases for the independent source oracle."""

from scripts.verification.board_upgrade_source import (
    activity_mapping,
    canonical_time,
    interval_history,
)
from scripts.verification.board_upgrade_views import Object, SourceRows


def history_source() -> SourceRows:
    events: list[Object] = [
        {
            "activity_sequence": 1,
            "topic_id": "project",
            "kind": "mainMessageCreated",
            "message_id": "thread",
            "actor_key": "actor-a",
            "root_id": None,
        },
        {
            "activity_sequence": 2,
            "topic_id": "project",
            "kind": "participantJoined",
            "message_id": None,
            "actor_key": "actor-a",
            "root_id": "thread",
            "participant_key": "actor-a",
            "participant_role": "orchestrator",
        },
        {
            "activity_sequence": 3,
            "topic_id": "project",
            "kind": "participantJoined",
            "message_id": None,
            "actor_key": "actor-b",
            "root_id": "thread",
            "participant_key": "actor-b",
            "participant_role": "advisor",
        },
        {
            "activity_sequence": 4,
            "topic_id": "project",
            "kind": "participantJoined",
            "message_id": None,
            "actor_key": "actor-b",
            "root_id": "thread",
            "participant_key": "actor-b",
            "participant_role": "reviewer",
        },
        {
            "activity_sequence": 5,
            "topic_id": "project",
            "kind": "orchestratorReplaced",
            "message_id": None,
            "actor_key": "actor-a",
            "root_id": "thread",
            "participant_key": "actor-b",
            "participant_role": "orchestrator",
            "replaced_participant_key": "actor-a",
        },
        {
            "activity_sequence": 6,
            "topic_id": "project",
            "kind": "orchestratorReplaced",
            "message_id": None,
            "actor_key": "actor-a",
            "root_id": "thread",
            "participant_key": "actor-a",
            "participant_role": "orchestrator",
            "replaced_participant_key": "actor-b",
        },
    ]
    return SourceRows(tables={"board_activity": events})


def test_metadata_offset_and_fused_root_join_cutoff() -> None:
    assert activity_mapping(history_source(), "project") == {
        1: 3,
        2: 3,
        3: 4,
        4: 5,
        5: 6,
        6: 7,
    }


def test_rejoin_and_handover_have_distinct_interval_endings() -> None:
    source = history_source()
    assert interval_history(source, "project", activity_mapping(source, "project")) == [
        ["thread", "actor-a", "orchestrator", 3, 6, "replaced", "actor-b"],
        ["thread", "actor-b", "advisor", 4, 5, "rejoined", None],
        ["thread", "actor-b", "reviewer", 5, 6, "promoted", None],
        ["thread", "actor-b", "orchestrator", 6, 7, "replaced", "actor-a"],
        ["thread", "actor-a", "orchestrator", 7, None, None, None],
    ]


def test_source_time_comparison_preserves_nanoseconds_and_normalizes_zone() -> None:
    assert (
        canonical_time("2023-11-14T17:00:00.123456789-05:00")
        == "2023-11-14T22:00:00.123456789Z"
    )
    assert canonical_time("2023-11-14T22:00:00.123456780Z") != canonical_time(
        "2023-11-14T22:00:00.123456789Z"
    )
