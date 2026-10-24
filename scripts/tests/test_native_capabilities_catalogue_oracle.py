"""Check the J1 catalogue oracles against known-good and wrong evidence.

Each wrong case is a build the Verification Design says must fail: a substituted
Codex catalogue, a borrowed workspace, a wrong-role source error, a disabled row with
no native fact, an omitted or foreign plugin, invented detail items. These establish
apparatus sensitivity only, never product proof.
"""

import tempfile
import typing as t
import unittest
from pathlib import Path

from pydantic import JsonValue

from scripts.tests.native_capabilities_test_support import (
    CURSOR_DISABLED,
    SKILL,
    NativeSetup,
    error_reply,
    skill_row,
    skills_reply,
    write_skill,
)
from scripts.verification.native_capabilities_catalogue_oracle import (
    check_installed_plugin_skill_sample,
    check_management_rejected,
    check_plugin_catalogue,
    check_plugin_detail,
    check_skill_catalogue,
    check_wrong_context_catalogue,
)
from scripts.verification.native_capabilities_models import (
    NativeMarketplace,
    NativePluginFacts,
    NativePluginRow,
    NativeSkillAuthority,
    RpcReply,
)
from scripts.verification.typed_observations import JsonObject


class SkillCatalogueTests(unittest.TestCase):
    def test_native_fixture_with_user_skill_is_supported(self) -> None:
        with NativeSetup() as setup:
            reply = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("user-native", setup.user_skill, "user"),
                ],
            )
            result = check_skill_catalogue(reply, setup.fixture, setup.authority())
            self.assertEqual(result.status, "supported")
            self.assertEqual(result.skill_count, 2)

    def test_shared_codex_root_loaded_by_the_provider_is_legitimate(self) -> None:
        # Counterexample to a path-prefix ban: Cursor's own loader reads ~/.codex/skills.
        with NativeSetup() as setup:
            reply = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("shared-native", setup.shared_skill, "user"),
                ],
            )
            authority = setup.authority("shared-native")
            result = check_skill_catalogue(reply, setup.fixture, authority)
            self.assertEqual(result.status, "supported")

    def test_substituted_codex_catalogue_is_rejected(self) -> None:
        with NativeSetup() as setup:
            reply = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("codex-only", setup.codex_only, "user"),
                ],
            )
            with self.assertRaisesRegex(ValueError, "not loaded by the bound provider"):
                _ = check_skill_catalogue(reply, setup.fixture, setup.authority())

    def test_advertised_name_outside_the_providers_roots_is_rejected(self) -> None:
        with NativeSetup() as setup:
            reply = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("shared-native", setup.shared_skill, "user"),
                ],
            )
            authority = setup.authority("shared-native", shared_root=False)
            with self.assertRaisesRegex(ValueError, "outside the provider's own roots"):
                _ = check_skill_catalogue(reply, setup.fixture, authority)

    def test_real_natively_disabled_file_is_a_legitimate_disabled_row(self) -> None:
        with NativeSetup() as setup:
            skipped = write_skill(
                setup.workspace / ".claude/skills", "skipped", "user-invocable: false\n"
            )
            # The provider doesn't advertise it; its own file says the loader skips it.
            reply = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("skipped", skipped, enabled=False),
                ],
            )
            authority = setup.authority(disabled_markers=CURSOR_DISABLED)
            result = check_skill_catalogue(reply, setup.fixture, authority)
            self.assertEqual(result.skill_count, 2)
            # Without a qualified native disabled fact the same row can't be judged.
            with self.assertRaisesRegex(ValueError, "without a native disabled fact"):
                _ = check_skill_catalogue(reply, setup.fixture, setup.authority())

    def test_falsely_disabled_or_enabled_rows_are_rejected(self) -> None:
        with NativeSetup() as setup:
            skipped = write_skill(
                setup.workspace / ".claude/skills", "skipped", "user-invocable: false\n"
            )
            authority = setup.authority(
                "skipped", shared_root=False, disabled_markers=CURSOR_DISABLED
            )
            wrong: list[tuple[JsonObject, str]] = [
                # A foreign Codex skill marked disabled is still outside the roots.
                (
                    skill_row("codex-only", setup.codex_only, "user", enabled=False),
                    "roots",
                ),
                # A real native skill marked disabled without any native disabled fact.
                (
                    skill_row("user-native", setup.user_skill, "user", enabled=False),
                    "without a native disabled fact",
                ),
                # A natively skipped file reported as enabled, even if a name matches.
                (skill_row("skipped", skipped), "reported enabled"),
            ]
            for row, message in wrong:
                reply = skills_reply(
                    setup.workspace, [skill_row(SKILL, setup.skill_file), row]
                )
                with self.subTest(row=row), self.assertRaisesRegex(ValueError, message):
                    _ = check_skill_catalogue(reply, setup.fixture, authority)

    def test_disabled_file_never_shadows_an_active_definition(self) -> None:
        with NativeSetup() as setup:
            active = write_skill(setup.home / ".claude/skills", "dup")
            disabled = write_skill(
                setup.workspace / ".claude/skills", "dup", "user-invocable: false\n"
            )
            authority = setup.authority("dup", disabled_markers=CURSOR_DISABLED)
            both = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("dup", disabled, enabled=False),
                    skill_row("dup", active, "user"),
                ],
            )
            self.assertEqual(
                check_skill_catalogue(both, setup.fixture, authority).skill_count, 3
            )
            shadowed = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("dup", disabled, enabled=False),
                ],
            )
            with self.assertRaisesRegex(ValueError, "shadows the active native"):
                _ = check_skill_catalogue(shadowed, setup.fixture, authority)

    def test_plugin_skill_needs_its_own_plugins_namespace(self) -> None:
        with NativeSetup() as setup:
            plugins = setup.home / ".claude/plugins/cache/market"
            beta_review = write_skill(plugins / "beta/1.0.0/skills", "review")
            namespaces = (
                (plugins / "alpha/1.0.0", "alpha"),
                (plugins / "beta/1.0.0", "beta"),
            )
            reply = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("review", beta_review, "user"),
                ],
            )

            def authority(*advertised: str) -> NativeSkillAuthority:
                return NativeSkillAuthority(
                    advertised_names=frozenset({SKILL, *advertised}),
                    home_roots=(plugins,),
                    workspace_dirs=(".claude/skills",),
                    plugin_namespaces=namespaces,
                )

            # Another plugin's same-named skill must not vouch for this one.
            with self.assertRaisesRegex(ValueError, "not loaded by the bound provider"):
                _ = check_skill_catalogue(
                    reply, setup.fixture, authority("alpha:review")
                )
            result = check_skill_catalogue(
                reply, setup.fixture, authority("beta:review")
            )
            self.assertEqual(result.status, "supported")

    def test_workspace_roots_follow_the_providers_project_root(self) -> None:
        with NativeSetup() as setup:
            nested = setup.workspace / "packages/app"
            nested.mkdir(parents=True)
            authority = NativeSkillAuthority(
                advertised_names=frozenset(),
                home_roots=(),
                workspace_dirs=(".cursor/skills",),
                project_root_marker=".git",
            )
            # No marker anywhere: the native finder falls back to the cwd itself.
            self.assertEqual(authority.roots_for(nested), [nested / ".cursor/skills"])
            (setup.workspace / ".git").mkdir()
            self.assertEqual(
                authority.roots_for(nested), [setup.workspace / ".cursor/skills"]
            )

    def test_empty_or_wrong_fixture_rows_are_rejected(self) -> None:
        with NativeSetup() as setup:
            wrong_rows: list[list[JsonValue]] = [
                [],
                [skill_row(SKILL, setup.second_file)],
                [skill_row(SKILL, setup.skill_file, enabled=False)],
                [skill_row(SKILL, setup.skill_file, scope="user")],
                [skill_row(SKILL, Path("/builtin") / SKILL)],
            ]
            for rows in wrong_rows:
                reply = skills_reply(setup.workspace, rows)
                with self.subTest(rows=rows), self.assertRaises(ValueError):
                    _ = check_skill_catalogue(reply, setup.fixture, setup.authority())

    def test_skill_errors_must_name_an_actual_skill_source(self) -> None:
        with NativeSetup() as setup:
            manifest = (
                setup.home / "marketplaces/market/.claude-plugin/marketplace.json"
            )
            manifest.parent.mkdir(parents=True)
            _ = manifest.write_text("{}")
            wrong_roles: list[str] = [
                "builtin",
                str(setup.workspace),  # a real package/workspace directory
                str(manifest),  # the plugin role, not a skill source
                str(setup.codex_only),  # a skill file outside the provider's roots
            ]
            authority = setup.authority(shared_root=False)
            for path in wrong_roles:
                reply = skills_reply(
                    setup.workspace,
                    [skill_row(SKILL, setup.skill_file)],
                    [{"path": path, "message": "unreadable"}],
                )
                with (
                    self.subTest(path=path),
                    self.assertRaisesRegex(ValueError, "no actual native skill source"),
                ):
                    _ = check_skill_catalogue(reply, setup.fixture, authority)
            located = skills_reply(
                setup.workspace,
                [skill_row(SKILL, setup.skill_file)],
                [{"path": str(setup.second_file), "message": "unreadable"}],
            )
            result = check_skill_catalogue(located, setup.fixture, authority)
            self.assertEqual(result.error_count, 1)

    def test_explicit_error_is_unsupported_not_passed(self) -> None:
        with NativeSetup() as setup:
            result = check_skill_catalogue(
                error_reply(), setup.fixture, setup.authority()
            )
            self.assertEqual(result.status, "explicitUnsupported")

    def test_unloaded_fixture_leaves_no_reference(self) -> None:
        with NativeSetup() as setup:
            authority = NativeSkillAuthority(
                advertised_names=frozenset(), home_roots=(), workspace_dirs=()
            )
            with self.assertRaisesRegex(ValueError, "did not load the controlled"):
                _ = check_skill_catalogue(error_reply(), setup.fixture, authority)

    def test_other_workspace_never_borrows_fixture_skills(self) -> None:
        with NativeSetup() as setup:
            clean = skills_reply(
                setup.empty, [skill_row("user-native", setup.user_skill, "user")]
            )
            result = check_wrong_context_catalogue(
                clean, setup.fixture, setup.authority()
            )
            self.assertEqual(result.status, "supported")
            borrowed = skills_reply(setup.empty, [skill_row(SKILL, setup.skill_file)])
            with self.assertRaises(ValueError):
                _ = check_wrong_context_catalogue(
                    borrowed, setup.fixture, setup.authority()
                )


class InstalledPluginSkillSampleTests(unittest.TestCase):
    def test_installed_plugin_skill_must_appear_by_its_own_path(self) -> None:
        with NativeSetup() as setup:
            install = setup.home / ".claude/plugins/cache/market/alpha/1.0.0"
            skill_file = write_skill(install / "skills", "review")
            row = NativePluginRow(
                id="alpha@market", enabled=True, installPath=str(install)
            )
            present = skills_reply(
                setup.workspace,
                [
                    skill_row(SKILL, setup.skill_file),
                    skill_row("alpha:review", skill_file, "user"),
                ],
            )
            path = check_installed_plugin_skill_sample(
                present, setup.workspace, row, [skill_file]
            )
            self.assertEqual(path, str(skill_file))
            other_plugin_skill = write_skill(
                setup.home / ".claude/plugins/cache/market/beta/1.0.0/skills", "review"
            )
            wrong = [
                skills_reply(setup.workspace, [skill_row(SKILL, setup.skill_file)]),
                skills_reply(
                    setup.workspace,
                    [skill_row("beta:review", other_plugin_skill, "user")],
                ),
                error_reply(),
            ]
            for reply in wrong:
                with self.subTest(reply=reply), self.assertRaises(ValueError):
                    _ = check_installed_plugin_skill_sample(
                        reply, setup.workspace, row, [skill_file]
                    )


class PluginSetup:
    """A native install directory, its marketplace manifest and an unrelated one."""

    def __init__(self) -> None:
        self.directory: tempfile.TemporaryDirectory[str] = tempfile.TemporaryDirectory()
        root = Path(self.directory.name).resolve()
        self.install: Path = root / "cache/market/alpha/1.0.0"
        self.install.mkdir(parents=True)
        self.manifest: Path = (
            root / "marketplaces/market/.claude-plugin/marketplace.json"
        )
        self.other_manifest: Path = (
            root / "marketplaces/other/.claude-plugin/marketplace.json"
        )
        for manifest in (self.manifest, self.other_manifest):
            manifest.parent.mkdir(parents=True)
            _ = manifest.write_text("{}")
        beta = root / "cache/market/beta/1.0.0"
        self.package_file: Path = self.install / ".claude-plugin/plugin.json"
        self.package_file.parent.mkdir(parents=True)
        _ = self.package_file.write_text("{}")
        self.native: list[NativePluginRow] = [
            NativePluginRow(
                id="alpha@market", enabled=True, installPath=str(self.install)
            ),
            NativePluginRow(id="beta@market", enabled=False, installPath=str(beta)),
        ]
        # The provider's own marketplace export: each name with its actual manifest.
        self.marketplaces: list[NativeMarketplace] = [
            NativeMarketplace(name="market", manifest_path=self.manifest),
            NativeMarketplace(name="other", manifest_path=self.other_manifest),
        ]

    def __enter__(self) -> t.Self:
        return self

    def __exit__(self, *_: object) -> None:
        self.directory.cleanup()

    def reply(
        self,
        plugins: list[JsonValue],
        name: str = "market",
        errors: list[JsonValue] | None = None,
        path: Path | None = None,
    ) -> RpcReply:
        marketplace: JsonObject = {
            "name": name,
            "path": str(path or self.manifest),
            "plugins": plugins,
        }
        result: JsonObject = {
            "marketplaces": [marketplace],
            "marketplaceLoadErrors": errors or [],
        }
        return RpcReply.model_validate({"id": 3, "result": result})


def summary(plugin_id: str, enabled: bool, **extra: JsonValue) -> JsonObject:
    return {
        "id": plugin_id,
        "name": plugin_id.split("@")[0],
        "installed": True,
        "enabled": enabled,
        "installPolicy": "AVAILABLE",
        "authPolicy": "ON_INSTALL",
        "remotePluginId": None,
        "shareContext": None,
        **extra,
    }


def detail(**overrides: JsonValue) -> RpcReply:
    plugin: JsonObject = {
        "marketplaceName": "market",
        "summary": summary("alpha@market", True),
        "description": "Install and auth policies are Codex display defaults.",
        "skills": [
            {"name": "alpha:review", "description": "Review code", "enabled": True}
        ],
        "hooks": [{"key": "native-1", "eventName": "preToolUse"}],
        "mcpServers": ["search"],
    }
    plugin.update(overrides)
    return RpcReply.model_validate({"id": 5, "result": {"plugin": plugin}})


def facts(hooks: list[str] | None) -> NativePluginFacts:
    return NativePluginFacts(
        plugin_id="alpha@market",
        skill_descriptions={"review": "Review code"},
        hook_events=hooks,
        mcp_servers=["search"],
    )


class PluginCatalogueTests(unittest.TestCase):
    def test_exact_native_installation_is_supported(self) -> None:
        with PluginSetup() as setup:
            reply = setup.reply(
                [summary("alpha@market", True), summary("beta@market", False)]
            )
            result = check_plugin_catalogue(reply, setup.native, setup.marketplaces)
            self.assertEqual(result.installed_ids, ["alpha@market", "beta@market"])
            self.assertEqual(
                result.defaulted_policy_ids, ["alpha@market", "beta@market"]
            )

    def test_wrong_lists_are_rejected(self) -> None:
        with PluginSetup() as setup:
            both: list[JsonValue] = [
                summary("alpha@market", True),
                summary("beta@market", False),
            ]
            wrong = [
                setup.reply([summary("alpha@market", True)]),
                setup.reply([*both, summary("codex@openai", True)]),
                setup.reply(
                    [summary("alpha@market", False), summary("beta@market", False)]
                ),
                setup.reply(
                    [
                        summary("alpha@market", True, remotePluginId="remote-1"),
                        summary("beta@market", False),
                    ]
                ),
                setup.reply(both, name="guessed"),
            ]
            for reply in wrong:
                with self.subTest(reply=reply), self.assertRaises(ValueError):
                    _ = check_plugin_catalogue(reply, setup.native, setup.marketplaces)

    def test_unrelated_real_error_path_excuses_no_omission(self) -> None:
        with PluginSetup() as setup:
            reply = setup.reply(
                [summary("alpha@market", True)],
                errors=[
                    {"marketplacePath": str(setup.other_manifest), "message": "other"}
                ],
            )
            with self.assertRaisesRegex(ValueError, "omits installed plugin beta"):
                _ = check_plugin_catalogue(reply, setup.native, setup.marketplaces)

    def test_only_the_actual_marketplace_manifest_qualifies(self) -> None:
        with PluginSetup() as setup:
            both: list[JsonValue] = [
                summary("alpha@market", True),
                summary("beta@market", False),
            ]
            alpha: list[JsonValue] = [summary("alpha@market", True)]
            package_error: list[JsonValue] = [
                {"marketplacePath": str(setup.package_file), "message": "beta"}
            ]
            wrong = [
                # A real package file is the wrong source role for a marketplace error.
                setup.reply(alpha, errors=package_error),
                # A real file that is not this marketplace's manifest.
                setup.reply(both, path=setup.package_file),
                setup.reply(both, path=setup.other_manifest),
            ]
            for reply in wrong:
                with self.subTest(reply=reply), self.assertRaises(ValueError):
                    _ = check_plugin_catalogue(reply, setup.native, setup.marketplaces)
            # Nothing omitted: the wrong source role alone must fail the list.
            with self.assertRaisesRegex(ValueError, "names no actual manifest"):
                _ = check_plugin_catalogue(
                    setup.reply(both, errors=package_error),
                    setup.native,
                    setup.marketplaces,
                )
            unexported = [NativeMarketplace(name="market", manifest_path=None)]
            with self.assertRaisesRegex(
                ValueError, "no manifest in the provider export"
            ):
                _ = check_plugin_catalogue(setup.reply(both), setup.native, unexported)

    def test_own_marketplace_error_explains_its_omission(self) -> None:
        with PluginSetup() as setup:
            reply = setup.reply(
                [summary("alpha@market", True)],
                errors=[{"marketplacePath": str(setup.manifest), "message": "beta"}],
            )
            self.assertEqual(
                check_plugin_catalogue(reply, setup.native, setup.marketplaces).status,
                "supported",
            )

    def test_management_must_be_rejected_without_native_change(self) -> None:
        with PluginSetup() as setup:
            check_management_rejected(error_reply(), setup.native, setup.native)
            accepted = RpcReply.model_validate({"id": 4, "result": {}})
            with self.assertRaises(ValueError):
                check_management_rejected(accepted, setup.native, setup.native)
            with self.assertRaises(ValueError):
                check_management_rejected(error_reply(), setup.native, setup.native[:1])

    def test_detail_items_from_native_facts_are_supported(self) -> None:
        with PluginSetup() as setup:
            status = check_plugin_detail(
                detail(), setup.native[0], facts(["PreToolUse"])
            )
            self.assertEqual(status, "supported")

    def test_invented_or_dropped_detail_items_are_rejected(self) -> None:
        invented: JsonObject = {
            "name": "review",
            "description": "Invented",
            "enabled": True,
        }
        wrong = [
            detail(skills=[invented]),
            detail(skills=[]),
            detail(hooks=[]),
            detail(hooks=[{"key": "k", "eventName": "sessionStart"}]),
            detail(mcpServers=[]),
            detail(description="No attribution"),
            detail(marketplaceName="guessed"),
        ]
        with PluginSetup() as setup:
            for reply in wrong:
                with self.subTest(reply=reply), self.assertRaises(ValueError):
                    _ = check_plugin_detail(
                        reply, setup.native[0], facts(["PreToolUse"])
                    )

    def test_unobserved_hooks_require_the_method_error(self) -> None:
        with PluginSetup() as setup:
            with self.assertRaisesRegex(ValueError, "hooks"):
                _ = check_plugin_detail(detail(hooks=[]), setup.native[0], facts(None))
            status = check_plugin_detail(error_reply(), setup.native[0], facts(None))
            self.assertEqual(status, "explicitUnsupported")


if __name__ == "__main__":
    _ = unittest.main()
