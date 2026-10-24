"""Pure J1 oracles: skill and plugin discovery and management rejection.

Catalogue authority is the bound provider: its own advertised names and skill roots,
its own plugin export and the plugin's own files. Expected values never come from the
candidate's response. An explicit JSON-RPC error is `explicitUnsupported`, never a pass
of the experience; a wrong build raises ValueError.
"""

from pathlib import Path

from scripts.verification.native_capabilities_models import (
    CapabilityStatus,
    FixtureRecord,
    NativeMarketplace,
    NativePluginFacts,
    NativePluginRow,
    NativeSkillAuthority,
    PluginCatalogueCheck,
    PluginListView,
    PluginReadView,
    PluginSummaryView,
    RpcReply,
    SkillCatalogueCheck,
    SkillsEntryView,
    SkillsListView,
)
from scripts.verification.native_capabilities_oracle import same_path


def require_native_fixture_loaded(
    fixture: FixtureRecord, advertised: list[str]
) -> None:
    # The provider's own command list is the native reference that the fixture is in
    # this setup; without it no catalogue answer can be judged.
    if fixture.skill_name not in advertised:
        raise ValueError("native provider did not load the controlled fixture skill")


def workspace_entry(view: SkillsListView, cwd: Path) -> SkillsEntryView:
    entries = [entry for entry in view.data if same_path(entry.cwd, cwd)]
    if len(entries) != 1:
        raise ValueError("skills/list returned no single entry for the requested cwd")
    return entries[0]


def natively_advertised(name: str, path: Path, authority: NativeSkillAuthority) -> bool:
    """The provider advertises this skill under the namespace its file's owner implies.

    A skill inside an installed plugin's directory must be advertised as
    `namespace:skill` for that plugin; any other skill only under its exact name. A
    same-named skill of a different plugin never matches.
    """
    namespace = authority.namespace_for(path)
    if namespace is None:
        return name in authority.advertised_names
    qualified = name if name.startswith(f"{namespace}:") else f"{namespace}:{name}"
    return qualified in authority.advertised_names


def frontmatter_fields(skill_path: Path) -> dict[str, str]:
    """Single-line `key: value` fields from the native SKILL.md frontmatter."""
    skill_file = (
        skill_path if skill_path.name == "SKILL.md" else skill_path / "SKILL.md"
    )
    lines = skill_file.read_text().splitlines() if skill_file.is_file() else []
    if not lines or lines[0].strip() != "---":
        return {}
    fields: dict[str, str] = {}
    for line in lines[1:]:
        if line.strip() == "---":
            break
        key, separator, value = line.partition(":")
        if separator:
            fields[key.strip()] = value.strip().strip("'\"")
    return fields


def natively_disabled(path: Path, authority: NativeSkillAuthority) -> bool:
    """The provider's own loader skips this file, by a declared native frontmatter fact."""
    fields = frontmatter_fields(path)
    return any(
        fields.get(key, "").lower() == value.lower()
        for key, value in authority.disabled_markers
    )


def require_truthful_skill_entry(
    entry: SkillsEntryView, cwd: Path, authority: NativeSkillAuthority
) -> None:
    """Catalogue authority is the bound provider, not a path prefix.

    Every row's real file lies under one of that provider's own roots for this cwd; a
    shared root such as ~/.codex/skills is legitimate when the provider's loader reads
    it. Availability is judged from native facts, never from the row's own flag:
    - An enabled row is advertised by the provider under its file's namespace and
      carries no native disabled fact, so a substituted Codex catalogue fails.
    - A disabled row (FilesystemCatalogues keeps real skipped files) carries a declared
      native disabled fact in its own file; a row merely marked disabled fails.
    - A disabled file never stands in for an advertised active definition of its name.
    """
    roots = [root.resolve() for root in authority.roots_for(cwd)]
    for skill in entry.skills:
        path = Path(skill.path)
        if not path.is_absolute() or not path.exists():
            raise ValueError(f"skill {skill.name!r} has no real filesystem origin")
        if not any(path.resolve().is_relative_to(root) for root in roots):
            raise ValueError(
                f"skill {skill.name!r} lies outside the provider's own roots"
            )
        disabled = natively_disabled(path, authority)
        if skill.enabled and disabled:
            raise ValueError(f"natively disabled skill {skill.name!r} reported enabled")
        if skill.enabled and not natively_advertised(skill.name, path, authority):
            raise ValueError(
                f"skill {skill.name!r} is not loaded by the bound provider"
            )
        if not skill.enabled and not disabled:
            raise ValueError(
                f"skill {skill.name!r} reported disabled without a native disabled fact"
            )
    enabled_names = {skill.name for skill in entry.skills if skill.enabled}
    for skill in entry.skills:
        if (
            not skill.enabled
            and skill.name in authority.advertised_names
            and skill.name not in enabled_names
        ):
            raise ValueError(
                f"disabled file shadows the active native definition of {skill.name!r}"
            )
    for error in entry.errors:
        if not is_skill_source(Path(error.path), roots):
            raise ValueError("skill error names no actual native skill source")


def is_skill_source(path: Path, roots: list[Path]) -> bool:
    """Program Design `SkillSource`: a provider skill file or its directory, within the
    provider's own roots. A package directory, a marketplace manifest or any other real
    path is the wrong role (decision table, Accounted with a wrong-role error)."""
    if not path.is_absolute() or not path.exists():
        return False
    is_skill = path.name == "SKILL.md" or (path / "SKILL.md").is_file()
    return is_skill and any(path.resolve().is_relative_to(root) for root in roots)


def check_skill_catalogue(
    reply: RpcReply, fixture: FixtureRecord, authority: NativeSkillAuthority
) -> SkillCatalogueCheck:
    """J1 skills: the controlled workspace skill appears with its real origin (C1, R1)."""
    require_native_fixture_loaded(fixture, sorted(authority.advertised_names))
    if reply.error is not None:
        return SkillCatalogueCheck(
            status="explicitUnsupported", unsupported_message=reply.error.message
        )
    entry = workspace_entry(
        SkillsListView.model_validate(reply.result), fixture.workspace
    )
    require_truthful_skill_entry(entry, fixture.workspace, authority)
    matches = [skill for skill in entry.skills if skill.name == fixture.skill_name]
    if len(matches) != 1:
        raise ValueError("controlled native skill missing: empty or foreign catalogue")
    skill = matches[0]
    # Either the skill file or its directory is the actual origin; nothing else is.
    if not (
        same_path(skill.path, fixture.skill_file)
        or same_path(skill.path, fixture.skill_file.parent)
    ):
        raise ValueError("controlled native skill reported with a different origin")
    if not skill.enabled:
        raise ValueError("enabled native skill reported disabled")
    if skill.scope != "repo":
        raise ValueError("workspace skill reported outside the workspace scope")
    return SkillCatalogueCheck(
        status="supported",
        skill_count=len(entry.skills),
        error_count=len(entry.errors),
        fixture_path=skill.path,
    )


def check_wrong_context_catalogue(
    reply: RpcReply, fixture: FixtureRecord, authority: NativeSkillAuthority
) -> SkillCatalogueCheck:
    """J1 negative: another workspace never borrows the fixture workspace's skills."""
    if reply.error is not None:
        return SkillCatalogueCheck(
            status="explicitUnsupported", unsupported_message=reply.error.message
        )
    entry = workspace_entry(
        SkillsListView.model_validate(reply.result), fixture.empty_workspace
    )
    require_truthful_skill_entry(entry, fixture.empty_workspace, authority)
    for skill in entry.skills:
        if skill.name in {fixture.skill_name, fixture.second_skill_name} or Path(
            skill.path
        ).resolve().is_relative_to(fixture.workspace.resolve()):
            raise ValueError("catalogue borrowed another workspace's native skill")
        if skill.scope == "repo" and not Path(skill.path).resolve().is_relative_to(
            fixture.empty_workspace.resolve()
        ):
            raise ValueError("repo-scope skill outside the requested workspace")
    return SkillCatalogueCheck(
        status="supported", skill_count=len(entry.skills), error_count=len(entry.errors)
    )


def check_plugin_catalogue(
    reply: RpcReply,
    native_rows: list[NativePluginRow],
    marketplaces: list[NativeMarketplace],
) -> PluginCatalogueCheck:
    """J1 plugins: the installed list is exactly the provider's own export (C1, C4, D2).

    Marketplace rows and load errors name only the actual manifest file from the
    provider's own marketplace export (Program Design `MarketplaceManifest`); a package
    directory, a name match or any other real path never qualifies.
    """
    if reply.error is not None:
        return PluginCatalogueCheck(
            status="explicitUnsupported", unsupported_message=reply.error.message
        )
    view = PluginListView.model_validate(reply.result)
    native = {row.id: row for row in native_rows}
    manifests = {
        market.name: market.manifest_path
        for market in marketplaces
        if market.manifest_path is not None
    }
    listed: dict[str, PluginSummaryView] = {}
    for marketplace in view.marketplaces:
        manifest = manifests.get(marketplace.name)
        if manifest is None:
            raise ValueError(
                f"marketplace {marketplace.name} has no manifest in the provider export"
            )
        if marketplace.path is None or not same_path(marketplace.path, manifest):
            raise ValueError(
                f"marketplace {marketplace.name} does not name its actual manifest"
            )
        for plugin in marketplace.plugins:
            if plugin.remotePluginId is not None or plugin.shareContext is not None:
                raise ValueError(
                    f"plugin {plugin.id} carries a manufactured remote identity"
                )
            if plugin.installed:
                # Native ids are `name@marketplace`; the row must sit under that marketplace.
                if plugin.id.rpartition("@")[2] != marketplace.name:
                    raise ValueError(
                        f"plugin {plugin.id} listed under a guessed marketplace"
                    )
                listed[plugin.id] = plugin
    if foreign := sorted(set(listed) - set(native)):
        raise ValueError(f"plugins outside the provider's installation: {foreign}")
    for error in view.marketplaceLoadErrors:
        if not any(
            same_path(error.marketplacePath, manifest)
            for manifest in manifests.values()
        ):
            raise ValueError("marketplace load error names no actual manifest file")
    for plugin_id in sorted(set(native) - set(listed)):
        # Each omission needs its own marketplace's manifest error; another path excuses nothing.
        own_manifest = manifests.get(plugin_id.rpartition("@")[2])
        if own_manifest is None or not any(
            same_path(error.marketplacePath, own_manifest)
            for error in view.marketplaceLoadErrors
        ):
            raise ValueError(
                f"complete-looking list omits installed plugin {plugin_id}"
            )
    for plugin_id, plugin in listed.items():
        if plugin.enabled != native[plugin_id].enabled:
            raise ValueError(f"plugin {plugin_id} enablement differs from the provider")
    return PluginCatalogueCheck(
        status="supported",
        installed_ids=sorted(listed),
        defaulted_policy_ids=sorted(
            plugin_id
            for plugin_id, plugin in listed.items()
            if plugin.installPolicy == "AVAILABLE" and plugin.authPolicy == "ON_INSTALL"
        ),
        load_error_paths=[
            error.marketplacePath for error in view.marketplaceLoadErrors
        ],
    )


def event_key(name: str) -> str:
    return "".join(character for character in name.lower() if character.isalnum())


def check_plugin_detail(
    reply: RpcReply, native_row: NativePluginRow, facts: NativePluginFacts
) -> CapabilityStatus:
    """J1 plugin/read: the exact selector resolves to the provider's own record (C1, C4).

    Every required detail item comes from the plugin's own native facts (M1): skill
    descriptions, hooks with native events, MCP server names. A fact the
    native files don't establish must produce the method error, not an empty array.
    Codex AVAILABLE/ON_INSTALL defaults must be attributed in existing detail text.
    """
    if reply.error is not None:
        return "explicitUnsupported"
    detail = PluginReadView.model_validate(reply.result).plugin
    if detail.summary.id != native_row.id:
        raise ValueError("plugin/read resolved a different plugin")
    if detail.summary.enabled != native_row.enabled:
        raise ValueError("plugin/read enablement differs from the provider")
    if detail.marketplaceName != native_row.id.rpartition("@")[2]:
        raise ValueError("plugin/read reports a guessed marketplace")
    if (
        detail.summary.remotePluginId is not None
        or detail.summary.shareContext is not None
    ):
        raise ValueError("plugin/read carries a manufactured remote identity")
    if (
        detail.summary.installPolicy == "AVAILABLE"
        and detail.summary.authPolicy == "ON_INSTALL"
    ):
        text = (detail.description or "").lower()
        if "codex" not in text or "default" not in text:
            raise ValueError("Codex policy defaults shown without attribution")
    # Claude namespaces plugin skills as `plugin:skill`; either form names the same skill.
    namespace = f"{detail.summary.name}:"
    names = {skill.name.removeprefix(namespace): skill for skill in detail.skills}
    if len(names) != len(detail.skills) or set(names) != set(facts.skill_descriptions):
        raise ValueError("plugin/read skills differ from the plugin's own skills")
    for name, skill in names.items():
        if skill.description != facts.skill_descriptions[name]:
            raise ValueError(f"skill {skill.name} description is not the native one")
    # Per-skill enablement has native overrides (user-invocable, configured state) that
    # this oracle has not qualified; package enablement is no stand-in. Reported as a gap.
    if facts.hook_events is None:
        # Unobserved hooks can't be shown as an empty list or any list at all.
        raise ValueError("plugin/read shows hooks the native files do not establish")
    native_events = {event_key(event) for event in facts.hook_events}
    if {event_key(hook.eventName) for hook in detail.hooks} != native_events:
        raise ValueError("plugin/read hooks dropped or invented native hook events")
    if facts.mcp_servers is None or sorted(detail.mcpServers) != sorted(
        facts.mcp_servers
    ):
        raise ValueError("plugin/read MCP servers differ from the native declaration")
    return "supported"


def check_installed_plugin_skill_sample(
    reply: RpcReply, workspace: Path, row: NativePluginRow, skill_files: list[Path]
) -> str:
    """J1 sample beside the fixture: a real installed plugin's own skill file appears.

    Discovery only; nothing is executed. Its namespace provenance is checked with every
    row by `require_truthful_skill_entry`. Per-skill enablement stays an explicit gap:
    native overrides are unqualified here, and package enablement is no stand-in.
    """
    if reply.error is not None:
        raise ValueError("skills/list failed, so no installed sample can be compared")
    entry = workspace_entry(SkillsListView.model_validate(reply.result), workspace)
    matches = [
        skill
        for skill in entry.skills
        if any(
            same_path(skill.path, skill_file)
            or same_path(skill.path, skill_file.parent)
            for skill_file in skill_files
        )
    ]
    if not matches:
        raise ValueError(
            f"installed plugin {row.id} skills are missing from skills/list"
        )
    return matches[0].path


def check_management_rejected(
    reply: RpcReply,
    native_before: list[NativePluginRow],
    native_after: list[NativePluginRow],
) -> None:
    """Installation management is rejected with no change to the native setup (C4)."""
    if reply.error is None:
        raise ValueError("management write was accepted")
    if [row.model_dump() for row in native_before] != [
        row.model_dump() for row in native_after
    ]:
        raise ValueError(
            "native installation changed around a rejected management write"
        )
