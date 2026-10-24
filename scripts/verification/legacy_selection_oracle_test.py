"""The oracle must distinguish main marks from reply marks on mixed history."""

import contextlib
import sqlite3
import subprocess
import sys
from pathlib import Path

import pytest

from scripts.verification.legacy_selection_oracle import (
    LegacyActivity,
    LegacyMainStart,
    LegacyOracleReceipt,
    LegacySelection,
    LegacyThreadMark,
    LegacyThreadWatch,
    LegacyTopicMark,
    assert_same_post_selection,
    selected_post_ids,
)


@pytest.fixture
def mixed_history() -> LegacySelection:
    # Main mark=5 reads the main post at 4, but never reads the reply at 3.
    # Thread mark=2 plus watch start=1 leaves that reply unread.
    rows = [
        (1, "mainMessageCreated", "root", None, "other"),
        (2, "participantJoined", None, "root", "reader"),
        (3, "threadMessageCreated", "reply-before-main-mark", "root", "other"),
        (4, "mainMessageCreated", "main-before-mark", None, "other"),
        (6, "threadMessageCreated", "reply-after-mark", "root", "other"),
        (7, "mainMessageCreated", "main-after-mark", None, "other"),
        (8, "threadMessageCreated", "own-reply", "root", "reader"),
    ]
    return LegacySelection(
        reader_keys=["reader"],
        topic_ids=["project"],
        activity=[
            LegacyActivity(
                activity_sequence=sequence,
                project_id="workspace",
                topic_id="project",
                root_id=root,
                kind=kind,
                actor_key=actor,
                message_id=post,
            )
            for sequence, kind, post, root, actor in rows
        ],
        main_starts=[
            LegacyMainStart(reader_key="reader", project_id="workspace", main_start=1)
        ],
        topic_marks=[
            LegacyTopicMark(reader_key="reader", topic_id="project", through_activity=5)
        ],
        thread_marks=[
            LegacyThreadMark(reader_key="reader", root_id="root", through_activity=2)
        ],
        thread_watches=[
            LegacyThreadWatch(
                reader_key="reader", root_id="root", starts_after_activity=1, active=1
            )
        ],
    )


def test_mixed_topic_mark_does_not_read_watched_replies(
    mixed_history: LegacySelection,
) -> None:
    observed = selected_post_ids(mixed_history, "reader", "project", "unread")
    assert observed == ["reply-before-main-mark", "reply-after-mark", "main-after-mark"]


def test_main_mark_sensitivity_catches_project_wide_cutoff(
    mixed_history: LegacySelection,
) -> None:
    expected = selected_post_ids(mixed_history, "reader", "project", "unread")
    with pytest.raises(AssertionError, match="post IDs/order differ"):
        assert_same_post_selection(expected, ["reply-after-mark", "main-after-mark"])


def test_latest_ignores_marks_and_self_filter_but_keeps_newest_first(
    mixed_history: LegacySelection,
) -> None:
    assert selected_post_ids(mixed_history, "reader", "project", "latest") == [
        "own-reply",
        "main-after-mark",
        "reply-after-mark",
        "main-before-mark",
        "reply-before-main-mark",
        "root",
    ]


def test_thread_mark_filters_replies_only(mixed_history: LegacySelection) -> None:
    changed = mixed_history.model_copy(
        update={
            "thread_marks": [
                LegacyThreadMark(
                    reader_key="reader", root_id="root", through_activity=6
                )
            ]
        }
    )
    assert selected_post_ids(changed, "reader", "project", "unread") == [
        "main-after-mark"
    ]


def test_inactive_watch_excludes_replies_in_both_modes(
    mixed_history: LegacySelection,
) -> None:
    changed = mixed_history.model_copy(
        update={
            "thread_watches": [
                LegacyThreadWatch(
                    reader_key="reader",
                    root_id="root",
                    starts_after_activity=1,
                    active=0,
                )
            ]
        }
    )
    assert selected_post_ids(changed, "reader", "project", "unread") == [
        "main-after-mark"
    ]
    assert selected_post_ids(changed, "reader", "project", "latest") == [
        "main-after-mark",
        "main-before-mark",
        "root",
    ]


def test_missing_main_start_still_allows_watched_replies(
    mixed_history: LegacySelection,
) -> None:
    changed = mixed_history.model_copy(update={"main_starts": []})
    assert selected_post_ids(changed, "reader", "project", "unread") == [
        "reply-before-main-mark",
        "reply-after-mark",
    ]


def test_watch_start_is_exclusive(mixed_history: LegacySelection) -> None:
    changed = mixed_history.model_copy(
        update={
            "thread_watches": [
                LegacyThreadWatch(
                    reader_key="reader",
                    root_id="root",
                    starts_after_activity=3,
                    active=1,
                )
            ]
        }
    )
    assert selected_post_ids(changed, "reader", "project", "unread") == [
        "reply-after-mark",
        "main-after-mark",
    ]


def test_selection_accepts_unchanged_allowed_behavior(
    mixed_history: LegacySelection,
) -> None:
    assert_same_post_selection(
        selected_post_ids(mixed_history, "reader", "project", "unread"),
        ["reply-before-main-mark", "reply-after-mark", "main-after-mark"],
    )


def test_cli_covers_state_free_reader_and_empty_topic(tmp_path: Path) -> None:
    source = tmp_path / "legacy.sqlite"
    output = tmp_path / "oracle.json"
    with contextlib.closing(sqlite3.connect(source)) as connection:
        _ = connection.executescript(
            """
            CREATE TABLE board_activity(activity_sequence INTEGER, project_id TEXT,
              topic_id TEXT, root_id TEXT, kind TEXT, actor_key TEXT, message_id TEXT);
            CREATE TABLE project_reader_state(reader_key TEXT, project_id TEXT, main_start INTEGER);
            CREATE TABLE topic_read_bookmarks(reader_key TEXT, topic_id TEXT, through_activity INTEGER);
            CREATE TABLE thread_read_bookmarks(reader_key TEXT, root_id TEXT, through_activity INTEGER);
            CREATE TABLE thread_watches(reader_key TEXT, root_id TEXT, starts_after_activity INTEGER, active INTEGER);
            CREATE TABLE topic_watches(reader_key TEXT, topic_id TEXT, starts_after_activity INTEGER, active INTEGER);
            CREATE TABLE board_identities(identity_key TEXT);
            CREATE TABLE board_topics(topic_id TEXT);
            INSERT INTO board_identities VALUES('human:legacy-owner');
            INSERT INTO board_topics VALUES('019a0000-0000-7000-8000-000000000015');
            INSERT INTO topic_watches VALUES('human:legacy-owner','019a0000-0000-7000-8000-000000000015',0,1);
            """
        )
        connection.commit()
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "scripts.verification.legacy_selection_oracle",
            "--legacy-copy",
            str(source),
            "--copy-root",
            str(tmp_path),
            "--output",
            str(output),
        ],
        cwd=Path(__file__).resolve().parents[2],
        text=True,
        capture_output=True,
        check=False,
        timeout=10,
    )
    assert result.returncode == 0, result.stderr
    receipt = LegacyOracleReceipt.model_validate_json(output.read_text())
    assert [
        (row.reader, row.legacy_topic, row.mode, row.post_ids)
        for row in receipt.selections
    ] == [
        ("human:legacy-owner", "019a0000-0000-7000-8000-000000000015", "latest", []),
        ("human:legacy-owner", "019a0000-0000-7000-8000-000000000015", "unread", []),
    ]
