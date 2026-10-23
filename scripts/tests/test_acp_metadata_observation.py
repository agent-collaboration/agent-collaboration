"""Check the metadata journey's oracle against known-good and wrong evidence."""

import unittest

from scripts.verification.acp_metadata_observation import (
    CodexThread,
    ObservedHubEvent,
    ProviderObservation,
    check_metadata_observation,
    terminal_text,
)


def observation(*, usage: bool = True) -> ProviderObservation:
    events: list[dict[str, object]] = [
        {
            "sequence": 1,
            "event": {
                "kind": "itemStarted",
                "item": {
                    "itemId": "commands",
                    "kind": {"kind": "notice"},
                    "text": '{"availableCommands":[{"name":"compact","description":"Compact context"}]}',
                },
            },
        },
        {
            "sequence": 2,
            "event": {
                "kind": "itemStarted",
                "item": {
                    "itemId": "assistant",
                    "kind": {"kind": "agentMessage"},
                    "text": "ACP_OK",
                },
            },
        },
    ]
    if usage:
        events.append(
            {
                "sequence": 3,
                "event": {
                    "kind": "itemStarted",
                    "item": {
                        "itemId": "usage",
                        "kind": {"kind": "usage"},
                        "text": '{"used":12,"size":100}',
                    },
                },
            }
        )
    return ProviderObservation.model_validate(
        {
            "target": {
                "endpoint": {"serviceId": "service", "endpointId": "cursor-local"},
                "sessionId": "thread",
            },
            "events": events,
            "epoch": 0,
            "endReason": "deadlineReached",
        }
    )


def thread(
    *, leak: bool = False, reply: bool = True, thread_id: str = "thread"
) -> CodexThread:
    items: list[dict[str, str]] = []
    if reply:
        items.append({"id": "assistant", "type": "agentMessage", "text": "ACP_OK"})
    if leak:
        items.append(
            {
                "id": "commands",
                "type": "agentMessage",
                "text": "Even formatted metadata is not assistant speech",
            }
        )
    return CodexThread.model_validate(
        {"id": thread_id, "turns": [{"id": "turn", "items": items}]}
    )


class MetadataObservationTests(unittest.TestCase):
    def test_actual_live_capture_crlf_is_normalized_before_classification(self) -> None:
        result = check_metadata_observation(
            observation(),
            [thread(), thread()],
            terminal_text(b"\r\n\x1b[2m\xe2\x80\xa2 \x1b[22mACP_OK\r\n"),
            "\n• ACP_OK  › Ask Codex to do anything",
            "ACP_OK",
            False,
        )
        self.assertTrue(result.terminal_reply_observed)

    def test_actual_resume_row_can_be_followed_by_the_composer_on_the_same_line(
        self,
    ) -> None:
        result = check_metadata_observation(
            observation(),
            [thread(), thread()],
            "\n• ACP_OK\n",
            "\n• ACP_OK  › Ask Codex to do anything  Grok 4.6 default",
            "ACP_OK",
            False,
        )
        self.assertTrue(result.terminal_reply_observed)

    def test_leaked_title_alone_is_not_assistant_rendering(self) -> None:
        original = observation()
        original.events.append(
            ObservedHubEvent.model_validate(
                {
                    "sequence": 4,
                    "event": {
                        "kind": "itemStarted",
                        "item": {
                            "itemId": "title",
                            "kind": {"kind": "sessionInfo"},
                            "text": '{"title":"ACP_OK"}',
                        },
                    },
                }
            )
        )
        with self.assertRaises(ValueError):
            _ = check_metadata_observation(
                original,
                [thread(), thread()],
                '\n• {"title":"ACP_OK"}\n',
                "\n• ACP_OK\n",
                "ACP_OK",
                False,
            )

    def test_raw_usage_beside_a_real_reply_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "metadata"):
            _ = check_metadata_observation(
                observation(),
                [thread(), thread()],
                '\n• ACP_OK\n• {"used":12,"size":100}\n',
                "\n• ACP_OK\n",
                "ACP_OK",
                False,
            )

    def test_retention_and_both_historical_views_have_independent_controls(
        self,
    ) -> None:
        result = check_metadata_observation(
            observation(), [thread(), thread()], "• ACP_OK", "• ACP_OK", "ACP_OK", True
        )
        self.assertEqual(result.metadata_item_ids, ["commands", "usage"])
        self.assertEqual(result.usage_items, 1)
        self.assertEqual(result.terminal_views, 2)

    def test_either_historical_view_leaking_metadata_is_rejected(self) -> None:
        for position in (0, 1):
            views = [thread(), thread()]
            views[position] = thread(leak=True)
            with self.assertRaisesRegex(ValueError, "metadata leaked"):
                _ = check_metadata_observation(
                    observation(), views, "ACP_OK", "ACP_OK", "ACP_OK", False
                )

    def test_dropping_metadata_is_not_a_fix(self) -> None:
        original = observation()
        original.events = [
            entry
            for entry in original.events
            if entry.event.item is not None and entry.event.item.itemId == "assistant"
        ]
        with self.assertRaisesRegex(ValueError, "not retained"):
            _ = check_metadata_observation(
                original, [thread(), thread()], "ACP_OK", "ACP_OK", "ACP_OK", False
            )

    def test_dropping_assistant_output_is_not_a_fix(self) -> None:
        with self.assertRaisesRegex(ValueError, "missing from Codex history"):
            _ = check_metadata_observation(
                observation(),
                [thread(reply=False), thread()],
                "ACP_OK",
                "ACP_OK",
                "ACP_OK",
                False,
            )

    def test_both_original_terminal_captures_must_render_the_reply(self) -> None:
        for terminal, resume in [("working", "ACP_OK"), ("ACP_OK", "working")]:
            with self.assertRaisesRegex(ValueError, "original installed-client PTY"):
                _ = check_metadata_observation(
                    observation(),
                    [thread(), thread()],
                    terminal,
                    resume,
                    "ACP_OK",
                    False,
                )

    def test_incomplete_observation_and_wrong_threads_are_rejected(self) -> None:
        original = observation()
        original.endReason = "resultLimitReached"
        with self.assertRaisesRegex(ValueError, "incomplete"):
            _ = check_metadata_observation(
                original, [thread(), thread()], "ACP_OK", "ACP_OK", "ACP_OK", False
            )
        with self.assertRaisesRegex(ValueError, "same thread"):
            _ = check_metadata_observation(
                observation(),
                [thread(thread_id="other"), thread(thread_id="other")],
                "ACP_OK",
                "ACP_OK",
                "ACP_OK",
                False,
            )

    def test_unemitted_usage_stays_an_explicit_runtime_gap(self) -> None:
        result = check_metadata_observation(
            observation(usage=False),
            [thread(), thread()],
            "• ACP_OK",
            "• ACP_OK",
            "ACP_OK",
            False,
        )
        self.assertIsNotNone(result.usage_limit)
        with self.assertRaisesRegex(ValueError, "not retained"):
            _ = check_metadata_observation(
                observation(usage=False),
                [thread(), thread()],
                "ACP_OK",
                "ACP_OK",
                "ACP_OK",
                True,
            )

    def test_original_terminal_escape_sequences_do_not_hide_the_control(self) -> None:
        self.assertEqual(terminal_text(b"\x1b[32mACP_OK\x1b[0m"), "ACP_OK")


if __name__ == "__main__":
    _ = unittest.main()
