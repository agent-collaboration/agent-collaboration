"""Independent legacy-to-native assertions; never calls the product transform."""

import contextlib
import json
import sqlite3
import uuid
from pathlib import Path

from pydantic import JsonValue

from .board_store_readback import REPLICATED_KEYS, quoted_identifier
from .board_upgrade_source import (
    activity_mapping,
    canonical_time,
    cutoff,
    identity_key,
    interval_history,
    last_recorded_time,
    source_rows,
)
from .board_upgrade_views import (
    DECLARED_OWNER,
    DECLARED_PROVENANCE,
    ColumnName,
    NativeObservation,
    NativePage,
    Object,
    View,
    int_field,
    json_object,
    json_value,
    text_field,
)
from .legacy_selection_oracle import read_rows, read_sealed_copy, selected_post_ids


class CheckRecord(View):
    label: str
    expected: JsonValue
    observed: JsonValue
    passed: bool


class Checks:
    def __init__(self) -> None:
        self.records: list[CheckRecord] = []

    def equal(self, label: str, expected: object, observed: object) -> None:
        expected_json = json_value.validate_json(json.dumps(expected))
        observed_json = json_value.validate_json(json.dumps(observed))
        passed = expected_json == observed_json
        self.records.append(
            CheckRecord(
                label=label,
                expected=expected_json,
                observed=observed_json,
                passed=passed,
            )
        )
        if not passed:
            raise AssertionError(label)


def native_tables(pages: list[NativePage]) -> dict[str, list[Object]]:
    if not pages:
        raise ValueError("native observer returned no snapshot pages")
    tables: dict[str, list[Object]] = {}
    with contextlib.closing(sqlite3.connect(":memory:")) as schema:
        schema.row_factory = sqlite3.Row
        _ = schema.executescript(";\n".join(pages[0].schema_steps))
        for page in pages:
            if not page.table:
                continue
            columns = [
                ColumnName.model_validate(dict(row)).name
                for row in read_rows(
                    schema, f"PRAGMA table_info({quoted_identifier(page.table)})"
                )
            ]
            rows = tables.setdefault(page.table, [])
            for raw in page.rows:
                values: list[JsonValue] = []
                for cell in raw:
                    match cell["kind"]:
                        case "null":
                            values.append(None)
                        case "integer":
                            values.append(int_field(cell, "value"))
                        case "text":
                            values.append(text_field(cell, "value"))
                        case "real":
                            values.append(cell["value"])
                        case "blob":
                            values.append({"blobBase64": cell["value"]})
                        case _:
                            raise ValueError("unknown snapshot SQL value")
                rows.append(dict(zip(columns, values, strict=True)))
    return tables


def verify_native(path: Path, observed: NativeObservation, checks: Checks) -> None:
    source = source_rows(path)
    selection = read_sealed_copy(path, path.parent)
    topics = source.tables["board_topics"]
    checks.equal(
        "declared owner, independent of product output",
        DECLARED_OWNER,
        observed.report.owner,
    )
    checks.equal(
        "native project IDs one-to-one",
        sorted(text_field(row, "topic_id") for row in topics),
        sorted(row.project for row in observed.projects),
    )
    old_events = {
        int_field(row, "activity_sequence"): row
        for row in source.tables["board_activity"]
    }
    old_posts = {
        text_field(row, "message_id"): row for row in source.tables["board_messages"]
    }
    for project in observed.projects:
        label = project.project
        topic = next(row for row in topics if row["topic_id"] == label)
        report = next(row for row in observed.report.projects if row.project == label)
        mapping = activity_mapping(source, label)
        checks.equal(
            f"{label} independent activity-to-feed mapping",
            mapping,
            {int(key): value.position for key, value in report.old_to_new.items()},
        )
        tables = native_tables(project.pages)
        actors = {
            text_field(row, "actor_id"): identity_key(
                json_object.validate_json(text_field(row, "canonical_identity"))
            )
            for row in tables["actors"]
        }
        for name, key in REPLICATED_KEYS.items():
            for row in tables.get(name, []):
                value = text_field(row, key)
                parsed = uuid.UUID(value)
                checks.equal(
                    f"{label}/{name} canonical UUIDv7",
                    True,
                    str(parsed) == value
                    and parsed.version == 7
                    and parsed.variant == uuid.RFC_4122,
                )
        feed = sorted(
            tables["project_feed"], key=lambda row: int_field(row, "position")
        )
        checks.equal(
            f"{label} dense feed positions",
            list(range(1, len(feed) + 1)),
            [row["position"] for row in feed],
        )
        for row in feed:
            checks.equal(
                f"{label} import feed time/epoch/request",
                [1, None, "legacy_unknown", None],
                [
                    row["epoch"],
                    row["router_time"],
                    row["time_basis"],
                    row["request_id"],
                ],
            )
            checks.equal(
                f"{label} import origin",
                DECLARED_PROVENANCE,
                {
                    "installation": row["origin_installation"],
                    "machine": row["origin_machine"],
                },
            )
        for forbidden in [
            "facts",
            "workstreams",
            "work_trail_rows",
            "role_assignments",
            "owner_grants",
            "request_decisions",
        ]:
            checks.equal(
                f"{label} no invented {forbidden}", [], tables.get(forbidden, [])
            )
        state = tables["project_state"]
        checks.equal(f"{label} one metadata record first", 1, len(state))
        metadata = state[0]
        workspace = next(
            board["project_id"]
            for board in source.tables["project_boards"]
            if board["board_id"] == topic["board_id"]
        )
        checks.equal(
            f"{label} exact native metadata",
            [
                label,
                workspace,
                topic["board_id"],
                topic["description"],
                1,
                1,
                None,
                "legacy_unknown",
                "human:" + DECLARED_OWNER,
                None,
                None,
                None,
            ],
            [
                metadata["project_id"],
                metadata["workspace_id"],
                metadata["board_id"],
                metadata["description"],
                metadata["epoch"],
                metadata["position"],
                metadata["router_time"],
                metadata["time_basis"],
                actors[text_field(metadata, "actor_id")],
                metadata["request_id"],
                metadata["status"],
                metadata["pinned_definition"],
            ],
        )
        source_posts = [row for row in old_posts.values() if row["topic_id"] == label]
        checks.equal(
            f"{label} exact post ID set",
            sorted(text_field(row, "message_id") for row in source_posts),
            sorted(text_field(row, "post_id") for row in tables["posts"]),
        )
        for row in tables["posts"]:
            before = old_posts[text_field(row, "post_id")]
            event = next(
                event
                for event in old_events.values()
                if event["message_id"] == before["message_id"]
            )
            role = (
                None
                if before["posted_from_activity"] is None
                else old_events[int_field(before, "posted_from_activity")][
                    "participant_role"
                ]
            )
            actual_role = (
                None
                if row["posted_as_role"] is None
                else json_value.validate_json(text_field(row, "posted_as_role"))
            )
            visible = "".join(
                f"\\u{ord(character):04x}"
                if ord(character) < 32 and character not in "\n\t"
                else character
                for character in text_field(before, "text")
            )
            checks.equal(
                f"{label}/{row['post_id']} native post payload/history",
                [
                    before["root_id"] or before["message_id"],
                    before["actor_key"],
                    visible,
                    role,
                    mapping[int_field(event, "activity_sequence")],
                ],
                [
                    row["thread_id"],
                    actors[text_field(row, "actor_id")],
                    row["text"],
                    actual_role,
                    row["position"],
                ],
            )
        roots = {
            text_field(row, "message_id")
            for row in source_posts
            if row["root_id"] is None
        }
        checks.equal(
            f"{label} exact thread IDs",
            sorted(roots),
            sorted(text_field(row, "thread_id") for row in tables["threads"]),
        )
        actual_intervals = [
            [
                row["thread_id"],
                actors[text_field(row, "actor_id")],
                row["role"],
                row["joined_position"],
                row["closed_position"],
                row["closed_reason"],
                None
                if row["replaced_by"] is None
                else actors[text_field(row, "replaced_by")],
            ]
            for row in tables["thread_participation"]
        ]
        expected_intervals = interval_history(source, label, mapping)
        checks.equal(
            f"{label} exact continuous seat intervals",
            sorted(expected_intervals, key=json.dumps),
            sorted(actual_intervals, key=json.dumps),
        )
        for row in tables["post_references"]:
            target = json_object.validate_json(text_field(row, "target"))
            legacy = next(
                reference
                for reference in source.tables["message_references"]
                if reference["source_id"] == row["source_post"]
                and reference["ordinal"] == row["place"]
            )
            expected_target = (
                {"kind": "post", "postId": legacy["target_message_id"]}
                if legacy["kind"] == "message"
                else {"kind": "thread", "threadId": legacy["target_root_id"]}
            )
            checks.equal(
                f"{label} post reference target/order", expected_target, target
            )
        checks.equal(
            f"{label} reference count",
            len(
                [
                    row
                    for row in source.tables["message_references"]
                    if text_field(row, "source_id")
                    in {text_field(post, "message_id") for post in source_posts}
                ]
            ),
            len(tables["post_references"]),
        )
        for event in old_events.values():
            if event["topic_id"] != label or event["kind"] in [
                "mainMessageCreated",
                "threadMessageCreated",
            ]:
                continue
            position = mapping[int_field(event, "activity_sequence")]
            row = next(row for row in feed if row["position"] == position)
            if row["record_kind"] == "post":
                continue  # Fused creator join shares its root post's position.
            checks.equal(
                f"{label} seat/resolution Thread version",
                ["thread", event["root_id"], event["actor_key"]],
                [
                    row["record_kind"],
                    row["record_id"],
                    actors[text_field(row, "actor_id")],
                ],
            )
            content = json_object.validate_json(text_field(row, "content"))
            record = json_object.validate_python(content["record"])
            body = json_object.validate_python(record["record"])
            for role in ["orchestrator", "implementer"]:
                expected_holder = next(
                    (
                        interval[1]
                        for interval in expected_intervals
                        if interval[0] == event["root_id"]
                        and interval[2] == role
                        and isinstance(interval[3], int)
                        and interval[3] <= position
                        and (
                            interval[4] is None
                            or isinstance(interval[4], int)
                            and interval[4] > position
                        )
                    ),
                    None,
                )
                holder = body[role]
                actual_holder = (
                    None
                    if holder is None
                    else identity_key(
                        json_object.validate_python(
                            json_object.validate_python(holder)["identity"]
                        )
                    )
                )
                checks.equal(
                    f"{label} historical {role} after seat event",
                    expected_holder,
                    actual_holder,
                )
            if event["kind"] in ["threadResolved", "threadUnresolved"]:
                checks.equal(
                    f"{label} resolution state",
                    "resolved" if event["kind"] == "threadResolved" else "open",
                    body["state"],
                )
        expected_watches: list[list[JsonValue]] = []
        for table, scope in [
            ("topic_watches", "project"),
            ("thread_watches", "thread"),
        ]:
            for row in source.tables[table]:
                if (
                    row.get("topic_id") == label
                    if scope == "project"
                    else row["root_id"] in roots
                ):
                    expected_watches.append(
                        [
                            row["reader_key"],
                            None if scope == "project" else row["root_id"],
                            1,
                            cutoff(mapping, int_field(row, "starts_after_activity")),
                            row["active"],
                        ]
                    )
        actual_watches = [
            [
                actors[text_field(row, "reader_actor")],
                row.get("thread_id"),
                row["start_epoch"],
                row["start_position"],
                row["active"],
            ]
            for name in ["project_watches", "thread_watches"]
            for row in tables[name]
        ]
        checks.equal(
            f"{label} exact watches including inactive/no synthesized rows",
            sorted(expected_watches, key=json.dumps),
            sorted(actual_watches, key=json.dumps),
        )
        for table, scope in [
            ("topic_read_bookmarks", "project"),
            ("thread_read_bookmarks", "thread"),
        ]:
            for row in source.tables[table]:
                scope_id = row["topic_id"] if scope == "project" else row["root_id"]
                if scope_id != label and scope_id not in roots:
                    continue
                mark = next(
                    mark
                    for mark in tables["read_marks"]
                    if mark["scope_kind"] == scope
                    and mark["scope_id"] == scope_id
                    and actors[text_field(mark, "reader_actor")] == row["reader_key"]
                )
                old = int_field(row, "through_activity")
                if scope == "project":
                    workspace = next(
                        board["project_id"]
                        for board in source.tables["project_boards"]
                        if board["board_id"] == topic["board_id"]
                    )
                    starts = [
                        int_field(item, "main_start")
                        for item in source.tables["project_reader_state"]
                        if item["reader_key"] == row["reader_key"]
                        and item["project_id"] == workspace
                        and item["main_start"] is not None
                    ]
                    old = max([old, *starts])
                checks.equal(
                    f"{label} bookmark cutoff",
                    [1, cutoff(mapping, old)],
                    [mark["through_epoch"], mark["through_position"]],
                )
        expected_members = [
            row
            for row in source.tables["thread_subscriptions"]
            if (row["scope_kind"] == "topic" and row["scope_id"] == label)
            or (row["scope_kind"] == "thread" and row["scope_id"] in roots)
        ]
        checks.equal(
            f"{label} membership count",
            len(expected_members),
            len(tables["subscription_memberships"]),
        )
        for before in expected_members:
            member = next(
                row
                for row in tables["subscription_memberships"]
                if actors[text_field(row, "reader_actor")] == before["reader_key"]
                and row["scope_id"] == before["scope_id"]
            )
            policy = json_object.validate_json(text_field(member, "policy"))
            timing = json_object.validate_python(policy["timing"])
            checks.equal(
                f"{label} membership policy/generation/expiry",
                [
                    before["mode"],
                    before["when_idle"],
                    before["quiet_seconds"],
                    before["cap_seconds"],
                    before["lifetime_seconds"],
                    before["generation"],
                    canonical_time(text_field(before, "expires_at")),
                ],
                [
                    policy["mode"],
                    policy["whenIdle"],
                    timing["quietSeconds"],
                    timing["capSeconds"],
                    policy["lifetime"],
                    member["generation"],
                    canonical_time(text_field(member, "expires_at")),
                ],
            )
            ended = (
                None
                if member["ended"] is None
                else json_object.validate_json(text_field(member, "ended"))
            )
            if before["state"] == "active":
                checks.equal(
                    f"{label} active membership has no invented ending", None, ended
                )
            else:
                if ended is None:
                    raise AssertionError("membership lost its recorded ending")
                at = (
                    last_recorded_time(source)
                    if before["state"] == "draining"
                    else canonical_time(text_field(before, "ended_at"))
                )
                reason = (
                    "resolved"
                    if before["state"] == "draining"
                    else before["end_reason"]
                )
                ending_reason = json_object.validate_python(ended["reason"])
                checks.equal(
                    f"{label} ending exact source time/reason",
                    [at, reason],
                    [canonical_time(text_field(ended, "at")), ending_reason["kind"]],
                )
        for inbox in project.inboxes:
            expected = selected_post_ids(
                selection, identity_key(inbox.reader), label, inbox.mode
            )
            checks.equal(
                f"{label} {inbox.mode} exact source-selected post IDs/order",
                expected,
                inbox.post_ids,
            )
        checks.equal(
            f"{label} reader/mode coverage is not vacuous",
            len(selection.reader_keys) * 2,
            len(project.inboxes),
        )
