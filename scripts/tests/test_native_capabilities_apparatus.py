"""Check the native-capabilities fixture, native-fact reader and PTY driver.

The terminal cases drive only `/bin/cat`, a harmless local subprocess this test starts
and stops; they prove the driver's capture, waits and shutdown, never client behavior.
"""

import json
import tempfile
import unittest
from pathlib import Path

from pydantic import JsonValue

from scripts.verification.acp_metadata_observation import ProviderObservation
from scripts.verification.native_capabilities_fixture import (
    CANARY_PLACEHOLDER,
    materialize_fixture,
)
from scripts.verification.native_capabilities_models import NativePluginRow
from scripts.verification.native_capabilities_target import (
    ObservationCursor,
    native_plugin_facts,
    observe_command,
    paginate_observation,
    read_session_recorder,
)
from scripts.verification.native_capabilities_terminal import (
    InstalledCodexTerminal,
    TerminalTimeout,
)


def page(sequences: list[int], end: str, epoch: int = 3) -> ProviderObservation:
    events: list[JsonValue] = [
        {
            "sequence": sequence,
            "event": {"kind": "turnStarted", "turnId": f"t{sequence}"},
        }
        for sequence in sequences
    ]
    target: JsonValue = {
        "endpoint": {"serviceId": "service", "endpointId": "claude-local"},
        "sessionId": "thread",
    }
    return ProviderObservation.model_validate(
        {"target": target, "events": events, "epoch": epoch, "endReason": end}
    )


class ObservationPagingTests(unittest.TestCase):
    def argument(self, command: list[str], flag: str) -> str:
        return command[command.index(flag) + 1]

    def test_command_stays_inside_the_cli_parser_bounds(self) -> None:
        # event_commands.rs: --max-events 1..=4096, --max-bytes 1..=1048576.
        first = observe_command(Path("/cli"), "claude-local", "s", Path("/svc"), None)
        self.assertLessEqual(int(self.argument(first, "--max-events")), 4096)
        self.assertLessEqual(int(self.argument(first, "--max-bytes")), 1_048_576)
        self.assertNotIn("--after-sequence", first)
        self.assertNotIn("--epoch", first)

    def test_continuation_carries_the_observed_epoch(self) -> None:
        # --after-sequence `requires = "epoch"` in the parser.
        cursor = ObservationCursor(after_sequence=12, epoch=3)
        command = observe_command(
            Path("/cli"), "claude-local", "s", Path("/svc"), cursor
        )
        self.assertEqual(self.argument(command, "--after-sequence"), "12")
        self.assertEqual(self.argument(command, "--epoch"), "3")

    def test_result_limited_pages_are_followed_to_the_deadline(self) -> None:
        pages = [page([1, 2], "resultLimitReached"), page([3], "deadlineReached")]
        cursors: list[ObservationCursor | None] = []

        def fetch(cursor: ObservationCursor | None, index: int) -> ProviderObservation:
            cursors.append(cursor)
            return pages[index]

        merged = paginate_observation(fetch, None)
        self.assertEqual([entry.sequence for entry in merged.events], [1, 2, 3])
        self.assertEqual(merged.endReason, "deadlineReached")
        self.assertEqual(cursors, [None, ObservationCursor(after_sequence=2, epoch=3)])

    def test_incomplete_observations_never_pass_as_complete(self) -> None:
        start = ObservationCursor(after_sequence=5, epoch=3)
        cases: list[tuple[list[ProviderObservation], ObservationCursor | None, str]] = [
            ([page([6], "deadlineReached", epoch=4)], start, "epoch changed"),
            ([page([1], "resyncRequired")], None, "resync"),
            ([page([], "resultLimitReached")], None, "resync"),
            (
                [page([index], "resultLimitReached") for index in range(1, 70)],
                None,
                "page bound",
            ),
        ]
        for pages, cursor, message in cases:

            def fetch(
                _cursor: ObservationCursor | None,
                index: int,
                pages: list[ProviderObservation] = pages,
            ) -> ProviderObservation:
                return pages[index]

            with (
                self.subTest(message=message),
                self.assertRaisesRegex(ValueError, message),
            ):
                _ = paginate_observation(fetch, cursor)


class FixtureTests(unittest.TestCase):
    def test_materialized_skills_carry_fresh_distinct_canaries(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            fixture = materialize_fixture(root / "w", root / "e", root / "record.json")
            body = fixture.skill_file.read_text()
            second = fixture.second_skill_file.read_text()
            self.assertIn(fixture.canary, body)
            self.assertIn(fixture.second_canary, second)
            self.assertNotEqual(fixture.canary, fixture.second_canary)
            self.assertNotIn(CANARY_PLACEHOLDER, body + second)
            self.assertIn("$ARGUMENTS", body)
            self.assertEqual(
                fixture.skill_file,
                root / "w/.claude/skills/router-native-arguments/SKILL.md",
            )
            self.assertEqual(list((root / "e").iterdir()), [])
            self.assertEqual((root / "record.json").stat().st_mode & 0o777, 0o600)
            again = materialize_fixture(root / "w2", root / "e2", root / "record2.json")
            self.assertNotEqual(again.canary, fixture.canary)

    def test_existing_workspace_is_never_reused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "w").mkdir()
            with self.assertRaisesRegex(ValueError, "already exists"):
                _ = materialize_fixture(root / "w", root / "e", root / "record.json")


class NativePluginFactTests(unittest.TestCase):
    def plugin(self, root: Path) -> NativePluginRow:
        install = root / "alpha"
        (install / ".claude-plugin").mkdir(parents=True)
        (install / "skills/review").mkdir(parents=True)
        _ = (install / "skills/review/SKILL.md").write_text(
            "---\nname: review\ndescription: Review code\n---\nBody\n"
        )
        return NativePluginRow(
            id="alpha@market", enabled=True, installPath=str(install)
        )

    def test_declared_and_absent_components(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            row = self.plugin(root)
            install = Path(row.installPath)
            _ = (install / ".claude-plugin/plugin.json").write_text(
                json.dumps({"name": "alpha", "mcpServers": {"search": {}}})
            )
            (install / "hooks").mkdir()
            _ = (install / "hooks/hooks.json").write_text(
                json.dumps({"hooks": {"PreToolUse": [], "Stop": []}})
            )
            facts = native_plugin_facts(row, root, "case")
            self.assertEqual(facts.skill_descriptions, {"review": "Review code"})
            self.assertEqual(facts.hook_events, ["PreToolUse", "Stop"])
            self.assertEqual(facts.mcp_servers, ["search"])
            self.assertTrue((root / "case-native-plugin-facts.json").exists())

    def test_no_declaration_is_known_empty_and_unreadable_is_unobserved(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            row = self.plugin(root)
            install = Path(row.installPath)
            _ = (install / ".claude-plugin/plugin.json").write_text(
                json.dumps({"name": "alpha", "hooks": "missing/hooks.json"})
            )
            facts = native_plugin_facts(row, root, "case")
            self.assertIsNone(facts.hook_events)
            self.assertEqual(facts.mcp_servers, [])

    def test_multi_line_description_is_not_guessed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            row = self.plugin(root)
            _ = (Path(row.installPath) / "skills/review/SKILL.md").write_text(
                "---\nname: review\ndescription: >\n  folded\n---\n"
            )
            with self.assertRaisesRegex(ValueError, "not one line"):
                _ = native_plugin_facts(row, root, "case")


class RecorderTests(unittest.TestCase):
    def test_reader_requires_the_clients_own_log(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "session.jsonl"
            with self.assertRaisesRegex(ValueError, "no session recorder log"):
                _ = read_session_recorder(log)
            _ = log.write_text(
                '{"dir":"meta","kind":"session_start"}\n\n{"dir":"from_tui","kind":"op","payload":{}}\n'
            )
            self.assertEqual(
                [record["kind"] for record in read_session_recorder(log)],
                ["session_start", "op"],
            )


class TerminalDriverTests(unittest.TestCase):
    def test_typed_input_is_captured_and_waited_for(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            capture = Path(directory) / "cat.pty"
            with InstalledCodexTerminal(["/bin/cat"], capture) as terminal:
                mark = terminal.mark()
                terminal.type_text("router-native-probe\r")
                screen = terminal.wait_for(r"router-native-probe", mark, 10)
                self.assertIn("router-native-probe", screen)
                self.assertIsNone(
                    terminal.wait_for_any({"absent": "never-printed"}, mark, 0.3)
                )
            self.assertIsNotNone(terminal.exit_status)
            self.assertIn(b"router-native-probe", capture.read_bytes())
            self.assertEqual(capture.stat().st_mode & 0o777, 0o600)

    def test_missing_screen_text_times_out_instead_of_passing(self) -> None:
        with (
            tempfile.TemporaryDirectory() as directory,
            InstalledCodexTerminal(
                ["/bin/cat"], Path(directory) / "cat.pty"
            ) as terminal,
            self.assertRaises(TerminalTimeout),
        ):
            _ = terminal.wait_for(r"never-printed", terminal.mark(), 0.5)


if __name__ == "__main__":
    _ = unittest.main()
