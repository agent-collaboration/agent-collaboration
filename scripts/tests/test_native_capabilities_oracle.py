"""Check the J2-J4 native-capabilities oracles against known-good and wrong evidence.

Each wrong case is a build the Verification Design says must fail: a substituted
Codex catalogue, a borrowed workspace, an omitted or foreign plugin, invented detail
items, a silently dropped selection, a changed history command, display-driven
operations. These establish apparatus sensitivity only, never product proof.
"""

import unittest

from pydantic import JsonValue

from scripts.tests.native_capabilities_test_support import (
    ARGUMENTS,
    CANARY,
    SECOND_CANARY,
    SKILL,
    NativeSetup,
    catalogue,
    item,
    observation,
    operation,
    turn_started,
)
from scripts.verification.acp_metadata_observation import ProviderObservation
from scripts.verification.native_capabilities_models import (
    HistoryThread,
    OperationRow,
)
from scripts.verification.native_capabilities_oracle import (
    check_command_catalogues,
    check_display_operations,
    check_native_invocation,
    check_recorded_selection,
    check_rejected_input,
    check_selected_history,
)
from scripts.verification.typed_observations import JsonObject


class CommandAndInvocationTests(unittest.TestCase):
    def test_latest_list_replaces_earlier_entries(self) -> None:
        with NativeSetup() as setup:
            events = [
                catalogue("first", ["compact", "old", SKILL]),
                catalogue("second", ["compact", SKILL]),
            ]
            result = check_command_catalogues(observation(events), setup.fixture)
            self.assertEqual(result.replacement, "observed")
            self.assertEqual(result.removed_names, ["old"])

    def test_identical_lists_leave_replacement_unobserved(self) -> None:
        with NativeSetup() as setup:
            events = [catalogue("first", [SKILL]), catalogue("second", [SKILL])]
            result = check_command_catalogues(observation(events), setup.fixture)
            self.assertEqual(result.replacement, "unobserved")

    def test_current_list_without_the_native_command_is_rejected(self) -> None:
        with NativeSetup() as setup:
            events = [catalogue("first", [SKILL]), catalogue("second", ["compact"])]
            with self.assertRaises(ValueError):
                _ = check_command_catalogues(observation(events), setup.fixture)

    def test_argument_dependent_native_effect_passes(self) -> None:
        with NativeSetup() as setup:
            events = observation(
                [
                    turn_started(),
                    item("reply", "agentMessage", f"{CANARY}::{ARGUMENTS}"),
                ]
            )
            result = check_native_invocation(
                events,
                setup.fixture,
                ARGUMENTS,
                [f"${SKILL} {ARGUMENTS}"],
                [],
                [operation("prompt-1")],
            )
            self.assertEqual(result.reply_item_id, "reply")

    def test_dropped_corrupted_or_echoed_selection_is_rejected(self) -> None:
        cases = [
            ("Sure, done.", ["input"]),
            (f"{CANARY}::literal-value café\nsecond-line", ["input"]),
            (f"{CANARY}::{ARGUMENTS}", [f"{CANARY} {ARGUMENTS}"]),
        ]
        with NativeSetup() as setup:
            for reply, supplied in cases:
                events = observation(
                    [turn_started(), item("reply", "agentMessage", reply)]
                )
                with self.subTest(reply=reply), self.assertRaises(ValueError):
                    _ = check_native_invocation(
                        events, setup.fixture, ARGUMENTS, supplied, [], [operation("p")]
                    )

    def test_invocation_needs_exactly_one_router_prompt(self) -> None:
        with NativeSetup() as setup:
            events = observation(
                [
                    turn_started(),
                    item("reply", "agentMessage", f"{CANARY}::{ARGUMENTS}"),
                ]
            )
            with self.assertRaisesRegex(ValueError, "exactly one Router prompt"):
                _ = check_native_invocation(
                    events, setup.fixture, ARGUMENTS, [], [], []
                )

    def test_explicit_rejection_with_no_native_effect_passes(self) -> None:
        with NativeSetup() as setup:
            result = check_rejected_input(
                observation([]), setup.fixture, [], [], client_error_seen=True
            )
            self.assertEqual(result.new_operation_ids, [])

    def test_silent_drop_is_not_an_explicit_rejection(self) -> None:
        with (
            NativeSetup() as setup,
            self.assertRaisesRegex(ValueError, "dropped silently"),
        ):
            _ = check_rejected_input(
                observation([]), setup.fixture, [], [], client_error_seen=False
            )

    def test_rejected_selection_that_reached_the_provider_fails(self) -> None:
        reached: list[tuple[ProviderObservation, list[OperationRow]]] = [
            (observation([]), [operation("prompt-1")]),
            (observation([turn_started()]), []),
            (observation([item("reply", "agentMessage", f"{SECOND_CANARY}::x")]), []),
        ]
        with NativeSetup() as setup:
            for events, operations in reached:
                with self.subTest(operations=operations), self.assertRaises(ValueError):
                    _ = check_rejected_input(
                        events, setup.fixture, [], operations, client_error_seen=True
                    )

    def test_selected_steer_allows_only_its_ordinary_turn(self) -> None:
        with NativeSetup() as setup:
            events = observation(
                [turn_started(), item("reply", "agentMessage", "STEER_BASE_OK")]
            )
            result = check_rejected_input(
                events,
                setup.fixture,
                [],
                [operation("prompt-1")],
                client_error_seen=True,
                ordinary_prompts=1,
            )
            self.assertEqual(result.new_operation_ids, ["prompt-1"])


def recorder(items: list[JsonObject]) -> list[JsonObject]:
    turn: JsonObject = {"UserTurn": {"items": list[JsonValue](items), "cwd": "/w"}}
    return [
        {"dir": "meta", "kind": "session_start"},
        {"dir": "to_tui", "kind": "app_event", "variant": "CommitTick"},
        {"dir": "from_tui", "kind": "op", "payload": turn},
    ]


class RecordedSelectionTests(unittest.TestCase):
    text: str = f"${SKILL} {ARGUMENTS}"

    def test_client_text_and_skill_with_real_origin_pass(self) -> None:
        with NativeSetup() as setup:
            records = recorder(
                [
                    {
                        "type": "text",
                        "text": self.text,
                        "text_elements": [{"byte_range": {"start": 0, "end": 24}}],
                    },
                    {"type": "skill", "name": SKILL, "path": str(setup.skill_file)},
                ]
            )
            result = check_recorded_selection(
                records, setup.fixture, self.text, [SKILL]
            )
            self.assertEqual(result.item_types, ["text", "skill"])
            self.assertEqual(result.text_element_count, 1)

    def test_case_not_reached_by_the_client_is_rejected(self) -> None:
        with NativeSetup() as setup:
            text: JsonObject = {"type": "text", "text": self.text}
            skill: JsonObject = {
                "type": "skill",
                "name": SKILL,
                "path": str(setup.skill_file),
            }
            wrong: list[list[JsonObject]] = [
                [text],
                [
                    text,
                    {"type": "skill", "name": SKILL, "path": str(setup.second_file)},
                ],
                [{"type": "text", "text": "different"}, skill],
                [
                    text,
                    {"type": "mention", "name": "Alpha", "path": "plugin://alpha@m"},
                ],
            ]
            for items in wrong:
                with self.subTest(items=items), self.assertRaises(ValueError):
                    _ = check_recorded_selection(
                        recorder(items), setup.fixture, self.text, [SKILL]
                    )

    def test_turn_count_must_match_the_case(self) -> None:
        with NativeSetup() as setup:
            records = recorder(
                [
                    {"type": "text", "text": self.text},
                    {"type": "skill", "name": SKILL, "path": str(setup.skill_file)},
                ]
            )
            with self.assertRaisesRegex(ValueError, "expected 2 recorded user turns"):
                _ = check_recorded_selection(
                    records, setup.fixture, self.text, [SKILL], expected_turns=2
                )


def history(
    user_text: str, reply: str = f"{CANARY}::{ARGUMENTS}", thread_id: str = "thread"
) -> HistoryThread:
    items: list[JsonValue] = [
        {
            "id": "user",
            "type": "userMessage",
            "content": [{"type": "text", "text": user_text}],
        },
        {"id": "reply", "type": "agentMessage", "text": reply},
    ]
    return HistoryThread.model_validate(
        {"id": thread_id, "turns": [{"id": "turn", "items": items}]}
    )


class HistoryAndDisplayTests(unittest.TestCase):
    def test_canonical_command_with_outer_trim_only_passes(self) -> None:
        with NativeSetup() as setup:
            exact = history(f"/{SKILL} {ARGUMENTS}")
            result = check_selected_history(
                [exact, exact], "thread", setup.fixture, f"  {ARGUMENTS}  "
            )
            self.assertTrue(result.trimmed_outer_whitespace)
            same = check_selected_history(
                [exact, exact], "thread", setup.fixture, ARGUMENTS
            )
            self.assertFalse(same.trimmed_outer_whitespace)

    def test_changed_history_is_rejected(self) -> None:
        wrong = [
            history(f"${SKILL} {ARGUMENTS}"),
            history(f"/router-native-second {ARGUMENTS}"),
            history(f"/{SKILL} literal-value café second-line"),
            history(f"/{SKILL} {ARGUMENTS}", reply="ORDINARY"),
            history(f"/{SKILL} {ARGUMENTS}", thread_id="other"),
        ]
        with NativeSetup() as setup:
            for view in wrong:
                views = [view, history(f"/{SKILL} {ARGUMENTS}")]
                with self.subTest(view=view), self.assertRaises(ValueError):
                    _ = check_selected_history(
                        views, "thread", setup.fixture, ARGUMENTS
                    )

    def test_other_invocations_of_the_command_do_not_hide_the_selected_one(
        self,
    ) -> None:
        # J3 invokes the same native command through Router with different arguments.
        items: list[JsonValue] = [
            {
                "id": "router-user",
                "type": "userMessage",
                "content": [{"type": "text", "text": f"/{SKILL} router-probe"}],
            },
            {
                "id": "router-reply",
                "type": "agentMessage",
                "text": f"{CANARY}::router-probe",
            },
            {
                "id": "user",
                "type": "userMessage",
                "content": [{"type": "text", "text": f"/{SKILL} {ARGUMENTS}"}],
            },
            {"id": "reply", "type": "agentMessage", "text": f"{CANARY}::{ARGUMENTS}"},
        ]
        view = HistoryThread.model_validate(
            {"id": "thread", "turns": [{"id": "turn", "items": items}]}
        )
        with NativeSetup() as setup:
            result = check_selected_history(
                [view, view], "thread", setup.fixture, ARGUMENTS
            )
            self.assertEqual(result.user_item_ids, ["user", "user"])
            repeated = HistoryThread.model_validate(
                {"id": "thread", "turns": [{"id": "turn", "items": [*items, items[2]]}]}
            )
            with self.assertRaisesRegex(ValueError, "repeated"):
                _ = check_selected_history(
                    [repeated, view], "thread", setup.fixture, ARGUMENTS
                )

    def test_pure_display_adds_nothing(self) -> None:
        before = [operation("prompt-1")]
        result = check_display_operations(
            before, before, observation([]), unloaded_resume=False
        )
        self.assertEqual(result.new_operation_kinds, [])

    def test_display_driven_operations_or_turns_are_rejected(self) -> None:
        before = [operation("prompt-1")]
        loaded = [*before, operation("load", "conversationLoad")]
        replayed = [*before, operation("replayed")]
        with self.assertRaisesRegex(ValueError, "conversationLoad"):
            _ = check_display_operations(
                before, loaded, observation([]), unloaded_resume=False
            )
        with self.assertRaisesRegex(ValueError, "conversationPrompt"):
            _ = check_display_operations(
                before, replayed, observation([]), unloaded_resume=True
            )
        with self.assertRaisesRegex(ValueError, "provider turn"):
            _ = check_display_operations(
                before, before, observation([turn_started()]), unloaded_resume=False
            )
        with self.assertRaisesRegex(ValueError, "incomplete"):
            _ = check_display_operations(
                before,
                before,
                observation([], end="resultLimitReached"),
                unloaded_resume=False,
            )

    def test_explicit_unloaded_resume_may_load(self) -> None:
        before = [operation("prompt-1")]
        loaded = [*before, operation("load", "conversationLoad")]
        result = check_display_operations(
            before, loaded, observation([]), unloaded_resume=True
        )
        self.assertEqual(result.new_operation_kinds, ["conversationLoad"])


if __name__ == "__main__":
    _ = unittest.main()
